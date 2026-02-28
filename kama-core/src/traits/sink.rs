// kama-core/src/traits/sink.rs
use crate::traits::AudioError;

/// Активный приемник аудиоданных.
/// Запрашивает данные у подключенного к нему процессора или графа.
pub trait Sink<const BUF_SIZE: usize>: Send + Sync {
    /// Запустить поток вывода.
    /// Этот метод должен быть блокирующим и выполняться в отдельном потоке.
    /// Он будет вызывать коллбэк `pull_callback` для получения каждого блока.
    fn run(&mut self, pull_callback: Box<dyn FnMut(&mut [f32; BUF_SIZE]) -> Result<(), AudioError> + Send>);

    /// Остановить поток вывода.
    fn stop(&mut self);

    /// Получить частоту дискретизации, с которой работает Sink.
    fn sample_rate(&self) -> u32;
}