//! PipeWire бэкенд для Linux
//!
//! Использует `pipewire` (0.9) с `MainLoopRc` / `ContextRc` / `StreamBox`.
//! Output пишет напрямую в DMA-буфер PW через OutputWindow (без ring buffer).
//! Input по-прежнему использует IoRingBuffer.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::sys as spa_sys;

#[allow(unused_imports)]
use crate::audio_io::{AudioIo, IoResult as AudioIoResult};
use crate::backend::{AudioBackend, BackendType};
use crate::buffer::IoRingBuffer;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use crate::midi::MidiEvent;
use crate::PwBuffers;

/// Maximum stereo block in samples (4096 frames × 2 channels).
const MAX_BLOCK_SAMPLES: usize = 8192;

/// Callback slot.
#[derive(Copy, Clone)]
struct CbSlot(usize);
unsafe impl Send for CbSlot {}
unsafe impl Sync for CbSlot {}

impl CbSlot {
    fn new() -> Self {
        Self(Box::into_raw(Box::new(None::<Box<dyn Fn()>>)) as usize)
    }
    unsafe fn set(&self, cb: Box<dyn Fn()>) {
        (*(self.0 as *mut Option<Box<dyn Fn()>>)) = Some(cb);
    }
    unsafe fn call(&self) {
        if let Some(ref cb) = *(self.0 as *mut Option<Box<dyn Fn()>>) {
            cb();
        }
    }
    unsafe fn drop_box(&self) {
        drop(Box::from_raw(self.0 as *mut Option<Box<dyn Fn()>>));
    }
}

/// Mutable view into a PW DMA buffer slice.
struct OutputWindow {
    ptr: *mut f32,
    capacity: usize,
}

impl OutputWindow {
    fn new(slice: &mut [u8], max_frames: usize) -> Self {
        let cap = (slice.len() / 4).min(max_frames * 2);
        Self {
            ptr: slice.as_mut_ptr() as *mut f32,
            capacity: cap,
        }
    }
    fn as_mut_slice(&mut self) -> &mut [f32] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.capacity) }
    }
}

/// Lock-free slot for the current output window, set during PW process callback.
#[derive(Copy, Clone)]
struct OutputSlot(*mut Option<OutputWindow>);
unsafe impl Send for OutputSlot {}
unsafe impl Sync for OutputSlot {}

impl OutputSlot {
    fn new() -> Self {
        Self(Box::into_raw(Box::new(None)))
    }
    unsafe fn set(&self, w: OutputWindow) {
        *self.0 = Some(w);
    }
    unsafe fn clear(&self) {
        *self.0 = None;
    }
    unsafe fn as_mut(&self) -> Option<&mut OutputWindow> {
        (*self.0).as_mut()
    }
    unsafe fn drop_box(&self) {
        drop(Box::from_raw(self.0));
    }
}

// ============================================================================
// PipewireBackend
// ============================================================================

pub struct PipewireBackend {
    config: AudioConfig,
    input_buffer: Arc<IoRingBuffer>,
    process_cb: CbSlot,
    thread_handle: Mutex<Option<thread::JoinHandle<()>>>,
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    /// Pointed at the current PW DMA chunk during process callback.
    /// Only accessed from the PW RT thread — no locking needed.
    output_slot: OutputSlot,
}

impl fmt::Debug for PipewireBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipewireBackend")
            .field("config", &self.config)
            .field(
                "thread_handle",
                &self
                    .thread_handle
                    .lock()
                    .map(|g| g.is_some())
                    .unwrap_or(false),
            )
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

        let xruns = Arc::new(AtomicU32::new(0));
        let running = Arc::new(AtomicBool::new(false));
        let input_buffer = Arc::new(IoRingBuffer::new(
            (config.buffer_size * config.input_channels.max(1) * 4) as usize,
        ));

        let process_cb = CbSlot::new();
        let output_slot = OutputSlot::new();

        let t_xruns = xruns.clone();
        let t_input = input_buffer.clone();
        let t_midi_tx = config.midi_event_tx.clone();
        let t_config = config.clone();
        let t_running = running.clone();
        let t_process_cb = process_cb;
        let t_slot = output_slot;

        let handle = thread::Builder::new()
            .name("drift-pipewire".into())
            .spawn(move || {
                run_pipewire_thread(
                    t_xruns,
                    t_input,
                    t_process_cb,
                    t_config,
                    t_running,
                    t_midi_tx,
                    t_slot,
                );
            })
            .map_err(|e| IoError::Backend(e.to_string()))?;

        Ok(Self {
            config,
            input_buffer,
            process_cb,
            thread_handle: Mutex::new(Some(handle)),
            xruns,
            running,
            output_slot,
        })
    }

    pub fn rings(&self) -> Arc<PwBuffers> {
        Arc::new(PwBuffers {
            input: self.input_buffer.clone(),
            output: Arc::new(IoRingBuffer::new(0)), // unused — output goes directly to PW DMA
        })
    }
}

