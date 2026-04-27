//! WDF-based filters.
//!
//! These filters use Wave Digital Filter elements and adapters to model
//! analog circuits, providing authentic analog behavior.

mod moog_ladder;

pub use moog_ladder::MoogLadder;
