pub mod probe;
pub mod collector;

pub mod prelude {
    pub use crate::probe::TelemetryProbe;
    pub use crate::collector::TelemetryCollector;
}
