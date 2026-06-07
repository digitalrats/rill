//! Analog filters using WDF (Wave Digital Filter) modeling.
//!
//! Wraps [`rill_core_model::wdf::MoogLadder`] for use as a graph node.

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod nodes;

pub use nodes::WdfMoogLadderProcessor;
