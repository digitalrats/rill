pub mod biquad;
pub mod moog;
pub mod noise;
pub mod onepole;
pub mod osc;

pub use biquad::BiquadBuiltin;
pub use moog::MoogBuiltin;
pub use noise::NoiseGenBuiltin;
pub use onepole::OnePoleBuiltin;
pub use osc::OscBuiltin;

pub(crate) fn pv_f32(value: &rill_core::traits::ParamValue) -> f32 {
    match value {
        rill_core::traits::ParamValue::Float(f) => *f,
        rill_core::traits::ParamValue::Int(i) => *i as f32,
        _ => 0.0,
    }
}
