//! Продвинутый микшер и система маршрутизации для Kama Audio
//! 
//! Этот крейт расширяет базовый микшер из `kama-core`, добавляя:
//! - Реактивную систему событий (feature "reactive")
//! - Сложные фильтры (feature "complex-filters")
//! - Шины и маршрутизацию (feature "buses")

#![warn(missing_docs)]

pub mod events;
pub mod filters;

#[cfg(feature = "buses")]
pub mod buses;
#[cfg(feature = "reactive")]
pub mod reactive;

// Реэкспорт базовых типов из kama-core для удобства
pub use kama_core::mixer::{
    BasicMixer, MixerConfig, ChannelConfig, ChannelType,
    MasterConfig, MixerFactory,
};

// Реэкспорт наших расширений (доступных всегда)
pub use events::{MixerEvent, MixerEventSystem};
pub use filters::{Bitcrusher, FilterChain, FilterConfig, FilterType, FilterParams};

// Реэкспорт условных модулей
#[cfg(feature = "buses")]
pub use buses::{BusingMixer, BusConfig, SendConfig, SendType};
#[cfg(feature = "reactive")]
pub use reactive::{ReactiveMixer, ParameterUpdate};

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_bitcrusher() {
        let mut crusher = Bitcrusher::new(8, 0.5);
        let input = 0.75;
        let output = crusher.process(input);
        
        assert_ne!(input, output);
        assert!(output.abs() <= 1.0);
    }
}