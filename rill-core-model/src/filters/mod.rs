//! WDF-based filters.
//!
//! These filters use Wave Digital Filter elements and adapters to model
//! analog circuits, providing authentic analog behavior.

mod diode_clipper;
mod moog_ladder;

pub use diode_clipper::DiodeClipper;
pub use moog_ladder::{MoogLadder, RcPole};
