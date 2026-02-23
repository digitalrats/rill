// kama-lofi/src/node_params.rs
use kama_core::traits::{ParamMetadata, ParamValue, ParamType};

pub fn bit_depth_param(default: u8) -> ParamMetadata {
    ParamMetadata {
        name: "bit_depth".to_string(),
        typ: ParamType::Int,
        default: ParamValue::Int(default as i32),
        min: Some(1.0),
        max: Some(16.0),
        step: Some(1.0),
        unit: Some("bits".to_string()),
        choices: Some(vec![
            ("8-bit".to_string(), 8.0),
            ("12-bit".to_string(), 12.0),
            ("16-bit".to_string(), 16.0),
        ]),
    }
}

pub fn sample_rate_param(default: f32) -> ParamMetadata {
    ParamMetadata {
        name: "sample_rate".to_string(),
        typ: ParamType::Float,
        default: ParamValue::Float(default),
        min: Some(8000.0),
        max: Some(48000.0),
        step: Some(100.0),
        unit: Some("Hz".to_string()),
        choices: Some(vec![
            ("8kHz".to_string(), 8000.0),
            ("16kHz".to_string(), 16000.0),
            ("32kHz".to_string(), 32000.0),
            ("44.1kHz".to_string(), 44100.0),
        ]),
    }
}

pub fn dry_wet_param(default: f32) -> ParamMetadata {
    ParamMetadata {
        name: "dry_wet".to_string(),
        typ: ParamType::Float,
        default: ParamValue::Float(default),
        min: Some(0.0),
        max: Some(1.0),
        step: Some(0.01),
        unit: Some("mix".to_string()),
        choices: None,
    }
}

pub fn output_gain_param(default: f32) -> ParamMetadata {
    ParamMetadata {
        name: "output_gain".to_string(),
        typ: ParamType::Float,
        default: ParamValue::Float(default),
        min: Some(0.0),
        max: Some(4.0),
        step: Some(0.01),
        unit: Some("linear".to_string()),
        choices: None,
    }
}

pub fn enable_bitcrush_param(default: bool) -> ParamMetadata {
    ParamMetadata {
        name: "enable_bitcrush".to_string(),
        typ: ParamType::Bool,
        default: ParamValue::Bool(default),
        min: None,
        max: None,
        step: None,
        unit: None,
        choices: None,
    }
}

pub fn enable_sr_reduction_param(default: bool) -> ParamMetadata {
    ParamMetadata {
        name: "enable_sr_reduction".to_string(),
        typ: ParamType::Bool,
        default: ParamValue::Bool(default),
        min: None,
        max: None,
        step: None,
        unit: None,
        choices: None,
    }
}