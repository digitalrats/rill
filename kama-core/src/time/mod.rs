//! Модуль времени – единый источник тактовой информации для всей системы.

mod tick_info;
mod clock;
mod provider;
mod system_clock;

pub use tick_info::TickInfo;
pub use clock::Clock;
pub use provider::TimeProvider;
pub use system_clock::SystemClock;