//! PipeWire бэкенд для Linux
//!
//! Использует `pipewire` (0.9) с `MainLoopRc` / `ContextRc` / `StreamBox`.
//! Потоковая архитектура: отдельный std::thread с PW main loop,
//! процесс-колбэки работают через `IoRingBuffer`.

use crossbeam_channel::{unbounded, Receiver, Sender};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;

use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::sys as spa_sys;

use crate::audio_io::{AudioIo, IoResult as AudioIoResult};
use crate::backend::{AudioBackend, BackendType};
use crate::buffer::IoRingBuffer;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use crate::midi::MidiEvent;
use crate::PwBuffers;

// ============================================================================
// Команды для PW потока
// ============================================================================

#[derive(Debug, Clone)]
enum PwCommand {
    Start,
    Stop,
}

// ============================================================================
// PipewireBackend
// ============================================================================

pub struct PipewireBackend {
    config: AudioConfig,
    command_tx: Sender<PwCommand>,
    input_buffer: Arc<parking_lot::RwLock<IoRingBuffer>>,
    output_buffer: Arc<parking_lot::RwLock<IoRingBuffer>>,
    process_cb: *mut Option<Box<dyn Fn()>>,
    thread_handle: Option<thread::JoinHandle<()>>,
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
}

impl fmt::Debug for PipewireBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipewireBackend")
            .field("config", &self.config)
            .field("thread_handle", &self.thread_handle.is_some())
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

        let buf_cap = (config.buffer_size * config.output_channels.max(1) * 4) as usize;
        let (command_tx, command_rx) = unbounded();

        let xruns = Arc::new(AtomicU32::new(0));
        let running = Arc::new(AtomicBool::new(false));

        let input_buffer = Arc::new(parking_lot::RwLock::new(IoRingBuffer::new(buf_cap)));
        let output_buffer = Arc::new(parking_lot::RwLock::new(IoRingBuffer::new(buf_cap)));

        // Allocate a slot for the process callback. Leaked — reclaimed on drop.
        let process_cb = Box::into_raw(Box::new(None::<Box<dyn Fn()>>));

        let t_xruns = xruns.clone();
        let t_input = input_buffer.clone();
        let t_output = output_buffer.clone();
        let t_midi_tx = config.midi_event_tx.clone();
        let t_config = config.clone();
        let t_running = running.clone();

        let handle = thread::Builder::new()
            .name("drift-pipewire".into())
            .spawn(move || {
                run_pipewire_thread(command_rx, t_xruns, t_input, t_output, process_cb, t_config, t_running, t_midi_tx);
            })
            .map_err(|e| IoError::Backend(e.to_string()))?;

        Ok(Self {
            config,
            command_tx,
            input_buffer,
            output_buffer,
            process_cb,
            thread_handle: Some(handle),
            xruns,
            running,
        })
    }

    /// Return shared ring buffers for injection into AudioInput/AudioOutput.
    pub fn rings(&self) -> Arc<PwBuffers> {
        Arc::new(PwBuffers {
            input: self.input_buffer.clone(),
            output: self.output_buffer.clone(),
        })
    }
}

// ============================================================================
// AudioIo impl
// ============================================================================

impl AudioIo for PipewireBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn()>) {
        unsafe { *self.process_cb = Some(cb); }
    }

    fn read_input(&self, left: &mut [f32], right: &mut [f32]) -> usize {
        let mut buf = self.input_buffer.write();
        let mut temp = [0.0f32; 512]; // max BUF_SIZE*2
        let n = buf.read(&mut temp[..left.len().min(right.len()).min(256).saturating_mul(2)]);
        drop(buf);
        let frames = n / 2;
        for i in 0..frames.min(left.len()).min(right.len()) {
            left[i] = temp[i * 2];
            right[i] = temp[i * 2 + 1];
        }
        frames
    }

    fn write_output(&self, left: &[f32], right: &[f32]) -> usize {
        let n = left.len().min(right.len());
        let mut buf = self.output_buffer.write();
        let mut temp = [0.0f32; 512];
        let len = n.min(256);
        for i in 0..len {
            temp[i * 2] = left[i];
            temp[i * 2 + 1] = right[i];
        }
        let written = buf.write(&temp[..len * 2]);
        drop(buf);
        written / 2
    }

    fn start(&self) -> AudioIoResult<()> {
        self.running.store(true, Ordering::Release);
        self.command_tx.send(PwCommand::Start)
            .map_err(|e| format!("PW start: {e}"))
    }

    fn stop(&self) -> AudioIoResult<()> {
        self.running.store(false, Ordering::Release);
        self.command_tx.send(PwCommand::Stop)
            .map_err(|e| format!("PW stop: {e}"))
    }
}

