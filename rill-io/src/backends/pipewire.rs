//! PipeWire backend for Linux
//!
//! Uses `pipewire` (0.9) with `MainLoopRc` / `ContextRc` / `StreamBox`.
//!
//! The backend implements [`IoDriver`], [`IoCapture`], and [`IoPlayback`].
//! Streams are created based on channel counts:
//! - `output_channels > 0` → output stream (playback)
//! - `input_channels > 0`  → input stream (capture)
//!
//! The output callback always drives the graph (when output exists),
//! reading the most recent input DMA pointer from a shared slot.
//! When only input exists, the input callback drives the graph.
//!
//! No ring buffers — all DMA access is zero-copy through raw pointers
//! valid for the duration of the PipeWire processing cycle.

use std::cell::UnsafeCell;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::sys as spa_sys;

use crate::config::AudioConfig;
use crate::error::{IoError, IoResult as IoErrorResult};
use rill_core::io::{IoCapture, IoDriver, IoPlayback, IoResult};
use rill_core::time::ClockTick;

// ============================================================================
// CbSlot — stores the process callback
// ============================================================================

#[derive(Copy, Clone)]
struct CbSlot(usize);

impl CbSlot {
    fn new() -> Self {
        Self(Box::into_raw(Box::new(None::<Box<dyn FnMut(&ClockTick)>>)) as usize)
    }
    unsafe fn set(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        (*(self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>)) = Some(cb);
    }
    unsafe fn call(&self, tick: &ClockTick) {
        if let Some(ref mut cb) = *(self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>) {
            cb(tick);
        }
    }
    unsafe fn drop_box(&self) {
        drop(Box::from_raw(
            self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>,
        ));
    }
}

// ============================================================================
// Slots for DMA pointers (shared between PW callbacks and trait methods)
// ============================================================================

struct InputWindowSlot(UnsafeCell<Option<(*const f32, usize, usize)>>);

impl InputWindowSlot {
    fn new() -> Self {
        Self(UnsafeCell::new(None))
    }
    unsafe fn set(&self, ptr: *const f32, channels: usize, frames: usize) {
        *self.0.get() = Some((ptr, channels, frames));
    }
    unsafe fn clear(&self) {
        *self.0.get() = None;
    }
    unsafe fn get(&self) -> Option<(*const f32, usize, usize)> {
        *self.0.get()
    }
    unsafe fn advance(&self, chunk: usize) {
        if let Some((ref mut ptr, channels, ref mut frames)) = *self.0.get() {
            let step = chunk * channels;
            *ptr = unsafe { ptr.add(step) };
            *frames = frames.saturating_sub(chunk);
        }
    }
}

unsafe impl Send for InputWindowSlot {}
unsafe impl Sync for InputWindowSlot {}

struct OutputWindowSlot(UnsafeCell<Option<(*mut f32, usize, usize)>>);

impl OutputWindowSlot {
    fn new() -> Self {
        Self(UnsafeCell::new(None))
    }
    unsafe fn set(&self, ptr: *mut f32, channels: usize, frames: usize) {
        *self.0.get() = Some((ptr, channels, frames));
    }
    unsafe fn clear(&self) {
        *self.0.get() = None;
    }
    unsafe fn get(&self) -> Option<(*mut f32, usize, usize)> {
        *self.0.get()
    }
}

unsafe impl Send for OutputWindowSlot {}
unsafe impl Sync for OutputWindowSlot {}

// ============================================================================
// PipewireBackend
// ============================================================================

/// PipeWire audio backend — implements [`IoDriver`], [`IoCapture`], [`IoPlayback`].
pub struct PipewireBackend {
    config: AudioConfig,
    process_cb: CbSlot,
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    sample_pos: Arc<AtomicU64>,
    negotiated_input_channels: Arc<AtomicU32>,
    negotiated_input_rate: Arc<AtomicU32>,
    negotiated_output_rate: Arc<AtomicU32>,
    negotiated_output_channels: Arc<AtomicU32>,
    input_window: InputWindowSlot,
    output_window: OutputWindowSlot,
}

