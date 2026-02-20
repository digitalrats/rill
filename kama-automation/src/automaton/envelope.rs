// kama-automation/src/automaton/envelope.rs

/// Состояние огибающей
#[derive(Debug, Clone, Default)]
pub struct EnvelopeState {
    pub stage: EnvelopeStage,
    pub value: f64,
    pub samples_elapsed: usize,
}

/// Стадия огибающей
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvelopeStage {
    Attack,
    Decay,
    Sustain,
    Release,
    Off,
}

impl Default for EnvelopeStage {
    fn default() -> Self {
        EnvelopeStage::Off
    }
}