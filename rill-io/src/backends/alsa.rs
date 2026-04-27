//! ALSA бэкенд для Linux

use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::RwLock;
use std::fmt;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use alsa::pcm::{Access, Format, HwParams};
use alsa::{Direction, ValueOr, PCM};

use crate::buffer::IoRingBuffer;

use crate::backend::{AudioBackend, BackendType};
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};

/// Команды для ALSA потока
#[derive(Debug)]
enum AlsaCommand {
    Start,
    Stop,
}

/// Состояние ALSA потока
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AlsaState {
    Stopped,
    Running,
    Error,
}

/// ALSA бэкенд
pub struct AlsaBackend {
    config: AudioConfig,
    command_tx: Sender<AlsaCommand>,
    xruns: Arc<RwLock<u32>>,
    input_buffer: Arc<RwLock<IoRingBuffer>>,
    output_buffer: Arc<RwLock<IoRingBuffer>>,
    thread_handle: Option<thread::JoinHandle<()>>,
    state: Arc<RwLock<AlsaState>>,
    device_name: Arc<RwLock<String>>,
}

impl fmt::Debug for AlsaBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AlsaBackend")
            .field("config", &self.config)
            .field("xruns", &self.xruns)
            .field("state", &self.state.read().clone())
            .field("device_name", &self.device_name.read().clone())
            .field("thread_handle", &self.thread_handle.is_some())
            .finish()
    }
}

impl AlsaBackend {
    /// Создать новый ALSA бэкенд
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        let buffer_size = (config.buffer_size * config.output_channels * 4) as usize;
        let (command_tx, command_rx) = unbounded();

        let xruns = Arc::new(RwLock::new(0));
        let input_buffer = Arc::new(RwLock::new(IoRingBuffer::new(buffer_size)));
        let output_buffer = Arc::new(RwLock::new(IoRingBuffer::new(buffer_size)));
        let state = Arc::new(RwLock::new(AlsaState::Stopped));
        let device_name = Arc::new(RwLock::new(
            config
                .output_device
                .clone()
                .unwrap_or_else(|| "default".to_string()),
        ));

        let thread_xruns = xruns.clone();
        let thread_input = input_buffer.clone();
        let thread_output = output_buffer.clone();
        let thread_state = state.clone();
        let thread_config = config.clone();
        let thread_device_name = device_name.clone();

        // Запускаем поток для работы с ALSA
        let handle = thread::spawn(move || {
            run_alsa_thread(
                command_rx,
                thread_xruns,
                thread_input,
                thread_output,
                thread_state,
                thread_config,
                thread_device_name,
            );
        });

        Ok(Self {
            config,
            command_tx,
            xruns,
            input_buffer,
            output_buffer,
            thread_handle: Some(handle),
            state,
            device_name,
        })
    }

    /// Установить имя устройства
    pub fn with_device(self, device: &str) -> Self {
        *self.device_name.write() = device.to_string();
        self
    }

    /// Получить состояние
    #[allow(dead_code)]
    pub(crate) fn state(&self) -> AlsaState {
        *self.state.read()
    }
}

