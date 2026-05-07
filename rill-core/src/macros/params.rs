//! # Helper macros for working with parameters

// These macros are no longer needed, but can be kept for compatibility or removed.
// Keep only __init_ports, which is used.

/// Port initialization (kept)
#[macro_export]
#[doc(hidden)]
macro_rules! __init_ports {
    // For Source (outputs only)
    (ports { signal_out: $out:expr $(,)? }, $node:expr, $outputs:ident) => {
        for i in 0..$out {
            let port = $crate::Port::output($node.id, i as u16, &format!("out_{}", i));
            $node.$outputs.push(port);
        }
    };

    // For Sink (inputs only)
    (ports { signal_in: $in:expr $(,)? }, $node:expr, $inputs:ident) => {
        for i in 0..$in {
            let port = $crate::Port::input($node.id, i as u16, &format!("in_{}", i));
            $node.$inputs.push(port);
        }
    };

    // For Processor (inputs and outputs)
    (ports { signal_in: $in:expr, signal_out: $out:expr $(,)? }, $node:expr, $inputs:ident, $outputs:ident, $controls:ident) => {
        for i in 0..$in {
            let port = $crate::Port::input($node.id, i as u16, &format!("in_{}", i));
            $node.$inputs.push(port);
        }

        for i in 0..$out {
            let port = $crate::Port::output($node.id, i as u16, &format!("out_{}", i));
            $node.$outputs.push(port);
        }
    };

    // With control ports
    (ports { signal_in: $in:expr, signal_out: $out:expr, control: $ctrl:expr }, $node:expr, $inputs:ident, $outputs:ident, $controls:ident) => {
        $crate::__init_ports!(
            ports {
                signal_in: $in,
                signal_out: $out
            },
            $node,
            $inputs,
            $outputs,
            $controls
        );

        for i in 0..$ctrl {
            let port = $crate::Port::control_in($node.id, i as u16, &format!("ctrl_{}", i));
            $node.$controls.push(port);
        }
    };
}

// ============================================================================
// Conversions from primitive types to ParamValue
// ============================================================================
use crate::ParamValue;

impl From<f32> for ParamValue {
    fn from(value: f32) -> Self {
        ParamValue::Float(value)
    }
}

impl From<i32> for ParamValue {
    fn from(value: i32) -> Self {
        ParamValue::Int(value)
    }
}

impl From<bool> for ParamValue {
    fn from(value: bool) -> Self {
        ParamValue::Bool(value)
    }
}

impl From<String> for ParamValue {
    fn from(value: String) -> Self {
        ParamValue::String(value)
    }
}

impl From<&str> for ParamValue {
    fn from(value: &str) -> Self {
        ParamValue::String(value.to_string())
    }
}
