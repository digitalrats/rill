// Re-export базовых трейтов
pub use kama_core_traits::time::{
    Clock,
    TimeProvider,
    TickInfo,
};

mod system_clock;
pub use system_clock::SystemClock;