impl fmt::Debug for PipewireBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipewireBackend")
            .field("config", &self.config)
            .finish()
    }
}

impl PipewireBackend {
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        if !cfg!(target_os = "linux") {
            return Err(
                IoError::Unsupported("PipeWire is only available on Linux".into()).to_string(),
            );
        }
        let input_channels = config.input_channels;
        let output_channels = config.output_channels;
        let sample_rate = config.sample_rate;
        Ok(Self {
            config,
            process_cb: CbSlot::new(),
            xruns: Arc::new(AtomicU32::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            sample_pos: Arc::new(AtomicU64::new(0)),
            negotiated_input_channels: Arc::new(AtomicU32::new(input_channels)),
            negotiated_input_rate: Arc::new(AtomicU32::new(sample_rate)),
            negotiated_output_rate: Arc::new(AtomicU32::new(sample_rate)),
            negotiated_output_channels: Arc::new(AtomicU32::new(output_channels)),
            input_window: InputWindowSlot::new(),
            output_window: OutputWindowSlot::new(),
        })
    }

    pub fn negotiated_rate(&self) -> u32 {
        self.negotiated_input_rate.load(Ordering::Relaxed)
    }

    pub fn negotiated_channels(&self) -> u32 {
        self.negotiated_input_channels.load(Ordering::Relaxed)
    }
}

// ============================================================================
// IoDriver impl
// ============================================================================

impl IoDriver for PipewireBackend {
    fn set_process_callback(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        unsafe {
            self.process_cb.set(cb);
        }
    }

