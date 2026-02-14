use std::sync::Arc;
use std::time::Duration;
use std::thread;
use parking_lot::RwLock;
use crossbeam_channel::{unbounded, Sender, Receiver};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use kama_buffers::RingBuffer;

use crate::backend::AudioBackend;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};

// Команды для потока
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

pub struct CpalBackend {
    config: AudioConfig,
    host: Arc<cpal::Host>,
    command_tx: Sender<Command>,
    status_rx: Receiver<Status>,
    xruns: Arc<RwLock<u32>>,
    input_buffer: Arc<RwLock<RingBuffer>>,
    output_buffer: Arc<RwLock<RingBuffer>>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl CpalBackend {
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        let host = Arc::new(cpal::default_host());
        let buffer_size = (config.buffer_size * config.channels * 4) as usize;
        
        let (command_tx, command_rx) = unbounded();
        let (status_tx, status_rx) = unbounded();
        
        let xruns = Arc::new(RwLock::new(0));
        let input_buffer = Arc::new(RwLock::new(RingBuffer::new(buffer_size)));
        let output_buffer = Arc::new(RwLock::new(RingBuffer::new(buffer_size)));
        
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
    input_buffer: Arc<RwLock<RingBuffer>>,
    output_buffer: Arc<RwLock<RingBuffer>>,
    xruns: Arc<RwLock<u32>>,
) {
    let mut _input_device: Option<cpal::Device> = None;
    let mut output_device: Option<cpal::Device> = None;
    let mut stream: Option<cpal::Stream> = None;
    
    while let Ok(cmd) = command_rx.recv() {
        match cmd {
            Command::Init { input_device: in_name, output_device: out_name } => {
                // Находим устройства
                _input_device = find_device(&host, in_name.as_deref(), true).ok().flatten();
                output_device = find_device(&host, out_name.as_deref(), false).ok().flatten();
                
                // Очищаем буферы
                let zeros = vec![0.0f32; input_buffer.read().size()];
                input_buffer.write().write(&zeros);
                output_buffer.write().write(&zeros);
                
                let _ = status_tx.send(Status::Initialized);
            }
            
            Command::Start => {
                if let Some(dev) = &output_device {
                    let out_buf = output_buffer.clone();
                    let xruns_clone = xruns.clone();
                    
                    let stream_config = cpal::StreamConfig {
                        channels: config.channels as u16,
                        sample_rate: cpal::SampleRate(config.sample_rate),
                        buffer_size: cpal::BufferSize::Fixed(config.buffer_size),
                    };
                    
                    match dev.build_output_stream(
                        &stream_config,
                        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                            let out_buf_lock = out_buf.read();
                            let mut temp = vec![0.0f32; data.len()];
                            // В реальном приложении нужно читать из буфера
                            out_buf_lock.read(0, &mut temp);
                            data.copy_from_slice(&temp[..data.len()]);
                        },
                        move |err| {
                            eprintln!("Stream error: {}", err);
                            *xruns_clone.write() += 1;
                        },
                        None,
                    ) {
                        Ok(s) => {
                            if s.play().is_ok() {
                                stream = Some(s);
                                let _ = status_tx.send(Status::Started);
                            }
                        }
                        Err(e) => {
                            let _ = status_tx.send(Status::Error(e.to_string()));
                        }
                    }
                }
            }
            
            Command::Stop => {
                if let Some(s) = stream.take() {
                    let _ = s.pause();
                }
                let _ = status_tx.send(Status::Stopped);
            }
        }
    }
}

fn find_device(host: &cpal::Host, name: Option<&str>, is_input: bool) -> IoResult<Option<cpal::Device>> {
    let devices = if is_input {
        host.input_devices()
    } else {
        host.output_devices()
    }.map_err(|e| IoError::DeviceNotFound(e.to_string()))?;
    
    if let Some(name) = name {
        for device in devices {
            if let Ok(dev_name) = device.name() {
                if dev_name.contains(name) {
                    return Ok(Some(device));
                }
            }
        }
        Ok(None)
    } else {
        if is_input {
            Ok(host.default_input_device())
        } else {
            Ok(host.default_output_device())
        }
    }
}

impl AudioBackend for CpalBackend {
    fn name(&self) -> &'static str {
        "CPAL"
    }
    
    fn config(&self) -> &AudioConfig {
        &self.config
    }
    
    fn config_mut(&mut self) -> &mut AudioConfig {
        &mut self.config
    }
    
    fn init(&mut self) -> IoResult<()> {
        self.command_tx.send(Command::Init {
            input_device: self.config.input_device.clone(),
            output_device: self.config.output_device.clone(),
        }).map_err(|e| IoError::Backend(e.to_string()))?;
        
        self.wait_for_status(Status::Initialized)
    }
    
    fn start(&mut self) -> IoResult<()> {
        self.command_tx.send(Command::Start)
            .map_err(|e| IoError::Backend(e.to_string()))?;
        
        self.wait_for_status(Status::Started)
    }
    
    fn stop(&mut self) -> IoResult<()> {
        self.command_tx.send(Command::Stop)
            .map_err(|e| IoError::Backend(e.to_string()))?;
        
        self.wait_for_status(Status::Stopped)
    }
    
    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize> {
        let input_buf = self.input_buffer.read();
        input_buf.read(0, buffer);
        Ok(buffer.len())
    }
    
    fn write(&mut self, buffer: &[f32]) -> IoResult<usize> {
        let mut output_buf = self.output_buffer.write();  // <-- ИСПРАВЛЕНО: добавил mut
        output_buf.write(buffer);
        Ok(buffer.len())
    }
    
    fn xruns(&self) -> u32 {
        *self.xruns.read()
    }
    
    fn latency(&self) -> Duration {
        Duration::from_micros(
            (1_000_000.0 * self.config.buffer_size as f64 / self.config.sample_rate as f64) as u64
        )
    }
    
    fn is_available(&self) -> bool {
        true
    }
    
    fn list_input_devices(&self) -> Vec<String> {
        self.host.input_devices()
            .map(|devices| {
                devices.filter_map(|d| d.name().ok()).collect()
            })
            .unwrap_or_default()
    }
    
    fn list_output_devices(&self) -> Vec<String> {
        self.host.output_devices()
            .map(|devices| {
                devices.filter_map(|d| d.name().ok()).collect()
            })
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