// Функция, выполняющаяся в отдельном потоке
fn run_alsa_thread(
    command_rx: Receiver<AlsaCommand>,
    xruns: Arc<RwLock<u32>>,
    _input_buffer: Arc<RwLock<IoRingBuffer>>,
    output_buffer: Arc<RwLock<IoRingBuffer>>,
    state: Arc<RwLock<AlsaState>>,
    config: AudioConfig,
    device_name: Arc<RwLock<String>>,
) {
    let mut pcm_handle: Option<PCM> = None;

    while let Ok(cmd) = command_rx.recv() {
        match cmd {
            AlsaCommand::Start => {
                if pcm_handle.is_none() {
                    // Получаем имя устройства
                    let dev_name = device_name.read().clone();

                    // Открываем PCM устройство
                    match PCM::new(&dev_name, Direction::Playback, false) {
                        Ok(pcm) => {
                            pcm_handle = Some(pcm);
                        }
                        Err(e) => {
                            eprintln!("Failed to open ALSA device {}: {}", dev_name, e);
                            *state.write() = AlsaState::Error;
                            continue;
                        }
                    }
                }

                if let Some(pcm) = &mut pcm_handle {
                    // Настраиваем параметры
                    if let Err(e) = configure_alsa_pcm(pcm, &config) {
                        eprintln!("Failed to configure ALSA: {}", e);
                        *state.write() = AlsaState::Error;
                        continue;
                    }

                    // Запускаем воспроизведение
                    if let Err(e) = pcm.start() {
                        eprintln!("Failed to start ALSA: {}", e);
                        *state.write() = AlsaState::Error;
                        continue;
                    }

                    let mut running = true;
                    *state.write() = AlsaState::Running;

                    // Запускаем цикл обработки
                    let buffer_size = (config.buffer_size * config.output_channels) as usize;
                    let mut playback_buffer = vec![0i16; buffer_size]; // ALSA обычно использует i16

                    while running {
                        // Читаем из выходного буфера
                        let mut output = output_buffer.write();
                        let mut temp = vec![0.0f32; buffer_size];
                        output.read(&mut temp);
                        drop(output); // Освобождаем блокировку

                        // Конвертируем f32 в i16 для ALSA
                        for (i, sample) in playback_buffer.iter_mut().enumerate() {
                            *sample = (temp[i].clamp(-1.0, 1.0) * 32767.0) as i16;
                        }

                        // Записываем в ALSA
                        match pcm.io_i16() {
                            Ok(io) => {
                                match io.writei(&playback_buffer) {
                                    Ok(_) => {}
                                    Err(e) => {
                                        eprintln!("ALSA write error: {}", e);
                                        *xruns.write() += 1;

                                        // Пытаемся восстановиться
                                        if let Err(recover_err) = pcm.try_recover(e, true) {
                                            eprintln!("Failed to recover ALSA: {}", recover_err);
                                            running = false;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Failed to get ALSA IO: {}", e);
                                *xruns.write() += 1;
                            }
                        }

                        // Проверяем команды без блокировки
                        while let Ok(cmd) = command_rx.try_recv() {
                            match cmd {
                                AlsaCommand::Stop => {
                                    running = false;
                                }
                                _ => {}
                            }
                        }

                        thread::sleep(Duration::from_micros(1000));
                    }

                    let _ = pcm.drain();
                }
            }

            AlsaCommand::Stop => {
                if let Some(pcm) = &mut pcm_handle {
                    let _ = pcm.drain();
                }
                *state.write() = AlsaState::Stopped;
            }
        }
    }
}

// Настройка параметров ALSA PCM
fn configure_alsa_pcm(pcm: &mut PCM, config: &AudioConfig) -> IoResult<()> {
    let hw_params = HwParams::any(pcm).map_err(|e| IoError::Config(e.to_string()))?;

    // Устанавливаем параметры
    hw_params
        .set_access(Access::RWInterleaved)
        .map_err(|e| IoError::Config(e.to_string()))?;

    hw_params
        .set_format(Format::s16())
        .map_err(|e| IoError::Config(e.to_string()))?;

    hw_params
        .set_rate(config.sample_rate as u32, ValueOr::Nearest)
        .map_err(|e| IoError::Config(e.to_string()))?;

    hw_params
        .set_channels(config.output_channels as u32)
        .map_err(|e| IoError::Config(e.to_string()))?;

    // Устанавливаем размер буфера
    hw_params
        .set_buffer_size(config.buffer_size as alsa::pcm::Frames)
        .map_err(|e| IoError::Config(e.to_string()))?;

    // Применяем параметры
    pcm.hw_params(&hw_params)
        .map_err(|e| IoError::Config(e.to_string()))?;

    Ok(())
}

impl AudioBackend for AlsaBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::Alsa
    }

    fn config(&self) -> &AudioConfig {
        &self.config
    }

    fn config_mut(&mut self) -> &mut AudioConfig {
        &mut self.config
    }

    fn init(&mut self) -> IoResult<()> {
        // Очищаем буферы
        let cap = self.input_buffer.read().capacity();
        let zeros = vec![0.0f32; cap];
        self.input_buffer.write().write(&zeros);
        self.output_buffer.write().write(&zeros);

        Ok(())
    }

    fn start(&mut self) -> IoResult<()> {
        self.command_tx
            .send(AlsaCommand::Start)
            .map_err(|e| IoError::Backend(e.to_string()))?;
        Ok(())
    }

    fn stop(&mut self) -> IoResult<()> {
        self.command_tx
            .send(AlsaCommand::Stop)
            .map_err(|e| IoError::Backend(e.to_string()))?;

        // Даем время потоку остановиться
        thread::sleep(Duration::from_millis(10));

        Ok(())
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
        // В ALSA обычно используем "default", "hw:0,0", "plughw:0,0" и т.д.
        vec![
            "default".to_string(),
            "hw:0,0".to_string(),
            "hw:1,0".to_string(),
            "plughw:0,0".to_string(),
            "plughw:1,0".to_string(),
        ]
    }

    fn list_output_devices(&self) -> Vec<String> {
        vec![
            "default".to_string(),
            "hw:0,0".to_string(),
            "hw:1,0".to_string(),
            "plughw:0,0".to_string(),
            "plughw:1,0".to_string(),
            "dmix:0".to_string(),
        ]
    }
}

impl Drop for AlsaBackend {
    fn drop(&mut self) {
        let _ = self.command_tx.send(AlsaCommand::Stop);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}