// ============================================================================
// PW thread
// ============================================================================

fn run_pipewire_thread(
    command_rx: Receiver<PwCommand>,
    xruns: Arc<AtomicU32>,
    input_buffer: Arc<parking_lot::RwLock<IoRingBuffer>>,
    output_buffer: Arc<parking_lot::RwLock<IoRingBuffer>>,
    process_cb: *mut Option<Box<dyn Fn()>>,
    config: AudioConfig,
    running: Arc<AtomicBool>,
    midi_event_tx: Option<Sender<MidiEvent>>,
) {
    // Ждём команду Start
    while let Ok(cmd) = command_rx.recv() {
        if matches!(cmd, PwCommand::Start) {
            break;
        }
    }

    pw::init();

    let mainloop = match pw::main_loop::MainLoopRc::new(None) {
        Ok(ml) => ml,
        Err(e) => {
            log::error!("PipeWire MainLoopRc::new: {e}");
            return;
        }
    };
    let context = match pw::context::ContextRc::new(&mainloop, None) {
        Ok(c) => c,
        Err(e) => {
            log::error!("PipeWire ContextRc::new: {e}");
            return;
        }
    };
    let core = match context.connect_rc(None) {
        Ok(c) => c,
        Err(e) => {
            log::error!("PipeWire core.connect_rc: {e}");
            return;
        }
    };

    let sample_rate = config.sample_rate;
    let out_channels = config.output_channels as usize;
    let in_channels = config.input_channels as usize;

    // ── Output (playback) stream ───────────────────────────────────────
    let out_node = config
        .output_device
        .as_deref()
        .unwrap_or("rill-output");
    let out_desc = format!("Rill Audio Output ({out_node})");

    let mut out_props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::MEDIA_CATEGORY => "Playback",
        *pw::keys::NODE_NAME => out_node,
        *pw::keys::NODE_DESCRIPTION => out_desc.as_str(),
    };
    out_props.insert("audio.channels", out_channels.to_string());

    let out_stream_name = format!("{out_node}-output");
    let out_stream = match pw::stream::StreamBox::new(&core, &out_stream_name, out_props) {
        Ok(s) => s,
        Err(e) => {
            log::error!("PipeWire StreamBox::new (output): {e}");
            return;
        }
    };

    let obuf = output_buffer.clone();
    let oxruns = xruns.clone();
    let _out_listener = match out_stream
        .add_local_listener_with_user_data(())
        .process(move |stream, _| {
            // 1. Call process callback (drives the signal graph)
            unsafe {
                if let Some(ref cb) = *process_cb {
                    cb();
                }
            }
            // 2. Read from output ring → DMA buffer
            process_output(stream, &obuf, &oxruns, out_channels);
        })
        .register()
    {
        Ok(l) => l,
        Err(e) => {
            log::error!("PipeWire output listener: {e}");
            return;
        }
    };

    // ── Format params for output ───────────────────────────────────────
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

    let out_params_bytes: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
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

    let mut out_params = [spa::pod::Pod::from_bytes(&out_params_bytes).unwrap()];

    if let Err(e) = out_stream.connect(
        spa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut out_params,
    ) {
        log::error!("PipeWire output stream connect: {e}");
        return;
    }

    running.store(true, Ordering::Release);

    // ── Input (capture) stream ─────────────────────────────────────────
    let in_node = config
        .input_device
        .as_deref()
        .unwrap_or("rill-input");
    let in_desc = format!("Rill Audio Input ({in_node})");

    let mut in_props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::NODE_NAME => in_node,
        *pw::keys::NODE_DESCRIPTION => in_desc.as_str(),
    };
    in_props.insert("audio.channels", in_channels.to_string());

    let in_stream_name = format!("{in_node}-input");
    let in_stream = match pw::stream::StreamBox::new(&core, &in_stream_name, in_props) {
        Ok(s) => Some(s),
        Err(e) => {
            log::warn!("PipeWire StreamBox::new (input): {e} — capture disabled");
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
                process_input(stream, &ibuf, &ixruns, in_channels);
            })
            .register()
        {
            Ok(l) => l,
            Err(e) => {
                log::warn!("PipeWire input listener: {e} — capture disabled");
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
            log::warn!("PipeWire input connect: {e} — capture disabled");
        }
    }

    // ── MIDI input stream (optional) ──────────────────────────────────
    let midi_stream = if config.midi_input {
        let m_node = config
            .input_device
            .as_deref()
            .unwrap_or("rill-midi-input");
        let m_desc = format!("Rill MIDI Input ({m_node})");

        let mut m_props = properties! {
            *pw::keys::MEDIA_TYPE => "Midi",
            *pw::keys::MEDIA_ROLE => "Music",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::NODE_NAME => m_node,
            *pw::keys::NODE_DESCRIPTION => m_desc.as_str(),
        };
        m_props.insert("midi.channels", "1");

        let m_name = format!("{m_node}-midi");
        match pw::stream::StreamBox::new(&core, &m_name, m_props) {
            Ok(s) => {
                let mt = midi_event_tx.clone();
                let _m_listener = match s
                    .add_local_listener_with_user_data(())
                    .process(move |stream, _| {
                        process_midi_input(stream, &mt);
                    })
                    .register()
                {
                    Ok(l) => l,
                    Err(e) => {
                        log::warn!("PipeWire MIDI listener: {e} — MIDI disabled");
                        return;
                    }
                };

                // MIDI streams use different format params
                let m_params: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
                    std::io::Cursor::new(Vec::new()),
                    &spa::pod::Value::Object(spa::pod::Object {
                        type_: spa_sys::SPA_TYPE_OBJECT_Format,
                        id: spa_sys::SPA_PARAM_EnumFormat,
                        properties: spa::param::audio::AudioInfoRaw::new().into(),
                    }),
                )
                .unwrap()
                .0
                .into_inner();

                let mut midi_params = [spa::pod::Pod::from_bytes(&m_params).unwrap()];

                if let Err(e) = s.connect(
                    spa::utils::Direction::Input,
                    None,
                    pw::stream::StreamFlags::AUTOCONNECT
                        | pw::stream::StreamFlags::MAP_BUFFERS
                        | pw::stream::StreamFlags::RT_PROCESS,
                    &mut midi_params,
                ) {
                    log::warn!("PipeWire MIDI connect: {e} — MIDI disabled");
                    return;
                }
                Some(s)
            }
            Err(e) => {
                log::warn!("PipeWire MIDI stream creation: {e} — MIDI disabled");
                None
            }
        }
    } else {
        None
    };

    // ── Main loop: iterate with 1 ms timeout ─────────────────────────
    loop {
        while let Ok(cmd) = command_rx.try_recv() {
            match cmd {
                PwCommand::Stop => {
                    let _ = out_stream.disconnect();
                    if let Some(ref s) = in_stream {
                        let _ = s.disconnect();
                    }
                    if let Some(ref s) = midi_stream {
                        let _ = s.disconnect();
                    }
                    running.store(false, Ordering::Release);
                    mainloop.quit();
                    return;
                }
                PwCommand::Start => {}
            }
        }
        mainloop.loop_().iterate(std::time::Duration::from_millis(1));
    }
}