// ============================================================================
// AudioIo impl
// ============================================================================

impl AudioIo for PipewireBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn()>) {
        unsafe {
            self.process_cb.set(cb);
        }
    }

    fn read_input(&self, left: &mut [f32], right: &mut [f32]) -> usize {
        let n = left.len().min(right.len());
        let mut temp = [0.0f32; MAX_BLOCK_SAMPLES];
        let max_s = n.saturating_mul(2).min(MAX_BLOCK_SAMPLES);
        let n_read = self.input_buffer.read(&mut temp[..max_s]);
        let frames = n_read / 2;
        let out = frames.min(n);
        for i in 0..out {
            left[i] = temp[i * 2];
            right[i] = temp[i * 2 + 1];
        }
        out
    }

    fn write_output(&self, left: &[f32], right: &[f32]) -> usize {
        let n = left.len().min(right.len());
        if let Some(win) = unsafe { self.output_slot.as_mut() } {
            let cap = win.capacity.min(n * 2);
            let dst = win.as_mut_slice();
            for i in 0..(cap / 2) {
                dst[i * 2] = left[i];
                dst[i * 2 + 1] = right[i];
            }
            cap / 2
        } else {
            0
        }
    }

    fn start(&self) -> AudioIoResult<()> {
        self.running.store(true, Ordering::Release);
        if let Ok(guard) = self.thread_handle.lock() {
            if let Some(ref handle) = *guard {
                handle.thread().unpark();
            }
        }
        Ok(())
    }

    fn stop(&self) -> AudioIoResult<()> {
        self.running.store(false, Ordering::Release);
        if let Ok(guard) = self.thread_handle.lock() {
            if let Some(ref handle) = *guard {
                handle.thread().unpark();
            }
        }
        Ok(())
    }
}

// ============================================================================
// PW thread
// ============================================================================

