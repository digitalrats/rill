//! PipeWire backend for Linux
//!
//! Uses `pipewire` (0.9) with `MainLoopRc` / `ContextRc` / `StreamBox`.
//! Zero-copy via `DirectView` — graph nodes read/write directly from/to
//! the PipeWire DMA buffers through `tick.view`.
//!
//! `run()` — blocking: initializes PW, creates context/core/streams,
//! enters mainloop iterate loop. Exits when `running` becomes false.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::sys as spa_sys;

use crate::buffer_view::DirectView;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use rill_core::io::IoBackend;
use rill_core::time::ClockTick;
use rill_core::traits::buffer_view::{BufferView, NullBufferView};

/// Callback slot — stores the process callback via raw pointer.
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

/// I/O mode — determines which PW stream drives the tick.
#[derive(Debug, Clone, Copy, PartialEq)]
enum IoMode {
    InputDriver,
    OutputDriver,
}

// ============================================================================
// PipewireBackend
// ============================================================================

/// PipeWire audio backend with zero-copy DMA access via `DirectView`.
pub struct PipewireBackend {
    config: AudioConfig,
    process_cb: CbSlot,
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    sample_pos: Arc<AtomicU64>,
    negotiated_input_channels: Arc<AtomicU32>,
    negotiated_input_rate: Arc<AtomicU32>,
    mode: IoMode,
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
            return Err(IoError::Unsupported(
                "PipeWire is only available on Linux".into(),
            ));
        }
        let input_channels = config.input_channels;
        let sample_rate = config.sample_rate;
        Ok(Self {
            config,
            process_cb: CbSlot::new(),
            xruns: Arc::new(AtomicU32::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            sample_pos: Arc::new(AtomicU64::new(0)),
            negotiated_input_channels: Arc::new(AtomicU32::new(input_channels)),
            negotiated_input_rate: Arc::new(AtomicU32::new(sample_rate)),
            mode: if input_channels > 0 {
                IoMode::InputDriver
            } else {
                IoMode::OutputDriver
            },
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
// IoBackend impl
// ============================================================================

impl IoBackend for PipewireBackend {
    fn create_view(&self) -> Arc<dyn BufferView> {
        // Real views are created per-callback with DMA pointers.
        Arc::new(NullBufferView::new(
            self.config.input_channels as usize,
            self.config.output_channels as usize,
        ))
    }

    fn set_process_callback(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        unsafe {
            self.process_cb.set(cb);
        }
    }

    fn run(&self, running: Arc<AtomicBool>) -> Result<(), String> {
        let process_cb = self.process_cb;
        let xruns = self.xruns.clone();
        let sample_rate = self.config.sample_rate;
        let block_size = self.config.buffer_size;
        let out_channels = self.config.output_channels;
        let in_channels = self.config.input_channels;
        let out_device = self.config.output_device.clone();
        let in_device = self.config.input_device.clone();
        let sample_pos = self.sample_pos.clone();

        pw::init();

        let mainloop =
            pw::main_loop::MainLoopRc::new(None).map_err(|e| format!("PW MainLoopRc::new: {e}"))?;
        let context = pw::context::ContextRc::new(&mainloop, None)
            .map_err(|e| format!("PW ContextRc::new: {e}"))?;
        let core = context
            .connect_rc(None)
            .map_err(|e| format!("PW core.connect_rc: {e}"))?;

        // ── Output stream ────────────────────────────────────────────────────
        let _out_stream;
        let _out_listener;
        let ml = mainloop.clone();
        let running2 = running.clone();

        if out_channels > 0 {
            let out_chan = out_channels;
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
            out_props.insert(
                *pw::keys::NODE_LATENCY,
                format!("{}/{}", block_size, sample_rate),
            );

            let stream =
                pw::stream::StreamBox::new(&core, &format!("{out_node}-output"), out_props)
                    .map_err(|e| format!("PW StreamBox output: {e}"))?;

            let out_running = running.clone();
            let out_ml = ml.clone();
            let out_sr = sample_rate;
            let out_spos = sample_pos.clone();
            let out_cb = process_cb;
            let is_input_driver = self.mode == IoMode::InputDriver;

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

                    let total_samps = slice.len() / 4;
                    let n_frames = total_samps / out_chan as usize;
                    let stride = out_chan as usize * 4;

                    if !is_input_driver {
                        // OutputDriver: create DirectView, fire process_cb in chunks
                        let mut offset = 0usize;
                        while offset < n_frames {
                            let chunk = (n_frames - offset).min(block_size as usize);
                            let view: Arc<dyn BufferView> = Arc::new(DirectView::new_output_only(
                                unsafe {
                                    (slice.as_mut_ptr() as *mut f32).add(offset * out_chan as usize)
                                },
                                out_chan as usize,
                                chunk,
                            ));
                            let pos = out_spos.fetch_add(chunk as u64, Ordering::Relaxed);
                            let tick = ClockTick::new(
                                pos,
                                chunk as u32,
                                out_sr as f32,
                                "pipewire".into(),
                                view,
                            );
                            unsafe {
                                out_cb.call(&tick);
                            }
                            offset += chunk;
                        }
                    } else {
                        // InputDriver: copy output_ring → DMA (passive output)
                        let samples: &mut [f32] = unsafe {
                            std::slice::from_raw_parts_mut(
                                slice.as_mut_ptr() as *mut f32,
                                total_samps,
                            )
                        };
                        // In InputDriver mode, the input callback drives the tick
                        // and fills a ring buffer. For now, silence the output.
                        samples.fill(0.0);
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
        } else {
            _out_listener = None;
            _out_stream = None;
        }

        self.running.store(true, Ordering::Release);

        // ── Input stream ─────────────────────────────────────────────────────
        let in_stream: Option<pw::stream::StreamBox>;
        let _in_listener: Option<_>;

        if in_channels > 0 {
            let in_node = in_device.as_deref().unwrap_or("rill-input");
            let in_desc = format!("Rill Audio Input ({in_node})");
            let mut in_props = properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_ROLE => "Music",
                *pw::keys::MEDIA_CATEGORY => "Stream",
                *pw::keys::NODE_NAME => in_node,
                *pw::keys::NODE_DESCRIPTION => in_desc.as_str(),
            };
            in_props.insert("audio.channels", in_channels.to_string());
            in_props.insert(
                *pw::keys::NODE_LATENCY,
                format!("{}/{}", block_size, sample_rate),
            );

            in_stream =
                match pw::stream::StreamBox::new(&core, &format!("{in_node}-input"), in_props) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        log::warn!("PW StreamBox input: {e} — capture disabled");
                        None
                    }
                };

            if let Some(ref in_st) = in_stream {
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

                let nch_fmt = self.negotiated_input_channels.clone();
                let nrate_fmt = self.negotiated_input_rate.clone();
                let nch_proc = self.negotiated_input_channels.clone();
                let nrate_proc = self.negotiated_input_rate.clone();
                let is_driver = self.mode == IoMode::InputDriver;

                let listener = in_st
                    .add_local_listener_with_user_data(())
                    .param_changed(move |_stream, _data, id, param| {
                        if id == spa_sys::SPA_PARAM_Format {
                            if let Some(param) = param {
                                let mut ai = spa::param::audio::AudioInfoRaw::new();
                                if ai.parse(param).is_ok() {
                                    nch_fmt.store(ai.channels(), Ordering::Relaxed);
                                    nrate_fmt.store(ai.rate(), Ordering::Relaxed);
                                }
                            }
                        }
                    })
                    .process(move |stream, _| {
                        if !running2.load(Ordering::Acquire) {
                            ml.quit();
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
                            let c = nch_proc.load(Ordering::Relaxed);
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

                        let view: Arc<dyn BufferView> = Arc::new(DirectView::new_interleaved(
                            sample_bytes.as_ptr() as *const f32,
                            std::ptr::null_mut(),
                            actual_channels,
                            0,
                            total_samps / actual_channels.max(1),
                        ));

                        if out_channels == 0 || is_driver {
                            let sr = nrate_proc.load(Ordering::Relaxed) as f32;
                            let effective_sr = if sr > 0.0 { sr } else { sample_rate as f32 };
                            let pos = sample_pos.fetch_add(
                                (total_samps / actual_channels.max(1)) as u64,
                                Ordering::Relaxed,
                            );
                            let tick = ClockTick::new(
                                pos,
                                (total_samps / actual_channels.max(1)) as u32,
                                effective_sr,
                                "pipewire".into(),
                                view,
                            );
                            unsafe {
                                process_cb.call(&tick);
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
                        | pw::stream::StreamFlags::RT_PROCESS,
                    &mut in_params,
                ) {
                    log::warn!("PW input connect: disabled — {e}");
                }
            } else {
                _in_listener = None;
            }
        } else {
            in_stream = None;
            _in_listener = None;
        }

        // ── PW event loop ───────────────────────────────────────────────────
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
