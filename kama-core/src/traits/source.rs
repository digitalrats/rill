// kama-core/src/traits/source.rs
use crate::ProcessError as AudioError;

/// Активный источник сигнала.
/// Заполняет предоставленный выходной буфер новыми семплами.
pub trait Source<const BUF_SIZE: usize>: Send + Sync {
    /// Сгенерировать следующий блок аудио.
    fn generate(&mut self, output: &mut [f32; BUF_SIZE]) -> Result<(), AudioError>;

    /// Инициализация источника с частотой дискретизации.
    fn init(&mut self, sample_rate: f32);

    /// Сброс внутреннего состояния (например, фазы осциллятора).
    fn reset(&mut self);
}