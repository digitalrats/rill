//! Signal-rate oscillators (20Hz - 20kHz) with Transcendental support

mod saw;
mod sine;
// mod square;
// mod triangle;
mod noise;
mod wavetable;
// mod fm;

pub use saw::SawOsc;
pub use sine::SineOsc;
// pub use square::SquareOsc;
// pub use triangle::TriangleOsc;
pub use noise::{NoiseOsc, NoiseType};
pub use wavetable::WavetableOscNode;
// pub use fm::FmOsc;

// Re-export core types
pub use rill_core::Transcendental;
