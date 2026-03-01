// kama-core/src/traits/processor.rs
use crate::{ProcessError as AudioError, traits::{ParameterId, PortId, ParamValue, PortType}};

/// Пассивный процессор аудио.
/// Получает входные буферы и заполняет выходные.
/// Не должен знать об источнике или приемнике данных.
pub trait Processor<const BUF_SIZE: usize>: Send + Sync {
    /// Обработать один блок аудио.
    fn process(
        &mut self,
        inputs: &[&[f32; BUF_SIZE]],
        outputs: &mut [&mut [f32; BUF_SIZE]],
    ) -> Result<(), AudioError>;

    /// Количество портов заданного типа.
    fn num_ports(&self, port_type: PortType) -> usize;

    /// Получение значения параметра порта.
    fn get_port_param(&self, port: PortId, param: &ParameterId) -> Option<ParamValue>;

    /// Установка значения параметра порта.
    fn set_port_param(
        &mut self,
        port: PortId,
        param: &ParameterId,
        value: ParamValue,
    ) -> Result<(), AudioError>;

    /// Инициализация процессора с частотой дискретизации.
    fn init(&mut self, sample_rate: f32);

    /// Сброс внутреннего состояния.
    fn reset(&mut self);
}