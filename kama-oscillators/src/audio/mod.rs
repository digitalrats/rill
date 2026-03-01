//! Audio-rate oscillators (20Hz - 20kHz) with AudioNum support

mod sine;
mod saw;
// mod square;
// mod triangle;
mod noise;
// mod fm;

pub use sine::SineOsc;
pub use saw::SawOsc;
// pub use square::SquareOsc;
// pub use triangle::TriangleOsc;
pub use noise::{NoiseOsc, NoiseType};
// pub use fm::FmOsc;

// Re-export core types
pub use kama_core::AudioNum;