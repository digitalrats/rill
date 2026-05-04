//! JACK бэкенд

use crossbeam_channel::{unbounded, Receiver, Sender};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

use jack::{AudioIn, AudioOut, Client, ClientOptions, Control, Port, ProcessHandler, ProcessScope};

use crate::audio_io::{AudioIo, IoResult as AudioIoResult};
use crate::backend::{AudioBackend, BackendType};
use crate::buffer::IoRingBuffer;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};

const MAX_JACK_SAMPLES: usize = 4096;

/// Wrapper to make `*mut` `Send` for cross-thread AudioIo access.
struct SendPtr(*mut Option<Box<dyn Fn()>>);
unsafe impl Send for SendPtr {}

#[derive(Debug)]
enum JackCommand {
    Start,
    Stop,
}

/// JACK audio backend.
///
/// Spawns a control thread that creates a JACK client, registers ports,
/// and manages activation. The real-time process callback uses `IoRingBuffer`
/// for cross-thread communication with the control/application thread.
pub struct JackBackend {
    config: AudioConfig,
    command_tx: Sender<JackCommand>,
    input_buffer: Arc<parking_lot::RwLock<IoRingBuffer>>,
    output_buffer: Arc<parking_lot::RwLock<IoRingBuffer>>,
    process_cb: SendPtr,
    thread_handle: Option<thread::JoinHandle<()>>,
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
}

impl fmt::Debug for JackBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JackBackend")
            .field("config", &self.config)
            .field("xruns", &self.xruns)
            .field("running", &self.running)
            .field("thread_handle", &self.thread_handle.is_some())
            .finish()
    }
}

impl JackBackend {
    /// Create a new JACK backend.
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            return Err(IoError::Unsupported(
                "JACK is only available on Linux and macOS".into(),
            ));
        }

        let buf_cap = (config.buffer_size * config.output_channels.max(1) * 4) as usize;
        let (command_tx, command_rx) = unbounded();

        let xruns = Arc::new(AtomicU32::new(0));
        let running = Arc::new(AtomicBool::new(false));

        let input_buffer = Arc::new(parking_lot::RwLock::new(IoRingBuffer::new(buf_cap)));
        let output_buffer = Arc::new(parking_lot::RwLock::new(IoRingBuffer::new(buf_cap)));

        let process_cb = Box::into_raw(Box::new(None::<Box<dyn Fn()>>));

        let t_xruns = xruns.clone();
        let t_input = input_buffer.clone();
        let t_output = output_buffer.clone();
        let t_config = config.clone();
        let t_running = running.clone();
        let t_process_cb = SendPtr(process_cb);

        let handle = thread::Builder::new()
            .name("rill-jack".into())
            .spawn(move || {
                run_jack_thread(command_rx, t_xruns, t_input, t_output, t_process_cb, t_config, t_running);
            })
            .map_err(|e| IoError::Backend(e.to_string()))?;

        Ok(Self {
            config,
            command_tx,
            input_buffer,
            output_buffer,
            process_cb: SendPtr(process_cb),
            thread_handle: Some(handle),
            xruns,
            running,
        })
    }
}

// ─── Custom ProcessHandler (Send-safe, no closure Send requirement) ─────

struct JackProcessHandler {
    process_cb: *mut Option<Box<dyn Fn()>>,
    in_port: Option<Port<AudioIn>>,
    out_port: Port<AudioOut>,
    ibuf: Arc<parking_lot::RwLock<IoRingBuffer>>,
    obuf: Arc<parking_lot::RwLock<IoRingBuffer>>,
    _xruns: Arc<AtomicU32>,
}

unsafe impl Send for JackProcessHandler {}

impl ProcessHandler for JackProcessHandler {
    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        let nframes = ps.n_frames() as usize;

        // 1. Read JACK input → input ring
        if let Some(ref in_p) = self.in_port {
            let inp = in_p.as_slice(ps);
            let mut ib = self.ibuf.write();
            ib.write(inp);
            drop(ib);
        }

        // 2. Call process callback (drives signal graph)
        unsafe {
            if let Some(ref cb) = *self.process_cb {
                cb();
            }
        }

