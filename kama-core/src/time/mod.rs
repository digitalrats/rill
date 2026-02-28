//! Time and clock abstractions for real-time audio
//!
//! This module provides:
//! - `Clock`: Basic time source
//! - `TimeProvider`: Extended time with tempo information
//! - `SystemClock`: High-precision system time implementation
//! - `TickInfo`: Musical timing information

mod clock;
mod provider;
mod system_clock;
mod tick;

pub use clock::Clock;
pub use provider::TimeProvider;
pub use system_clock::SystemClock;
pub use tick::TickInfo;

/// Prelude for time module
pub mod prelude {
    pub use super::{Clock, TimeProvider, SystemClock, TickInfo};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_exports() {
        let _clock: Box<dyn Clock> = Box::new(SystemClock::new(44100.0, 120.0));
        let _provider: Box<dyn TimeProvider> = Box::new(SystemClock::new(44100.0, 120.0));
        let _tick = TickInfo::new(0, 0, 0, 0);
    }
}