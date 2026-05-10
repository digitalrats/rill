//! PipeWire backend for Linux
//!
//! Uses `pipewire` (0.9) with `MainLoopRc` / `ContextRc` / `StreamBox`.
//! Output writes directly to the PW DMA buffer via OutputWindow (no ring buffer).
//! Input still uses IoRingBuffer.
//!
//! `run()` — blocking: initializes PW, creates context/core/streams,
//! enters mainloop iterate loop. Exits when `running` becomes false.
//! No `std::thread`, `std::sync`.

use rill_core::math::functions::{deinterleave_stereo, interleave_stereo};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::sys as spa_sys;

use crate::buffer::IoRingBuffer;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use crate::output_window::{OutputSlot, OutputWindow};
use crate::PwBuffers;
use rill_core::io::IoBackend;

/// Maximum stereo block in samples (4096 frames × 2 channels).
const MAX_BLOCK_SAMPLES: usize = 8192;

/// Callback slot.
#[derive(Copy, Clone)]
struct CbSlot(usize);

impl CbSlot {
    fn new() -> Self {
        Self(Box::into_raw(Box::new(None::<Box<dyn Fn(f32)>>)) as usize)
    }
    unsafe fn set(&self, cb: Box<dyn Fn(f32)>) {
        (*(self.0 as *mut Option<Box<dyn Fn(f32)>>)) = Some(cb);
    }
    unsafe fn call(&self, sr: f32) {
        if let Some(ref cb) = *(self.0 as *mut Option<Box<dyn Fn(f32)>>) {
            cb(sr);
        }
    }
    unsafe fn drop_box(&self) {
        drop(Box::from_raw(self.0 as *mut Option<Box<dyn Fn(f32)>>));
    }
}

// PipewireBackend
// ============================================================================

/// Direct capture data — set by capture callback, read by generate().
/// Valid only during process_cb.call() on the PW RT thread.
/// PipeWire audio backend — processes audio via PW stream callbacks,
/// output goes directly to PW DMA buffer through `OutputWindow`.
pub struct PipewireBackend {
    config: AudioConfig,
    input_buffer: Arc<IoRingBuffer>,
    process_cb: CbSlot,
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    output_slot: OutputSlot,
    negotiated_input_channels: Arc<AtomicU32>,
    negotiated_input_rate: Arc<AtomicU32>,
}

impl fmt::Debug for PipewireBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipewireBackend")
            .field("config", &self.config)
            .finish()
    }
}

impl PipewireBackend {
    /// Create a new PipeWire backend.
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        if !cfg!(target_os = "linux") {
            return Err(IoError::Unsupported(
                "PipeWire is only available on Linux".into(),
            ));
        }

        Ok(Self {
            input_buffer: Arc::new(IoRingBuffer::new(
                (config.buffer_size * config.input_channels.max(1) * 32) as usize,
            )),
            config,
            process_cb: CbSlot::new(),
            xruns: Arc::new(AtomicU32::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            output_slot: OutputSlot::new(),
            negotiated_input_channels: Arc::new(AtomicU32::new(0)),
            negotiated_input_rate: Arc::new(AtomicU32::new(0)),
        })
    }

    /// Return the negotiated sample rate from PipeWire, or 0 if not yet negotiated.
    pub fn negotiated_rate(&self) -> u32 {
        self.negotiated_input_rate.load(Ordering::Relaxed)
    }

    /// Return the negotiated channel count from PipeWire, or 0 if not yet negotiated.
    pub fn negotiated_channels(&self) -> u32 {
        self.negotiated_input_channels.load(Ordering::Relaxed)
    }

    /// Return shared ring buffers for injection into AudioInput/AudioOutput.
    pub fn rings(&self) -> Arc<PwBuffers> {
        Arc::new(PwBuffers {
            input: self.input_buffer.clone(),
            output: Arc::new(IoRingBuffer::new(0)),
        })
    }
}

// ============================================================================
// IoBackend impl
// ============================================================================

