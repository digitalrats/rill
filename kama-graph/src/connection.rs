use kama_core_traits::PortId;

/// Соединение между портами узлов
#[derive(Debug, Clone)]
pub struct Connection {
    /// Выходной порт источника
    pub from: PortId,
    /// Входной порт назначения
    pub to: PortId,
    /// Коэффициент усиления (для микширования)
    pub gain: f32,
}

impl Connection {
    /// Создать новое соединение
    pub fn new(from: PortId, to: PortId, gain: f32) -> Self {
        Self { from, to, gain }
    }

    /// Проверить валидность соединения
    pub fn is_valid(&self) -> bool {
        self.from.is_output() && self.to.is_input()
    }
}