    fn run(&self, running: Arc<AtomicBool>) -> IoResult<()> {
        let process_cb = self.process_cb;
        let xruns = self.xruns.clone();
        let sample_rate = self.config.sample_rate;
        let out_channels = self.config.output_channels;
        let in_channels = self.config.input_channels;
        let out_device = self.config.output_device.clone();
        let in_device = self.config.input_device.clone();
        let block_size = self.config.buffer_size as usize;
        let input_window = &self.input_window as *const InputWindowSlot;
        let output_window = &self.output_window as *const OutputWindowSlot;
        let out_spos = self.sample_pos.clone();
        let sample_pos = self.sample_pos.clone();

        let out_nchan = self.negotiated_output_channels.clone();
        let out_nrate = self.negotiated_output_rate.clone();
        let in_nch_fmt = self.negotiated_input_channels.clone();
        let in_nrate_fmt = self.negotiated_input_rate.clone();
        let in_nch_proc = self.negotiated_input_channels.clone();
        let in_nrate_proc = self.negotiated_input_rate.clone();

        let out_chan = out_channels;

        pw::init();

        let mainloop =
            pw::main_loop::MainLoopRc::new(None).map_err(|e| format!("PW MainLoopRc::new: {e}"))?;
        let context = pw::context::ContextRc::new(&mainloop, None)
            .map_err(|e| format!("PW ContextRc::new: {e}"))?;
        let core = context
            .connect_rc(None)
            .map_err(|e| format!("PW core.connect_rc: {e}"))?;

        // ── Output stream ────────────────────────────────────────────────
        let _out_stream;
        let _out_listener: Option<_>;
        let out_ml = mainloop.clone();
        let out_running = running.clone();

        if out_channels > 0 {
            let out_node = out_device.as_deref().unwrap_or("rill-output");
            let out_desc = format!("Rill Audio Output ({out_node})");
            let mut out_props = properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_ROLE => "Music",
                *pw::keys::MEDIA_CATEGORY => "Playback",
                *pw::keys::NODE_NAME => out_node,
                *pw::keys::NODE_DESCRIPTION => out_desc.as_str(),
            };
            out_props.insert("audio.channels", out_chan.to_string());

            let stream =
                pw::stream::StreamBox::new(&core, &format!("{out_node}-output"), out_props)
                    .map_err(|e| format!("PW StreamBox output: {e}"))?;

            let out_sr = sample_rate;
            let out_nchan_proc_pc = out_nchan.clone();
            let out_nrate_proc_pc = out_nrate.clone();
            let out_nchan_proc = out_nchan.clone();
            let out_nrate_proc = out_nrate.clone();
            let has_input = in_channels > 0;

            let listener = stream
                .add_local_listener_with_user_data(())
                .param_changed(move |_stream, _data, id, param| {
                    if id == spa_sys::SPA_PARAM_Format {
                        if let Some(param) = param {
                            let mut ai = spa::param::audio::AudioInfoRaw::new();
                            if ai.parse(param).is_ok() {
                                out_nrate_proc_pc.store(ai.rate(), Ordering::Relaxed);
                                out_nchan_proc_pc.store(ai.channels(), Ordering::Relaxed);
                            }
                        }
                    }
                })
                .process(move |s, _| {
                    if !out_running.load(Ordering::Acquire) {
                        out_ml.quit();
                        return;
                    }
                    let mut buf = match s.dequeue_buffer() {
                        Some(b) => b,
                        None => return,
                    };
                    let datas = buf.datas_mut();
                    if datas.is_empty() {
                        return;
                    }
                    let data = &mut datas[0];

                    let (ck_stride, ck_size) = {
                        let ck = data.chunk();
                        (ck.stride() as usize, ck.size() as usize)
                    };

                    let slice = match data.data() {
                        Some(s) => s,
                        None => return,
                    };

                    let stride = if ck_stride > 0 {
                        ck_stride
                    } else {
                        let actual_ch = out_nchan_proc.load(Ordering::Relaxed) as usize;
                        if actual_ch > 0 {
                            actual_ch * 4
                        } else {
                            out_chan as usize * 4
                        }
                    };
                    let n_frames = if ck_stride > 0 && ck_size > 0 {
                        ck_size / ck_stride
                    } else {
                        slice.len() / stride
                    };
                    let total_samps = n_frames * (stride / 4);

                    let out_ptr = slice.as_mut_ptr() as *mut f32;

                    let mut offset = 0usize;
                    while offset < n_frames {
                        let chunk = (n_frames - offset).min(block_size);
                        let pos = out_spos.fetch_add(chunk as u64, Ordering::Relaxed);

                        // Build tick (timing only, no view)
                        let mut tick =
                            ClockTick::new(pos, chunk as u32, out_sr as f32, "pipewire".into());
                        let nrate = out_nrate_proc.load(Ordering::Relaxed) as f64;
                        let config_rate = out_sr as f64;
                        tick.speed_ratio = if nrate > 0.0 && (config_rate - nrate).abs() > 1.0 {
                            config_rate / nrate
                        } else {
                            1.0
                        };

                        // Store output DMA window for write_output()
                        unsafe {
                            output_window.as_ref().unwrap().set(
                                out_ptr.add(offset * out_chan as usize),
                                out_chan as usize,
                                chunk,
                            );
                            process_cb.call(&tick);
                            output_window.as_ref().unwrap().clear();
                            if has_input {
                                input_window.as_ref().unwrap().advance(chunk);
                            }
                        }

                        offset += chunk;
                    }

                    // Zero-fill remainder
                    let filled_samps = offset * out_chan as usize;
                    if filled_samps < total_samps {
                        let samples: &mut [f32] =
                            unsafe { std::slice::from_raw_parts_mut(out_ptr, total_samps) };
                        samples[filled_samps..].fill(0.0);
                    }

                    let ck = data.chunk_mut();
                    *ck.offset_mut() = 0;
                    *ck.stride_mut() = stride as i32;
                    *ck.size_mut() = (total_samps * 4) as u32;
                })
                .register()
                .map_err(|e| format!("PW output listener: {e}"))?;
            _out_listener = Some(listener);

            let mut audio_info = spa::param::audio::AudioInfoRaw::new();
            audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
            audio_info.set_rate(sample_rate);
            audio_info.set_channels(out_chan);
            let mut position = [0; spa::param::audio::MAX_CHANNELS];
            if out_chan >= 1 {
                position[0] = spa_sys::SPA_AUDIO_CHANNEL_FL;
            }
            if out_chan >= 2 {
                position[1] = spa_sys::SPA_AUDIO_CHANNEL_FR;
            }
            audio_info.set_position(position);

            let params_bytes: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
                std::io::Cursor::new(Vec::new()),
                &spa::pod::Value::Object(spa::pod::Object {
                    type_: spa_sys::SPA_TYPE_OBJECT_Format,
                    id: spa_sys::SPA_PARAM_EnumFormat,
                    properties: audio_info.into(),
                }),
            )
            .unwrap()
            .0
            .into_inner();
            let mut out_params = [spa::pod::Pod::from_bytes(&params_bytes).unwrap()];