/// Process MIDI input: parse raw MIDI bytes from the PW buffer and
/// send them through the channel.
fn process_midi_input(
    stream: &pw::stream::Stream,
    event_tx: &Option<Sender<MidiEvent>>,
) {
    let tx = match event_tx {
        Some(t) => t,
        None => return,
    };

    let mut buffer = match stream.dequeue_buffer() {
        Some(b) => b,
        None => return,
    };

    let datas = buffer.datas_mut();
    if datas.is_empty() {
        return;
    }

    let data = &mut datas[0];
    let slice = match data.data() {
        Some(s) => s,
        None => return,
    };

    // Parse MIDI events from the raw byte buffer.
    // Each MIDI message is 1-3 bytes.  We scan forward.
    let mut i = 0;
    while i < slice.len() {
        let status = slice[i];
        let msg_len = match status & 0xF0 {
            0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 3, // 3-byte messages
            0xC0 | 0xD0 => 2,                        // 2-byte messages
            _ => 1,                                   // system messages, etc.
        };

        let end = (i + msg_len).min(slice.len());
        if let Some(ev) = MidiEvent::from_bytes(&slice[i..end]) {
            let _ = tx.try_send(ev);
        }
        i = end;
    }
}

// ============================================================================
// Process callbacks (RT-safe, called from PipeWire audio thread)
// ============================================================================

