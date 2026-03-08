//! # Вспомогательные макросы для работы с параметрами

// Эти макросы больше не нужны, но можно оставить для совместимости или удалить.
// Оставляем только __init_ports, который используется.

/// Инициализация портов (оставляем)
#[macro_export]
#[doc(hidden)]
macro_rules! __init_ports {
    // Для Source (только выходы)
    (ports { audio_out: $out:expr $(,)? }, $node:expr, $outputs:ident) => {
        for i in 0..$out {
            let port = $crate::Port::output(
                $node.id,
                i as u16,
                &format!("out_{}", i)
            );
            $node.$outputs.push(port);
        }
    };
    
    // Для Sink (только входы)
    (ports { audio_in: $in:expr $(,)? }, $node:expr, $inputs:ident) => {
        for i in 0..$in {
            let port = $crate::Port::input(
                $node.id,
                i as u16,
                &format!("in_{}", i)
            );
            $node.$inputs.push(port);
        }
    };
    
    // Для Processor (входы и выходы)
    (ports { audio_in: $in:expr, audio_out: $out:expr $(,)? }, $node:expr, $inputs:ident, $outputs:ident, $controls:ident) => {
        for i in 0..$in {
            let port = $crate::Port::input(
                $node.id,
                i as u16,
                &format!("in_{}", i)
            );
            $node.$inputs.push(port);
        }
        
        for i in 0..$out {
            let port = $crate::Port::output(
                $node.id,
                i as u16,
                &format!("out_{}", i)
            );
            $node.$outputs.push(port);
        }
    };
    
    // С управляющими портами
    (ports { audio_in: $in:expr, audio_out: $out:expr, control: $ctrl:expr }, $node:expr, $inputs:ident, $outputs:ident, $controls:ident) => {
        $crate::__init_ports!(
            ports { audio_in: $in, audio_out: $out },
            $node,
            $inputs,
            $outputs,
            $controls
        );
        
        for i in 0..$ctrl {
            let port = $crate::Port::control_in(
                $node.id,
                i as u16,
                &format!("ctrl_{}", i)
            );
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