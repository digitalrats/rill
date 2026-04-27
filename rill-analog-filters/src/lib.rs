//! Analog filters using WDF (Wave Digital Filter) modeling.
//!
//! Wraps [`rill_core_wdf::filters::MoogLadder`] for use as a graph node.

#![deny(unsafe_code)]

mod nodes;

pub use nodes::WdfMoogLadderProcessor;
