pub mod collector;
pub mod probe;

pub mod prelude {
    pub use crate::collector::TelemetryCollector;
    pub use crate::probe::TelemetryProbe;
}
