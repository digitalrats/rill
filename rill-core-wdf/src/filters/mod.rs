//! WDF-based filters.
//!
//! These filters use Wave Digital Filter elements and adapters to model
//! analog circuits, providing authentic analog behavior.

mod moog_ladder;
mod diode_clipper;

pub use moog_ladder::{MoogLadder, RcPole};
pub use diode_clipper::DiodeClipper;