impl IoBackend<f32> for PipewireBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn(f32)>) {
        unsafe {
            self.process_cb.set(cb);
        }
    }

    fn read(&self, channels: &mut [&mut [f32]]) -> usize {
        let frames = channels.first().map(|c| c.len()).unwrap_or(0);
        if frames == 0 {
            return 0;
        }
        let out_ch = {
            let c = self.negotiated_input_channels.load(Ordering::Relaxed);
            if c > 0 {
                c as usize
            } else {
                self.config.input_channels.max(1) as usize
            }
        };
        let mut temp = [0.0f32; MAX_BLOCK_SAMPLES];
        let max_s = frames.saturating_mul(out_ch).min(MAX_BLOCK_SAMPLES);
        let n_read = self.input_buffer.read(&mut temp[..max_s]);
        let frames_out = n_read / out_ch;
        let out = frames_out.min(frames);
        if out_ch >= 2 {
            let (ch0, ch1) =
                if let (Some(c0), Some(c1)) = (channels.get_mut(0), channels.get_mut(1)) {
                    (&mut c0[..out], &mut c1[..out])
                } else {
                    return out;
                };
            deinterleave_stereo(&temp[..out * 2], ch0, ch1);
        } else {
            for i in 0..out {
                if let Some(c) = channels.get_mut(0) {
                    c[i] = temp[i];
                }
                if let Some(c) = channels.get_mut(1) {
                    c[i] = temp[i];
                }
            }
        }
        out
    }

    fn write(&self, channels: &[&[f32]]) -> usize {
        let nch = channels.len();
        if nch == 0 {
            return 0;
        }
        let frames = channels[0].len();
        if let Some(win) = unsafe { self.output_slot.as_mut() } {
            let cap = win.capacity().min(frames * nch);
            let dst = win.as_mut_slice();
            if nch >= 2 {
                interleave_stereo(channels[0], channels[1], &mut dst[..frames * 2]);
            } else {
                for i in 0..frames {
                    dst[i] = channels[0][i];
                }
            }
            cap / nch
        } else {
            0
        }
    }

    fn run(&self, running: Arc<AtomicBool>) -> Result<(), String> {
        let process_cb = self.process_cb;
        let oslot = self.output_slot.clone();
        let ibuf = self.input_buffer.clone();
        let xruns = self.xruns.clone();
        let sample_rate = self.config.sample_rate;
        let out_channels = self.config.output_channels;
        let in_channels = self.config.input_channels;
        let out_device = self.config.output_device.clone();
        let in_device = self.config.input_device.clone();

        pw::init();

        let mainloop =
            pw::main_loop::MainLoopRc::new(None).map_err(|e| format!("PW MainLoopRc::new: {e}"))?;
        let context = pw::context::ContextRc::new(&mainloop, None)
            .map_err(|e| format!("PW ContextRc::new: {e}"))?;
        let core = context
            .connect_rc(None)
            .map_err(|e| format!("PW core.connect_rc: {e}"))?;

        // Output stream and listener — alive for the duration of run()
        let _out_stream;
        let _out_listener;
        let ml = mainloop.clone();
        let ml2 = ml.clone();
        let running2 = running.clone();

        if out_channels > 0 {
            let out_chan = out_channels;
            let buf_frames = self.config.buffer_size as usize;
            let chunk_frames = buf_frames;
            let chunk_bytes = chunk_frames * out_chan as usize * 4;
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

            let out_running = running.clone();
            let out_ml = ml.clone();
            let out_sr = sample_rate;
            let listener = stream
                .add_local_listener_with_user_data(())
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
                    let slice = match data.data() {
                        Some(s) => s,
                        None => return,
                    };
                    let stride = out_chan as usize * 4;
                    let n_frames = slice.len() / stride;
                    let mut offset = 0usize;
                    while offset + chunk_bytes <= slice.len() {
                        let chunk = &mut slice[offset..offset + chunk_bytes];
                        unsafe {
                            oslot.set(OutputWindow::new(
                                chunk.as_mut_ptr() as *mut f32,
                                chunk_frames * out_chan as usize,
                            ));
                            process_cb.call(out_sr as f32);
                            oslot.clear();
                        }
                        offset += chunk_bytes;
                    }
                    if offset < slice.len() {
                        slice[offset..].fill(0);
                    }
                    let ck = data.chunk_mut();
                    *ck.offset_mut() = 0;
                    *ck.stride_mut() = stride as i32;
                    *ck.size_mut() = (stride * n_frames) as u32;
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
        } else {
            _out_listener = None;
            _out_stream = None;
        }

        self.running.store(true, Ordering::Release);

        // ── Input stream ────────────────────────────────────────────────────
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
            format!("{}/{}", self.config.buffer_size, sample_rate),
        );

        let in_stream =
            match pw::stream::StreamBox::new(&core, &format!("{in_node}-input"), in_props) {
                Ok(s) => Some(s),
                Err(e) => {
                    log::warn!("PW StreamBox input: {e} — capture disabled");
                    None
                }
            };

        // Input listener — MUST live until end of run() or PW stops calling it.
        let _in_listener;

        if let Some(ref in_st) = in_stream {
            let mut in_ai = spa::param::audio::AudioInfoRaw::new();
            in_ai.set_format(spa::param::audio::AudioFormat::F32LE);
            // Don't set rate/channels — let PW negotiate.

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

            let buf_frames = self.config.buffer_size as usize;
            let nch_fmt = self.negotiated_input_channels.clone();
            let nrate_fmt = self.negotiated_input_rate.clone();
            let nch_proc = self.negotiated_input_channels.clone();
            let nrate_proc = self.negotiated_input_rate.clone();

            let listener = in_st
                .add_local_listener_with_user_data(())
                .param_changed(move |_stream, _data, id, param| {
                    if id == spa_sys::SPA_PARAM_Format {
                        if let Some(param) = param {
                            let mut ai = spa::param::audio::AudioInfoRaw::new();
                            if ai.parse(param).is_ok() {
                                nch_fmt.store(ai.channels(), std::sync::atomic::Ordering::Relaxed);
                                nrate_fmt.store(ai.rate(), std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    }
                })
                .process(move |stream, _| {
                    if !running2.load(Ordering::Acquire) {
                        ml2.quit();
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
                        let c = nch_proc.load(std::sync::atomic::Ordering::Relaxed);
                        if c > 0 {
                            c as usize
                        } else {
                            in_channels as usize
                        }
                    };

                    let (_chunk_offset, chunk_size) = {
                        let ck = data.chunk_mut();
                        (*ck.offset_mut(), *ck.size_mut())
                    };

                    if chunk_size == 0 {
                        return;
                    }

                    let slice = match data.data() {
                        Some(s) => s,
                        None => return,
                    };
                    let stride = actual_channels * 4;
                    let n_samp = (chunk_size as usize / stride) * actual_channels;
                    let len = n_samp.min(MAX_BLOCK_SAMPLES);
                    let mut temp = [0.0f32; MAX_BLOCK_SAMPLES];
                    let mut i = 0usize;
                    while i + 4 <= len {
                        let off = i * 4;
                        if off + 16 <= slice.len() {
                            let b0 = f32::from_le_bytes(slice[off..off + 4].try_into().unwrap());
                            let b1 =
                                f32::from_le_bytes(slice[off + 4..off + 8].try_into().unwrap());
                            let b2 =
                                f32::from_le_bytes(slice[off + 8..off + 12].try_into().unwrap());
                            let b3 =
                                f32::from_le_bytes(slice[off + 12..off + 16].try_into().unwrap());
                            temp[i] = b0;
                            temp[i + 1] = b1;
                            temp[i + 2] = b2;
                            temp[i + 3] = b3;
                        }
                        i += 4;
                    }
                    for j in i..len {
                        let off = j * 4;
                        if off + 4 <= slice.len() {
                            let mut bytes = [0u8; 4];
                            bytes.copy_from_slice(&slice[off..off + 4]);
                            temp[j] = f32::from_le_bytes(bytes);
                        }
                    }
                    let block_samps = buf_frames * actual_channels;
                    ibuf.write(&temp[..len]);
                    if out_channels == 0 {
                        while ibuf.len() >= block_samps {
                            unsafe {
                                let sr =
                                    nrate_proc.load(std::sync::atomic::Ordering::Relaxed) as f32;
                                process_cb.call(if sr > 0.0 { sr } else { sample_rate as f32 });
                            }
                        }
                    }
                })
                .register()
                .map_err(|e| format!("PW input listener: {e}"))?;
            _in_listener = Some(listener);

            if let Err(e) = in_st.connect(
                spa::utils::Direction::Input,
                None,
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::DRIVER,
                &mut in_params,
            ) {
                log::warn!("PW input connect: disabled — {e}");
            }
        } else {
            _in_listener = None;
        }

        // ── PW event loop ───────────────────────────────────────────────────
        // No own loop — run() blocks the thread, events are
        // handled by PW. Callback checks running and calls quit.
        mainloop.run();

        if let Some(ref s) = in_stream {
            let _ = s.disconnect();
        }

        Ok(())
    }

    fn stop(&self) -> Result<(), String> {
        self.running.store(false, Ordering::Release);
        Ok(())
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