fn process_output(
    stream: &pw::stream::Stream,
    output_buffer: &parking_lot::RwLock<IoRingBuffer>,
    xruns: &AtomicU32,
    channels: usize,
) {
    let mut buffer = match stream.dequeue_buffer() {
        Some(b) => b,
        None => {
            xruns.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };

    let datas = buffer.datas_mut();
    if datas.is_empty() {
        return;
    }

    let data = &mut datas[0];
    let slice = match data.data() {
        Some(s) => s,
        None => return,
    };

    let stride = channels * 4;
    let n_frames = slice.len() / stride;
    let n_samples = n_frames * channels;

    let mut temp = vec![0.0f32; n_samples];
    let mut obuf = output_buffer.write();
    let read = obuf.read(&mut temp);
    drop(obuf);

    for frame in 0..n_frames {
        for ch in 0..channels {
            let idx = frame * channels + ch;
            let offset = idx * 4;
            if offset + 4 <= slice.len() {
                let val = if idx < read { temp[idx] } else { 0.0 };
                slice[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
            }
        }
    }

    let chunk = data.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = stride as i32;
    *chunk.size_mut() = (stride * n_frames) as u32;
}

fn process_input(
    stream: &pw::stream::Stream,
    input_buffer: &parking_lot::RwLock<IoRingBuffer>,
    xruns: &AtomicU32,
    channels: usize,
) {
    let mut buffer = match stream.dequeue_buffer() {
        Some(b) => b,
        None => {
            xruns.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };

    let datas = buffer.datas_mut();
    if datas.is_empty() {
        return;
    }

    let data = &mut datas[0];
    let slice = match data.data() {
        Some(s) => s,
        None => return,
    };

    let stride = channels * 4;
    let n_frames = slice.len() / stride;
    let n_samples = n_frames * channels;

    let mut temp = vec![0.0f32; n_samples];
    for (i, sample) in temp.iter_mut().enumerate() {
        let offset = i * 4;
        if offset + 4 <= slice.len() {
            let mut bytes = [0u8; 4];
            bytes.copy_from_slice(&slice[offset..offset + 4]);
            *sample = f32::from_le_bytes(bytes);
        }
    }

    let mut ibuf = input_buffer.write();
    ibuf.write(&temp);
    drop(ibuf);

    let chunk = data.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = stride as i32;
    *chunk.size_mut() = (stride * n_frames) as u32;
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
        self.command_tx
            .send(PwCommand::Start)
            .map_err(|e| IoError::Backend(e.to_string()))
    }

    fn stop(&mut self) -> IoResult<()> {
        self.running.store(false, Ordering::Release);
        let _ = self.command_tx.send(PwCommand::Stop);
        thread::sleep(std::time::Duration::from_millis(50));
        Ok(())
    }

    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize> {
        let n = self.input_buffer.write().read(buffer);
        Ok(n)
    }

    fn write(&mut self, buffer: &[f32]) -> IoResult<usize> {
        let n = self.output_buffer.write().write(buffer);
        Ok(n)
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
        let _ = self.command_tx.send(PwCommand::Stop);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}
