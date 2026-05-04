//! CPAL бэкенд (кросс-платформенный)

use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::RwLock;
use std::fmt;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::buffer::IoRingBuffer;

use crate::backend::{AudioBackend, BackendType};
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};

// Команды для потока
#[derive(Debug)]
enum Command {
    Init {
        input_device: Option<String>,
        output_device: Option<String>,
    },
    Start,
    Stop,
}

// Сообщения о состоянии
#[derive(Debug, PartialEq)]
enum Status {
    Initialized,
    Started,
    Stopped,
    Error(String),
}

/// CPAL бэкенд
pub struct CpalBackend {
    config: AudioConfig,
    host: Arc<cpal::Host>,
    command_tx: Sender<Command>,
    status_rx: Receiver<Status>,
    xruns: Arc<RwLock<u32>>,
    input_buffer: Arc<RwLock<IoRingBuffer>>,
    output_buffer: Arc<RwLock<IoRingBuffer>>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl fmt::Debug for CpalBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CpalBackend")
            .field("config", &self.config)
            .field("xruns", &self.xruns)
            .field("thread_handle", &self.thread_handle.is_some())
            .finish()
    }
}

impl CpalBackend {
    /// Создать новый CPAL бэкенд
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        let host = Arc::new(cpal::default_host());
        let buffer_size = (config.buffer_size * config.output_channels.max(config.input_channels).max(1) * 4) as usize;

        let (command_tx, command_rx) = unbounded();
        let (status_tx, status_rx) = unbounded();

        let xruns = Arc::new(RwLock::new(0));
        let input_buffer = Arc::new(RwLock::new(IoRingBuffer::new(buffer_size)));
        let output_buffer = Arc::new(RwLock::new(IoRingBuffer::new(buffer_size)));

        let thread_host = host.clone();
        let thread_input = input_buffer.clone();
        let thread_output = output_buffer.clone();
        let thread_xruns = xruns.clone();
        let thread_config = config.clone();

        // Запускаем поток для работы с CPAL
        let handle = thread::spawn(move || {
            run_cpal_thread(
                command_rx,
                status_tx,
                thread_host,
                thread_config,
                thread_input,
                thread_output,
                thread_xruns,
            );
        });

        Ok(Self {
            config,
            host,
            command_tx,
            status_rx,
            xruns,
            input_buffer,
            output_buffer,
            thread_handle: Some(handle),
        })
    }

    fn wait_for_status(&self, expected: Status) -> IoResult<()> {
        while let Ok(status) = self.status_rx.recv_timeout(Duration::from_millis(1000)) {
            match status {
                Status::Error(e) => return Err(IoError::Backend(e)),
                s if s == expected => return Ok(()),
                _ => continue,
            }
        }
        Err(IoError::Timeout)
    }
}