        // 3. Read from output ring → JACK output buffers
        let out = self.out_port.as_mut_slice(ps);
        let mut ob = self.obuf.write();
        let mut temp = [0.0f32; MAX_JACK_SAMPLES];
        let n = ob.read(&mut temp[..nframes.min(MAX_JACK_SAMPLES)]);
        drop(ob);
        for i in 0..nframes.min(MAX_JACK_SAMPLES) {
            out[i] = if i < n { temp[i] } else { 0.0 };
        }

        Control::Continue
    }
}

fn run_jack_thread(
    command_rx: Receiver<JackCommand>,
    xruns: Arc<AtomicU32>,
    input_buffer: Arc<parking_lot::RwLock<IoRingBuffer>>,
    output_buffer: Arc<parking_lot::RwLock<IoRingBuffer>>,
    process_cb: SendPtr,
    config: AudioConfig,
    running: Arc<AtomicBool>,
) {
    while let Ok(cmd) = command_rx.recv() {
        if matches!(cmd, JackCommand::Start) {
            break;
        }
    }

    let client_name = config
        .output_device
        .as_deref()
        .unwrap_or("rill");

    let (client, _status) = match Client::new(client_name, ClientOptions::NO_START_SERVER) {
        Ok(c) => c,
        Err(e) => {
            log::error!("JACK client new: {e:?}");
            return;
        }
    };

    let in_port: Option<Port<AudioIn>> = if config.input_channels > 0 {
        match client.register_port("input", AudioIn) {
            Ok(p) => Some(p),
            Err(e) => {
                log::warn!("JACK input port: {e:?} — capture disabled");
                None
            }
        }
    } else {
        None
    };

    let out_port: Port<AudioOut> = match client.register_port("output", AudioOut) {
        Ok(p) => p,
        Err(e) => {
            log::error!("JACK output port: {e:?}");
            return;
        }
    };

    let handler = JackProcessHandler {
        process_cb: process_cb.0,
        in_port,
        out_port,
        ibuf: input_buffer.clone(),
        obuf: output_buffer.clone(),
        _xruns: xruns.clone(),
    };

    let active_client = match client.activate_async((), handler) {
        Ok(a) => a,
        Err(e) => {
            log::error!("JACK activate: {e:?}");
            return;
        }
    };

    running.store(true, Ordering::Release);

    while let Ok(cmd) = command_rx.recv() {
        if matches!(cmd, JackCommand::Stop) {
            break;
        }
    }

    running.store(false, Ordering::Release);
    drop(active_client);
}

// ============================================================================
// AudioIo impl
// ============================================================================

impl AudioIo for JackBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn()>) {
        unsafe { *self.process_cb.0 = Some(cb); }
    }

    fn read_input(&self, left: &mut [f32], right: &mut [f32]) -> usize {
        let mut buf = self.input_buffer.write();
        let mut temp = [0.0f32; MAX_JACK_SAMPLES];
        let n = buf.read(&mut temp[..left.len().min(right.len()).min(MAX_JACK_SAMPLES / 2).saturating_mul(2)]);
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
        let mut temp = [0.0f32; MAX_JACK_SAMPLES];
        let len = n.min(MAX_JACK_SAMPLES / 2);
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
        self.command_tx
            .send(JackCommand::Start)
            .map_err(|e| format!("JACK start: {e}"))
    }

    fn stop(&self) -> AudioIoResult<()> {
        self.running.store(false, Ordering::Release);
        self.command_tx
            .send(JackCommand::Stop)
            .map_err(|e| format!("JACK stop: {e}"))
    }
}

// ============================================================================
// AudioBackend impl
// ============================================================================

impl AudioBackend for JackBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::Jack
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
            .send(JackCommand::Start)
            .map_err(|e| IoError::Backend(e.to_string()))
    }

    fn stop(&mut self) -> IoResult<()> {
        self.running.store(false, Ordering::Release);
        let _ = self.command_tx.send(JackCommand::Stop);
        thread::sleep(Duration::from_millis(50));
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

    fn latency(&self) -> Duration {
        Duration::from_micros(
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

impl Drop for JackBackend {
    fn drop(&mut self) {
        let _ = self.command_tx.send(JackCommand::Stop);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}
