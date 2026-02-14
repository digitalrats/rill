use std::sync::Arc;
use std::thread;
use std::time::Duration;
use parking_lot::RwLock;
use crossbeam_channel::{unbounded, Sender, Receiver};

use crate::backend::AudioBackend;
use crate::error::{IoError, IoResult};

/// Тип процессора аудио
pub trait AudioProcessor: Send + Sync {
    /// Обработать блок аудио
    fn process(&mut self, input: &[f32], output: &mut [f32]);
    
    /// Сбросить состояние
    fn reset(&mut self);
    
    /// Установить частоту дискретизации
    fn set_sample_rate(&mut self, sample_rate: f32);
}

/// Состояние аудио движка
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineState {
    Stopped,
    Running,
    Paused,
    Error,
}

/// Команды для потока обработки
enum EngineCommand {
    Start,
    Stop,
    Pause,
    Resume,
}

/// Основной аудио движок (синхронная версия)
pub struct AudioEngine<B: AudioBackend, P: AudioProcessor> {
    backend: Option<B>,  // <-- ИСПРАВЛЕНО: используем Option
    processor: Option<P>, // <-- ИСПРАВЛЕНО: используем Option
    state: Arc<RwLock<EngineState>>,
    command_tx: Sender<EngineCommand>,
    command_rx: Receiver<EngineCommand>,
    thread_handle: Option<thread::JoinHandle<()>>,
    sample_rate: f32,
    buffer_size: usize,
    channels: usize,
    xrun_count: Arc<RwLock<u32>>,
}

impl<B, P> AudioEngine<B, P>
where
    B: AudioBackend + 'static,
    P: AudioProcessor + 'static,
{
    pub fn new(backend: B, processor: P) -> Self {
        let sample_rate = backend.config().sample_rate as f32;
        let buffer_size = backend.config().buffer_size as usize;
        let channels = backend.config().channels as usize;
        
        let (command_tx, command_rx) = unbounded();
        
        Self {
            backend: Some(backend),
            processor: Some(processor),
            state: Arc::new(RwLock::new(EngineState::Stopped)),
            command_tx,
            command_rx,
            thread_handle: None,
            sample_rate,
            buffer_size,
            channels,
            xrun_count: Arc::new(RwLock::new(0)),
        }
    }
    
    /// Запустить движок
    pub fn start(&mut self) -> IoResult<()> {
        if *self.state.read() == EngineState::Running {
            return Ok(());
        }
        
        // Забираем backend и processor из Option
        let mut backend = self.backend.take().ok_or_else(|| IoError::Backend("Backend already taken".to_string()))?;
        let mut processor = self.processor.take().ok_or_else(|| IoError::Backend("Processor already taken".to_string()))?;
        
        backend.init()?;
        backend.start()?;
        
        *self.state.write() = EngineState::Running;
        
        // Запускаем поток обработки
        let state = self.state.clone();
        let xrun_count = self.xrun_count.clone();
        let buffer_size = self.buffer_size;
        let channels = self.channels;
        let command_rx = self.command_rx.clone();
        
        let handle = thread::spawn(move || {
            let total_samples = buffer_size * channels;
            let mut input_buffer = vec![0.0f32; total_samples];
            let mut output_buffer = vec![0.0f32; total_samples];
            
            while *state.read() == EngineState::Running {
                // Проверяем команды
                while let Ok(cmd) = command_rx.try_recv() {
                    match cmd {
                        EngineCommand::Stop => {
                            *state.write() = EngineState::Stopped;
                            return;
                        }
                        EngineCommand::Pause => {
                            *state.write() = EngineState::Paused;
                        }
                        EngineCommand::Resume => {
                            *state.write() = EngineState::Running;
                        }
                        _ => {}
                    }
                }
                
                // Читаем входные данные
                match backend.read(&mut input_buffer) {
                    Ok(read) if read > 0 => {
                        // Обрабатываем
                        processor.process(&input_buffer[..read], &mut output_buffer[..read]);
                        
                        // Записываем выходные данные
                        if let Err(e) = backend.write(&output_buffer[..read]) {
                            eprintln!("Write error: {}", e);
                            *xrun_count.write() += 1;
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("Read error: {}", e);
                        *xrun_count.write() += 1;
                    }
                }
                
                thread::sleep(Duration::from_micros(1000));
            }
        });
        
        self.thread_handle = Some(handle);
        
        Ok(())
    }
    
    /// Остановить движок
    pub fn stop(&mut self) -> IoResult<()> {
        self.command_tx.send(EngineCommand::Stop)
            .map_err(|e| IoError::Backend(e.to_string()))?;
        
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
        
        if let Some(backend) = &mut self.backend {
            backend.stop()?;
        }
        
        *self.state.write() = EngineState::Stopped;
        
        Ok(())
    }
    
    /// Приостановить обработку
    pub fn pause(&mut self) -> IoResult<()> {
        if *self.state.read() == EngineState::Running {
            self.command_tx.send(EngineCommand::Pause)
                .map_err(|e| IoError::Backend(e.to_string()))?;
            *self.state.write() = EngineState::Paused;
        }
        Ok(())
    }
    
    /// Возобновить обработку
    pub fn resume(&mut self) -> IoResult<()> {
        if *self.state.read() == EngineState::Paused {
            self.command_tx.send(EngineCommand::Resume)
                .map_err(|e| IoError::Backend(e.to_string()))?;
            *self.state.write() = EngineState::Running;
        }
        Ok(())
    }
    
    /// Получить состояние движка
    pub fn state(&self) -> EngineState {
        *self.state.read()
    }
    
    /// Получить количество xrun'ов
    pub fn xruns(&self) -> u32 {
        *self.xrun_count.read()
    }
    
    /// Получить текущую задержку
    pub fn latency(&self) -> Duration {
        if let Some(backend) = &self.backend {
            backend.latency()
        } else {
            Duration::from_micros(0)
        }
    }
    
    /// Получить частоту дискретизации
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
    
    /// Получить размер буфера
    pub fn buffer_size(&self) -> usize {
        self.buffer_size
    }

    /// Получить мутабельную ссылку на процессор (если движок не запущен)
    pub fn processor_mut(&mut self) -> Option<&mut P> {
        self.processor.as_mut()
    }
    
    /// Отправить команду изменения параметра в поток обработки
    pub fn update_parameter<F>(&self, f: F) -> IoResult<()>
    where
        F: FnOnce(&mut P) + Send + 'static,
    {
        // Здесь должна быть реализация через каналы
        // Для простоты пока заглушка
        Ok(())
    }
}  