fn run_pipewire_thread(
    xruns: Arc<AtomicU32>,
    input_buffer: Arc<IoRingBuffer>,
    process_cb: CbSlot,
    config: AudioConfig,
    running: Arc<AtomicBool>,
    midi_event_tx: Option<std::sync::mpsc::Sender<MidiEvent>>,
    output_slot: OutputSlot,
) {
    while !running.load(Ordering::Acquire) {
        thread::park();
    }

    pw::init();

    let mainloop = match pw::main_loop::MainLoopRc::new(None) {
        Ok(ml) => ml,
        Err(e) => {
            log::error!("PW MainLoopRc::new: {e}");
            return;
        }
    };
    let context = match pw::context::ContextRc::new(&mainloop, None) {
        Ok(c) => c,
        Err(e) => {
            log::error!("PW ContextRc::new: {e}");
            return;
        }
    };
    let core = match context.connect_rc(None) {
        Ok(c) => c,
        Err(e) => {
            log::error!("PW core.connect_rc: {e}");
            return;
        }
    };

    let sample_rate = config.sample_rate;
    let out_channels = config.output_channels as usize;
    let in_channels = config.input_channels as usize;

    // ── Output stream ───────────────────────────────────────────────────
    let out_node = config.output_device.as_deref().unwrap_or("rill-output");
    let out_desc = format!("Rill Audio Output ({out_node})");
    let mut out_props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::MEDIA_CATEGORY => "Playback",
        *pw::keys::NODE_NAME => out_node,
        *pw::keys::NODE_DESCRIPTION => out_desc.as_str(),
    };
    out_props.insert("audio.channels", out_channels.to_string());

    let out_stream =
        match pw::stream::StreamBox::new(&core, &format!("{out_node}-output"), out_props) {
            Ok(s) => s,
            Err(e) => {
                log::error!("PW StreamBox output: {e}");
                return;
            }
        };

    let oslot = output_slot;
    let _out_listener = match out_stream
        .add_local_listener_with_user_data(())
        .process(move |stream, _| {
            let mut buf = match stream.dequeue_buffer() {
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

            let stride = out_channels * 4;
            let n_frames = slice.len() / stride;

            // Split PW DMA buffer into BUF_SIZE chunks and process each.
            // Each process_cb.call() produces one block (BUF_SIZE frames).
            // The OutputWindow lets write_output() write directly into DMA memory.
            let chunk_bytes = 512 * 4; // 512 interleaved samples × 4 bytes = 2048 bytes
            let mut offset = 0usize;
            while offset + chunk_bytes <= slice.len() {
                let chunk = &mut slice[offset..offset + chunk_bytes];
                unsafe {
                    oslot.set(OutputWindow::new(chunk, 256)); // BUF=256
                    process_cb.call();
                    oslot.clear();
                }
                offset += chunk_bytes;
            }

            // Zero any remaining DMA buffer
            if offset < slice.len() {
                slice[offset..].fill(0);
            }

            let ck = data.chunk_mut();
            *ck.offset_mut() = 0;
            *ck.stride_mut() = stride as i32;
            *ck.size_mut() = (stride * n_frames) as u32;
        })
        .register()
    {
        Ok(l) => l,
        Err(e) => {
            log::error!("PW output listener: {e}");
            return;
        }
    };

    // ── Format params ──────────────────────────────────────────────────
    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    audio_info.set_rate(sample_rate);
    audio_info.set_channels(out_channels as u32);
    let mut position = [0; spa::param::audio::MAX_CHANNELS];
    if out_channels >= 1 {
        position[0] = spa_sys::SPA_AUDIO_CHANNEL_FL;
    }
    if out_channels >= 2 {
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

    if let Err(e) = out_stream.connect(
        spa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut out_params,
    ) {
        log::error!("PW output connect: {e}");
        return;
    }

    running.store(true, Ordering::Release);

    // ── Input stream ────────────────────────────────────────────────────
    let in_node = config.input_device.as_deref().unwrap_or("rill-input");
    let in_desc = format!("Rill Audio Input ({in_node})");
    let mut in_props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::NODE_NAME => in_node,
        *pw::keys::NODE_DESCRIPTION => in_desc.as_str(),
    };
    in_props.insert("audio.channels", in_channels.to_string());

    let in_stream = match pw::stream::StreamBox::new(&core, &format!("{in_node}-input"), in_props) {
        Ok(s) => Some(s),
        Err(e) => {
            log::warn!("PW StreamBox input: {e} — capture disabled");
            None
        }
    };

    if let Some(ref in_st) = in_stream {
        let ibuf = input_buffer.clone();
        let ixruns = xruns.clone();
        let mut in_ai = spa::param::audio::AudioInfoRaw::new();
        in_ai.set_format(spa::param::audio::AudioFormat::F32LE);
        in_ai.set_rate(sample_rate);
        in_ai.set_channels(in_channels as u32);
        let mut in_pos = [0; spa::param::audio::MAX_CHANNELS];
        if in_channels >= 1 {
            in_pos[0] = spa_sys::SPA_AUDIO_CHANNEL_FL;
        }
        if in_channels >= 2 {
            in_pos[1] = spa_sys::SPA_AUDIO_CHANNEL_FR;
        }
        in_ai.set_position(in_pos);

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

        let _in_listener = match in_st
            .add_local_listener_with_user_data(())
            .process(move |stream, _| {
                let mut buf = match stream.dequeue_buffer() {
                    Some(b) => b,
                    None => {
                        ixruns.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
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
                let stride = in_channels * 4;
                let n_samp = (slice.len() / stride) * in_channels;
                let len = n_samp.min(MAX_BLOCK_SAMPLES);
                let mut temp = [0.0f32; MAX_BLOCK_SAMPLES];
                for i in 0..len {
                    let off = i * 4;
                    if off + 4 <= slice.len() {
                        let mut bytes = [0u8; 4];
                        bytes.copy_from_slice(&slice[off..off + 4]);
                        temp[i] = f32::from_le_bytes(bytes);
                    }
                }
                ibuf.write(&temp[..len]);
            })
            .register()
        {
            Ok(l) => l,
            Err(e) => {
                log::warn!("PW input listener: {e} — capture disabled");
                return;
            }
        };

        if let Err(e) = in_st.connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut in_params,
        ) {
            log::warn!("PW input connect: {e} — capture disabled");
        }
    }

    // ── MIDI ────────────────────────────────────────────────────────────
    #[allow(unused_variables)]
    let midi_stream = if config.midi_input {
        let m_name = format!("{in_node}-midi");
        let m_desc = format!("Rill MIDI Input ({in_node})");
        match pw::stream::StreamBox::new(
            &core,
            &m_name,
            properties! {
                *pw::keys::MEDIA_TYPE => "Midi",
                *pw::keys::MEDIA_ROLE => "Music",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::NODE_NAME => m_name.as_str(),
                *pw::keys::NODE_DESCRIPTION => m_desc.as_str(),
            },
        ) {
            Ok(s) => {
                let mt = midi_event_tx.clone();
                let _ml = s
                    .add_local_listener_with_user_data(())
                    .process(move |st, _| {
                        let mut b = match st.dequeue_buffer() {
                            Some(b) => b,
                            None => return,
                        };
                        let ds = b.datas_mut();
                        if ds.is_empty() {
                            return;
                        }
                        let d = &mut ds[0];
                        let sl = match d.data() {
                            Some(s) => s,
                            None => return,
                        };
                        let mut i = 0;
                        while i < sl.len() {
                            let st = sl[i];
                            let ml = match st & 0xF0 {
                                0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 3,
                                0xC0 | 0xD0 => 2,
                                _ => 1,
                            };
                            let end = (i + ml).min(sl.len());
                            if let Some(ev) = MidiEvent::from_bytes(&sl[i..end]) {
                                let _ = mt.as_ref().map(|t| t.send(ev));
                            }
                            i = end;
                        }
                    })
                    .register();
                if let Err(e) = s.connect(
                    spa::utils::Direction::Input,
                    None,
                    pw::stream::StreamFlags::AUTOCONNECT
                        | pw::stream::StreamFlags::MAP_BUFFERS
                        | pw::stream::StreamFlags::RT_PROCESS,
                    &mut [],
                ) {
                    log::warn!("PW MIDI connect: {e}");
                }
                Some(s)
            }
            Err(e) => {
                log::warn!("PW MIDI stream: {e}");
                None
            }
        }
    } else {
        None
    };

    // ── Main loop ──────────────────────────────────────────────────────
    loop {
        mainloop
            .loop_()
            .iterate(std::time::Duration::from_millis(1));
        if !running.load(Ordering::Acquire) {
            mainloop.quit();
            break;
        }
    }
    let _ = out_stream.disconnect();
    if let Some(ref s) = in_stream {
        let _ = s.disconnect();
    }
}

// ============================================================================
// AudioBackend
// ============================================================================

impl AudioBackend for PipewireBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::PipeWire
    }
    fn config(&self) -> &AudioConfig {
        &self.config
    }
    fn config_mut(&mut self) -> &mut AudioConfig {
        &mut self.config
    }
    fn init(&mut self) -> IoResult<()> {
        Ok(())
    }

    fn start(&mut self) -> IoResult<()> {
        self.running.store(true, Ordering::Release);
        if let Ok(guard) = self.thread_handle.lock() {
            if let Some(ref handle) = *guard {
                handle.thread().unpark();
            }
        }
        Ok(())
    }

    fn stop(&mut self) -> IoResult<()> {
        self.running.store(false, Ordering::Release);
        if let Ok(guard) = self.thread_handle.lock() {
            if let Some(ref handle) = *guard {
                handle.thread().unpark();
            }
        }
        thread::sleep(std::time::Duration::from_millis(50));
        Ok(())
    }

    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize> {
        let n = self.input_buffer.read(buffer);
        Ok(n)
    }

    fn write(&mut self, _buffer: &[f32]) -> IoResult<usize> {
        Ok(0)
    }

    fn xruns(&self) -> u32 {
        self.xruns.load(Ordering::Acquire)
    }
    fn latency(&self) -> std::time::Duration {
        std::time::Duration::from_micros(
            (1_000_000.0 * self.config.buffer_size as f64 / self.config.sample_rate as f64) as u64,
        )
    }
    fn list_input_devices(&self) -> Vec<String> {
        vec!["default".to_string()]
    }
    fn list_output_devices(&self) -> Vec<String> {
        vec!["default".to_string()]
    }
}

impl Drop for PipewireBackend {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Ok(guard) = self.thread_handle.lock() {
            if let Some(ref handle) = *guard {
                handle.thread().unpark();
            }
        }
        let handle = self.thread_handle.lock().ok().and_then(|mut g| g.take());
        if let Some(h) = handle {
            let _ = h.join();
        }
        unsafe {
            self.process_cb.drop_box();
        }
        unsafe {
            self.output_slot.drop_box();
        }
    }
}
