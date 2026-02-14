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
enum ProcessorCommand<P: AudioProcessor> {
    Update(Box<dyn FnOnce(&mut P) + Send>),
}

/// Основной аудио движок (синхронная версия)
pub struct AudioEngine<B, P>
where
    B: AudioBackend + 'static,  // <-- Добавлено 'static в определение
    P: AudioProcessor + 'static, // <-- Добавлено 'static в определение
{
    backend: Option<B>,
    processor: Arc<RwLock<Option<P>>>,
    state: Arc<RwLock<EngineState>>,
    command_tx: Sender<ProcessorCommand<P>>,
    command_rx: Receiver<ProcessorCommand<P>>,
    thread_handle: Option<thread::JoinHandle<()>>,
    sample_rate: f32,
    buffer_size: usize,
    channels: usize,
    xrun_count: Arc<RwLock<u32>>,
}

impl<B, P> AudioEngine<B, P>
where
    B: AudioBackend + Send + Sync + 'static,
    P: AudioProcessor + Send + Sync + 'static,
{
    pub fn new(backend: B, processor: P) -> Self {
        let sample_rate = backend.config().sample_rate as f32;
        let buffer_size = backend.config().buffer_size as usize;
        let channels = backend.config().channels as usize;
        
        let (command_tx, command_rx) = unbounded();
        
        Self {
            backend: Some(backend),
            processor: Arc::new(RwLock::new(Some(processor))),
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
        
        // Забираем backend из Option
        let mut backend = self.backend.take().ok_or_else(|| IoError::Backend("Backend already taken".to_string()))?;
        
        backend.init()?;
        backend.start()?;
        
        *self.state.write() = EngineState::Running;
        
        // Клонируем для потока
        let state = self.state.clone();
        let xrun_count = self.xrun_count.clone();
        let processor = self.processor.clone();
        let buffer_size = self.buffer_size;
        let channels = self.channels;
        let command_rx = self.command_rx.clone();
        
        // Запускаем поток обработки
        let handle = thread::spawn(move || {
            let total_samples = buffer_size * channels;
            let mut input_buffer = vec![0.0f32; total_samples];
            let mut output_buffer = vec![0.0f32; total_samples];
            
            while *state.read() == EngineState::Running {
                // Обрабатываем команды
                while let Ok(cmd) = command_rx.try_recv() {
                    match cmd {
                        ProcessorCommand::Update(f) => {
                            if let Some(proc) = processor.write().as_mut() {
                                f(proc);
                            }
                        }
                    }
                }
                
                // Читаем входные данные
                match backend.read(&mut input_buffer) {
                    Ok(read) if read > 0 => {
                        // Получаем процессор и обрабатываем
                        if let Some(proc) = processor.write().as_mut() {
                            proc.process(&input_buffer[..read], &mut output_buffer[..read]);
                        }
                        
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
        *self.state.write() = EngineState::Stopped;
        
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
        
        if let Some(backend) = &mut self.backend {
            backend.stop()?;
        }
        
        Ok(())
    }
    
    /// Приостановить обработку
    pub fn pause(&mut self) -> IoResult<()> {
        if *self.state.read() == EngineState::Running {
            *self.state.write() = EngineState::Paused;
        }
        Ok(())
    }
    
    /// Возобновить обработку
    pub fn resume(&mut self) -> IoResult<()> {
        if *self.state.read() == EngineState::Paused {
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
    
    /// Выполнить операцию с процессором (если движок не запущен)
    pub fn with_processor<F, R>(&mut self, f: F) -> Option<R>
    where
        F: FnOnce(&mut P) -> R,
    {
        if *self.state.read() == EngineState::Running {
            None
        } else {
            if let Some(proc) = self.processor.write().as_mut() {
                Some(f(proc))
            } else {
                None
            }
        }
    }
    
    /// Обновить процессор через замыкание (безопасно для многопоточности)
    /// 
    /// Это единственный способ изменить состояние процессора, когда движок запущен.
    /// Замыкание выполняется в контексте потока обработки.
    pub fn update_processor<F>(&self, f: F) -> IoResult<()>
    where
        F: FnOnce(&mut P) + Send + 'static,
    {
        self.command_tx.send(ProcessorCommand::Update(Box::new(f)))
            .map_err(|e| IoError::Backend(e.to_string()))?;
        Ok(())
    }
    
    /// Отправить команду изменения параметра в поток обработки (устаревший метод)
    #[deprecated(since = "0.2.0", note = "use update_processor instead")]
    pub fn update_parameter<F>(&self, f: F) -> IoResult<()>
    where
        F: FnOnce(&mut P) + Send + 'static,
    {
        self.update_processor(f)
    }
}

impl<B, P> Drop for AudioEngine<B, P>
where
    B: AudioBackend + 'static,
    P: AudioProcessor + 'static,
{
    fn drop(&mut self) {
        let _ = self.stop();
    }
}