// Функция, выполняющаяся в отдельном потоке
fn run_cpal_thread(
    command_rx: Receiver<Command>,
    status_tx: Sender<Status>,
    host: Arc<cpal::Host>,
    config: AudioConfig,
    input_buffer: Arc<RwLock<IoRingBuffer>>,
    output_buffer: Arc<RwLock<IoRingBuffer>>,
    xruns: Arc<RwLock<u32>>,
) {
    let mut input_device: Option<cpal::Device> = None;
    let mut output_device: Option<cpal::Device> = None;
    let mut output_stream: Option<cpal::Stream> = None;
    let mut input_stream: Option<cpal::Stream> = None;

    while let Ok(cmd) = command_rx.recv() {
        match cmd {
            Command::Init {
                input_device: in_name,
                output_device: out_name,
            } => {
                input_device = find_device(&host, in_name.as_deref(), true).ok().flatten();
                output_device = find_device(&host, out_name.as_deref(), false)
                    .ok()
                    .flatten();

                // Очищаем буферы
                let cap = input_buffer.read().capacity();
                let zeros = vec![0.0f32; cap];
                input_buffer.write().write(&zeros);
                output_buffer.write().write(&zeros);

                let _ = status_tx.send(Status::Initialized);
            }

            Command::Start => {
                // ── Выходной (playback) поток ───────────────────────────────
                if let Some(dev) = &output_device {
                    let out_buf = output_buffer.clone();
                    let xruns_clone = xruns.clone();

                    let stream_config = cpal::StreamConfig {
                        channels: config.output_channels as u16,
                        sample_rate: cpal::SampleRate(config.sample_rate),
                        buffer_size: cpal::BufferSize::Fixed(config.buffer_size),
                    };

                    match dev.build_output_stream(
                        &stream_config,
                        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                            let mut out_buf_lock = out_buf.write();
                            let n = out_buf_lock.read(data);
                            drop(out_buf_lock);
                            if n < data.len() {
                                data[n..].fill(0.0);
                            }
                        },
                        move |err| {
                            eprintln!("Output stream error: {}", err);
                            *xruns_clone.write() += 1;
                        },
                        None,
                    ) {
                        Ok(s) => {
                            if s.play().is_ok() {
                                output_stream = Some(s);
                            } else {
                                let _ = status_tx.send(Status::Error("Failed to play output stream".into()));
                                continue;
                            }
                        }
                        Err(e) => {
                            let _ = status_tx.send(Status::Error(format!("Output stream build: {e}")));
                            continue;
                        }
                    }
                }

                // ── Входной (capture) поток ─────────────────────────────────
                if let Some(dev) = &input_device {
                    let in_buf = input_buffer.clone();
                    let xruns_clone = xruns.clone();

                    let stream_config = cpal::StreamConfig {
                        channels: config.input_channels as u16,
                        sample_rate: cpal::SampleRate(config.sample_rate),
                        buffer_size: cpal::BufferSize::Fixed(config.buffer_size),
                    };

                    match dev.build_input_stream(
                        &stream_config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            let mut in_buf_lock = in_buf.write();
                            in_buf_lock.write(data);
                            drop(in_buf_lock);
                        },
                        move |err| {
                            eprintln!("Input stream error: {}", err);
                            *xruns_clone.write() += 1;
                        },
                        None,
                    ) {
                        Ok(s) => {
                            if s.play().is_ok() {
                                input_stream = Some(s);
                            } else {
                                log::warn!("Failed to play input stream — capture may be inactive");
                            }
                        }
                        Err(e) => {
                            log::warn!("Input stream build: {e} — capture disabled");
                        }
                    }
                }

                let _ = status_tx.send(Status::Started);
            }

            Command::Stop => {
                if let Some(s) = output_stream.take() {
                    let _ = s.pause();
                }
                if let Some(s) = input_stream.take() {
                    let _ = s.pause();
                }
                let _ = status_tx.send(Status::Stopped);
            }
        }
    }
}

fn find_device(
    host: &cpal::Host,
    name: Option<&str>,
    is_input: bool,
) -> IoResult<Option<cpal::Device>> {
    let devices = if is_input {
        host.input_devices()
    } else {
        host.output_devices()
    }
    .map_err(|e| IoError::DeviceNotFound(e.to_string()))?;

    if let Some(name) = name {
        for device in devices {
            if let Ok(dev_name) = device.name() {
                if dev_name.contains(name) {
                    return Ok(Some(device));
                }
            }
        }
        Ok(None)
    } else if is_input {
        Ok(host.default_input_device())
    } else {
        Ok(host.default_output_device())
    }
}

impl AudioBackend for CpalBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::Cpal
    }

    fn config(&self) -> &AudioConfig {
        &self.config
    }

    fn config_mut(&mut self) -> &mut AudioConfig {
        &mut self.config
    }

    fn init(&mut self) -> IoResult<()> {
        self.command_tx
            .send(Command::Init {
                input_device: self.config.input_device.clone(),
                output_device: self.config.output_device.clone(),
            })
            .map_err(|e| IoError::Backend(e.to_string()))?;

        self.wait_for_status(Status::Initialized)
    }

    fn start(&mut self) -> IoResult<()> {
        self.command_tx
            .send(Command::Start)
            .map_err(|e| IoError::Backend(e.to_string()))?;

        self.wait_for_status(Status::Started)
    }

    fn stop(&mut self) -> IoResult<()> {
        self.command_tx
            .send(Command::Stop)
            .map_err(|e| IoError::Backend(e.to_string()))?;

        self.wait_for_status(Status::Stopped)
    }

    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize> {
        let mut input_buf = self.input_buffer.write();
        input_buf.read(buffer);
        Ok(buffer.len())
    }

    fn write(&mut self, buffer: &[f32]) -> IoResult<usize> {
        let mut output_buf = self.output_buffer.write();
        output_buf.write(buffer);
        Ok(buffer.len())
    }

    fn xruns(&self) -> u32 {
        *self.xruns.read()
    }

    fn latency(&self) -> Duration {
        Duration::from_micros(
            (1_000_000.0 * self.config.buffer_size as f64 / self.config.sample_rate as f64) as u64,
        )
    }

    fn list_input_devices(&self) -> Vec<String> {
        self.host
            .input_devices()
            .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }

    fn list_output_devices(&self) -> Vec<String> {
        self.host
            .output_devices()
            .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }
}

impl Drop for CpalBackend {
    fn drop(&mut self) {
        if let Some(handle) = self.thread_handle.take() {
            let _ = self.command_tx.send(Command::Stop);
            let _ = handle.join();
        }
    }
}
