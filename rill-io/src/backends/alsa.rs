//! ALSA бэкенд для Linux

use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::RwLock;
use std::fmt;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use alsa::pcm::{Access, Format, HwParams};
use alsa::{Direction, ValueOr, PCM};

use crate::audio_io::AudioIo;
use crate::buffer::IoRingBuffer;

use crate::backend::{AudioBackend, BackendType};
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};

/// Wrapper to make `*mut` `Send` for cross-thread AudioIo access.
struct SendPtr(*mut Option<Box<dyn Fn()>>);
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}
impl Copy for SendPtr {}
impl Clone for SendPtr {
    fn clone(&self) -> Self { *self }
}

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
    process_cb: SendPtr,
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

        let process_cb = Box::into_raw(Box::new(None::<Box<dyn Fn()>>));
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

        let thread_cb = SendPtr(process_cb);
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
                thread_cb,
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
            process_cb: SendPtr(process_cb),
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
    process_cb: SendPtr,
    xruns: Arc<RwLock<u32>>,
    input_buffer: Arc<RwLock<IoRingBuffer>>,
    output_buffer: Arc<RwLock<IoRingBuffer>>,
    state: Arc<RwLock<AlsaState>>,
    config: AudioConfig,
    device_name: Arc<RwLock<String>>,
) {
    while let Ok(cmd) = command_rx.recv() {
        match cmd {
            AlsaCommand::Start => {
                let dev_name = device_name.read().clone();

                // Открываем PCM playback устройство
                let pcm_playback = match PCM::new(&dev_name, Direction::Playback, false) {
                    Ok(pcm) => pcm,
                    Err(e) => {
                        eprintln!("Failed to open ALSA playback device {}: {}", dev_name, e);
                        *state.write() = AlsaState::Error;
                        continue;
                    }
                };

                // Открываем PCM capture устройство (если нужны входные каналы)
                let pcm_capture: Option<PCM> = if config.input_channels > 0 {
                    match PCM::new(&dev_name, Direction::Capture, false) {
                        Ok(pcm) => Some(pcm),
                        Err(e) => {
                            log::warn!("Failed to open ALSA capture device {}: {} — capture disabled", dev_name, e);
                            None
                        }
                    }
                } else {
                    None
                };

                // Настраиваем параметры playback
                if let Err(e) = configure_alsa_pcm(&pcm_playback, config.output_channels, &config) {
                    eprintln!("Failed to configure ALSA playback: {}", e);
                    *state.write() = AlsaState::Error;
                    continue;
                }

                // Настраиваем параметры capture
                if let Some(ref pcm) = pcm_capture {
                    if let Err(e) = configure_alsa_pcm(pcm, config.input_channels, &config) {
                        log::warn!("Failed to configure ALSA capture: {} — capture disabled", e);
                        // Продолжаем без захвата
                    }
                }

                // Запускаем воспроизведение
                if let Err(e) = pcm_playback.start() {
                    eprintln!("Failed to start ALSA playback: {}", e);
                    *state.write() = AlsaState::Error;
                    continue;
                }

                // Запускаем захват
                if let Some(ref pcm) = pcm_capture {
                    if let Err(e) = pcm.start() {
                        log::warn!("Failed to start ALSA capture: {} — capture disabled", e);
                    }
                }

                let mut running = true;
                *state.write() = AlsaState::Running;

                let out_buffer_size = (config.buffer_size * config.output_channels) as usize;
                let in_buffer_size = (config.buffer_size * config.input_channels) as usize;
                let mut playback_buffer = vec![0i16; out_buffer_size];
                let mut capture_buffer = vec![0i16; in_buffer_size.max(1)];

                while running {
                    // Wait until playback needs more data (event-driven, no sleep).
                    match pcm_playback.wait(None) {
                        Ok(true) => {}
                        Ok(false) => continue,  // spurious wake, retry
                        Err(e) => {
                            eprintln!("ALSA playback wait error: {}", e);
                            if let Err(recover_err) = pcm_playback.try_recover(e, true) {
                                eprintln!("Failed to recover ALSA playback: {}", recover_err);
                                running = false;
                            }
                            continue;
                        }
                    }

                    // ── Capture: читаем из ALSA → input ring ────────────────
                    if let Some(ref pcm) = pcm_capture {
                        match pcm.io_i16() {
                            Ok(io) => {
                                match io.readi(&mut capture_buffer) {
                                    Ok(n_read) => {
                                        let n = n_read * config.input_channels as usize;
                                        let mut temp = vec![0.0f32; n];
                                        for (i, sample) in capture_buffer[..n].iter().enumerate() {
                                            temp[i] = *sample as f32 / 32768.0;
                                        }
                                        let mut input = input_buffer.write();
                                        input.write(&temp);
                                        drop(input);
                                    }
                                    Err(e) => {
                                        eprintln!("ALSA capture error: {}", e);
                                        *xruns.write() += 1;
                                        if let Err(recover_err) = pcm.try_recover(e, true) {
                                            eprintln!("Failed to recover ALSA capture: {}", recover_err);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Failed to get ALSA capture IO: {}", e);
                                *xruns.write() += 1;
                            }
                        }
                    }

                    // ── Process callback: drives the signal graph ────────────
                    if let Some(ref cb) = unsafe { &*process_cb.0 } {
                        cb();
                    }

                    // ── Playback: читаем из output ring → ALSA ──────────────
                    {
                        let mut output = output_buffer.write();
                        let mut temp = vec![0.0f32; out_buffer_size];
                        let n = output.read(&mut temp);
                        drop(output);

                        for (i, sample) in playback_buffer.iter_mut().enumerate() {
                            *sample = if i < n {
                                (temp[i].clamp(-1.0, 1.0) * 32767.0) as i16
                            } else {
                                0
                            };
                        }
                    }

                    match pcm_playback.io_i16() {
                        Ok(io) => {
                            match io.writei(&playback_buffer) {
                                Ok(_) => {}
                                Err(e) => {
                                    eprintln!("ALSA playback error: {}", e);
                                    *xruns.write() += 1;
                                    if let Err(recover_err) = pcm_playback.try_recover(e, true) {
                                        eprintln!("Failed to recover ALSA playback: {}", recover_err);
                                        running = false;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to get ALSA playback IO: {}", e);
                            *xruns.write() += 1;
                        }
                    }

                    // Проверяем команды без блокировки
                    while let Ok(cmd) = command_rx.try_recv() {
                        match cmd {
                            AlsaCommand::Stop => {
                                running = false;
                            }
                            AlsaCommand::Start => {}
                        }
                    }
                }

                let _ = pcm_playback.drain();
                if let Some(ref pcm) = pcm_capture {
                    let _ = pcm.drain();
                }
            }

            AlsaCommand::Stop => {
                *state.write() = AlsaState::Stopped;
            }
        }
    }
}

// Настройка параметров ALSA PCM
fn configure_alsa_pcm(pcm: &PCM, channels: u32, config: &AudioConfig) -> IoResult<()> {
    let hw_params = HwParams::any(pcm).map_err(|e| IoError::Config(e.to_string()))?;

    // Устанавливаем параметры
    hw_params
        .set_access(Access::RWInterleaved)
        .map_err(|e| IoError::Config(e.to_string()))?;

    hw_params
        .set_format(Format::s16())
        .map_err(|e| IoError::Config(e.to_string()))?;

    hw_params
        .set_rate(config.sample_rate, ValueOr::Nearest)
        .map_err(|e| IoError::Config(e.to_string()))?;

    hw_params
        .set_channels(channels)
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

impl AudioIo for AlsaBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn()>) {
        unsafe { *self.process_cb.0 = Some(cb); }
    }

    fn read_input(&self, left: &mut [f32], right: &mut [f32]) -> usize {
        let frames = left.len().min(right.len());
        let mut buf = self.input_buffer.write();
        let mut temp = vec![0.0f32; frames * 2];
        let n = buf.read(&mut temp);
        drop(buf);
        let frames_out = n / 2;
        for i in 0..frames_out.min(frames) {
            left[i] = temp[i * 2];
            right[i] = temp[i * 2 + 1];
        }
        frames_out
    }

    fn write_output(&self, left: &[f32], right: &[f32]) -> usize {
        let frames = left.len().min(right.len());
        let mut temp = vec![0.0f32; frames * 2];
        for i in 0..frames {
            temp[i * 2] = left[i];
            temp[i * 2 + 1] = right[i];
        }
        let mut buf = self.output_buffer.write();
        buf.write(&temp) / 2
    }

    fn start(&self) -> crate::audio_io::IoResult<()> {
        self.command_tx
            .send(AlsaCommand::Start)
            .map_err(|e| format!("{e}"))?;
        Ok(())
    }

    fn stop(&self) -> crate::audio_io::IoResult<()> {
        self.command_tx
            .send(AlsaCommand::Stop)
            .map_err(|e| format!("{e}"))?;
        Ok(())
    }
}

impl Drop for AlsaBackend {
    fn drop(&mut self) {
        let _ = self.command_tx.send(AlsaCommand::Stop);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
        // Free the heap-allocated callback slot.
        unsafe { drop(Box::from_raw(self.process_cb.0)); }
    }
}