            if let Err(e) = stream.connect(
                spa::utils::Direction::Output,
                None,
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut out_params,
            ) {
                return Err(format!("PW output connect: {e}"));
            }

            _out_stream = Some(stream);
        }

        // ── Input stream ─────────────────────────────────────────────────
        let _in_stream;
        let _in_listener: Option<_>;
        let in_ml = mainloop.clone();
        let in_running = running.clone();

        if in_channels > 0 {
            let in_node = in_device.as_deref().unwrap_or("rill-input");
            let in_desc = format!("Rill Audio Input ({in_node})");
            let mut in_props = properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_ROLE => "Music",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::NODE_NAME => in_node,
                *pw::keys::NODE_DESCRIPTION => in_desc.as_str(),
            };
            in_props.insert("audio.channels", in_channels.to_string());
            in_props.insert(
                *pw::keys::NODE_LATENCY,
                format!("{}/{}", block_size, sample_rate),
            );

            let stream =
                match pw::stream::StreamBox::new(&core, &format!("{in_node}-input"), in_props) {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("PW StreamBox input: {e} — capture disabled");
                        return Err(format!("PW StreamBox input: {e}"));
                    }
                };

            let no_output = out_channels == 0;

            let listener = stream
                .add_local_listener_with_user_data(())
                .param_changed(move |_stream, _data, id, param| {
                    if id == spa_sys::SPA_PARAM_Format {
                        if let Some(param) = param {
                            let mut ai = spa::param::audio::AudioInfoRaw::new();
                            if ai.parse(param).is_ok() {
                                in_nch_fmt.store(ai.channels(), Ordering::Relaxed);
                                in_nrate_fmt.store(ai.rate(), Ordering::Relaxed);
                            }
                        }
                    }
                })
                .process(move |stream, _| {
                    if !in_running.load(Ordering::Acquire) {
                        in_ml.quit();
                        return;
                    }
                    let mut buf = match stream.dequeue_buffer() {
                        Some(b) => b,
                        None => {
                            xruns.fetch_add(1, Ordering::Relaxed);
                            return;
                        }
                    };
                    let datas = buf.datas_mut();
                    if datas.is_empty() {
                        return;
                    }
                    let data = &mut datas[0];

                    let actual_channels = {
                        let c = in_nch_proc.load(Ordering::Relaxed);
                        if c > 0 {
                            c as usize
                        } else {
                            in_channels as usize
                        }
                    };

                    let chunk_offset;
                    let chunk_size;
                    {
                        let ck = data.chunk_mut();
                        if ck.flags().contains(spa::buffer::ChunkFlags::CORRUPTED) {
                            xruns.fetch_add(1, Ordering::Relaxed);
                            return;
                        }
                        chunk_offset = ck.offset() as usize;
                        chunk_size = ck.size() as usize;
                    }

                    if chunk_size == 0 {
                        return;
                    }

                    let slice = match data.data() {
                        Some(s) => s,
                        None => return,
                    };

                    let data_start = chunk_offset.min(slice.len());
                    let data_end = (chunk_offset + chunk_size).min(slice.len());
                    let sample_bytes = &slice[data_start..data_end];
                    let total_samps = sample_bytes.len() / 4;
                    let n_frames = total_samps / actual_channels.max(1);
                    let in_ptr = sample_bytes.as_ptr() as *const f32;

                    // Store input DMA pointer for read_input()
                    unsafe {
                        input_window
                            .as_ref()
                            .unwrap()
                            .set(in_ptr, actual_channels, n_frames);
                    }

                    if no_output {
                        // Input-only: drive the graph from the input callback.
                        // Chunk the DMA buffer into block_size ticks so the graph
                        // processes all available frames.
                        let sr = in_nrate_proc.load(Ordering::Relaxed) as f32;
                        let effective_sr = if sr > 0.0 { sr } else { sample_rate as f32 };
                        let config_rate = sample_rate as f64;
                        let actual_rate = effective_sr as f64;
                        let speed_ratio = if (config_rate - actual_rate).abs() > 1.0 {
                            config_rate / actual_rate
                        } else {
                            1.0
                        };

                        let mut offset = 0usize;
                        while offset < n_frames {
                            let chunk = (n_frames - offset).min(block_size);
                            let pos = sample_pos.fetch_add(chunk as u64, Ordering::Relaxed);
                            let mut tick =
                                ClockTick::new(pos, chunk as u32, effective_sr, "pipewire".into());
                            tick.speed_ratio = speed_ratio;
                            unsafe {
                                input_window.as_ref().unwrap().set(
                                    in_ptr.add(offset * actual_channels),
                                    actual_channels,
                                    chunk,
                                );
                                process_cb.call(&tick);
                            }
                            offset += chunk;
                        }
                        unsafe {
                            input_window.as_ref().unwrap().clear();
                        }
                    }
                    // When output exists, leave the input window set —
                    // the output callback will read it and clear it.
                })
                .register()
                .map_err(|e| format!("PW input listener: {e}"))?;
            _in_listener = Some(listener);

            let mut in_ai = spa::param::audio::AudioInfoRaw::new();
            in_ai.set_format(spa::param::audio::AudioFormat::F32LE);

            let in_params_bytes: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
                std::io::Cursor::new(Vec::new()),
                &spa::pod::Value::Object(spa::pod::Object {
                    type_: spa_sys::SPA_TYPE_OBJECT_Format,
                    id: spa_sys::SPA_PARAM_EnumFormat,
                    properties: in_ai.into(),
                }),
            )
            .unwrap()
            .0
            .into_inner();
            let mut in_params = [spa::pod::Pod::from_bytes(&in_params_bytes).unwrap()];

            if let Err(e) = stream.connect(
                spa::utils::Direction::Input,
                None,
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut in_params,
            ) {
                return Err(format!("PW input connect: {e}"));
            }

            _in_stream = Some(stream);
        }

        self.running.store(true, Ordering::Release);

        // ── PW event loop ───────────────────────────────────────────────
        mainloop.run();

        Ok(())
    }

    fn stop(&self) -> IoResult<()> {
        self.running.store(false, Ordering::Release);
        Ok(())
    }
}

