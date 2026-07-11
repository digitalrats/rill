#![allow(missing_docs)]

pub mod delay;
pub mod distortion;
pub mod limiter;
pub mod register;

pub(crate) fn pv_f32(value: &rill_core::traits::ParamValue) -> f32 {
    match value {
        rill_core::traits::ParamValue::Float(f) => *f,
        rill_core::traits::ParamValue::Int(i) => *i as f32,
        _ => 0.0,
    }
}