// ============================================================================
// IoCapture impl
// ============================================================================

impl IoCapture for PipewireBackend {
    fn read_input(&self, channel: usize, dst: &mut [f32]) -> usize {
        unsafe {
            if let Some((ptr, channels, frames)) = self.input_window.get() {
                let n = dst.len().min(frames);
                for i in 0..n {
                    dst[i] = *ptr.add(i * channels + channel);
                }
                if n < dst.len() {
                    dst[n..].fill(0.0);
                }
                n
            } else {
                dst.fill(0.0);
                dst.len()
            }
        }
    }

    fn num_input_channels(&self) -> usize {
        self.config.input_channels as usize
    }
}

// ============================================================================
// IoPlayback impl
// ============================================================================

impl IoPlayback for PipewireBackend {
    fn write_output(&self, channel: usize, src: &[f32]) -> usize {
        unsafe {
            if let Some((ptr, channels, frames)) = self.output_window.get() {
                let n = src.len().min(frames);
                for i in 0..n {
                    *ptr.add(i * channels + channel) = src[i];
                }
                n
            } else {
                0
            }
        }
    }

    fn num_output_channels(&self) -> usize {
        self.config.output_channels as usize
    }
}

impl Drop for PipewireBackend {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        unsafe {
            self.process_cb.drop_box();
        }
    }
}
