//! Macro for creating Sink nodes (audio outputs)
//!
//! # Examples
//! ```
//! use kama_core::prelude::*;
//! use kama_core::macros::sink_node;
//! use kama_core::DEFAULT_BLOCK_SIZE;
//!
//! sink_node! {
//!     /// File writer with gain control
//!     #[derive(Debug)]
//!     pub struct FileSink {
//!         params: {
//!             /// Output gain
//!             gain: f32 = 1.0,
//!         },
//!         control_inputs: {
//!             /// Volume automation (0.0 to 1.0)
//!             volume: f32 = 1.0,
//!         },
//!         state: {
//!             samples_written: u64 = 0,
//!         },
//!         inputs: 1,
//!         sink: |this, _channel, input, control| {
//!             let volume = control[0];
//!             for &sample in input {
//!                 let _ = sample * this.gain * volume;
//!                 this.samples_written += 1;
//!             }
//!         }
//!     }
//! }
//! ```
#[macro_export]
macro_rules! sink_node {
    // Generic version with explicit audio type
    (
        <$type:ty>
        $(#[$struct_meta:meta])*
        $vis:vis struct $name:ident {
            params: {
                $(
                    $(#[$param_meta:meta])*
                    $param_name:ident : $param_type:ty = $param_default:expr
                ),* $(,)?
            },
            $(control_inputs: {
                $(
                    $(#[$control_meta:meta])*
                    $control_name:ident : $control_type:ty = $control_default:expr
                ),* $(,)?
            },)?
            state: {
                $(
                    $(#[$state_meta:meta])*
                    $state_name:ident : $state_type:ty = $state_default:expr
                ),* $(,)?
            },
            inputs: $num_inputs:expr,
            sink: $sink:expr
        }
    ) => {
        $(#[$struct_meta])*
        $vis struct $name {
            $(
                $(#[$param_meta])*
                pub $param_name: $param_type,
            )*
            
            $(
                $(
                    $(#[$control_meta])*
                    pub $control_name: $control_type,
                )*
            )?
            
            $(
                $(#[$state_meta])*
                pub $state_name: $state_type,
            )*
            
            /// Sample rate
            pub sample_rate: f32,
            
            /// Control input values (updated from graph)
            pub control_values: Vec<f32>,
            
            /// Parameter IDs for automation
            pub param_ids: std::collections::HashMap<String, $crate::traits::ParameterId>,
        }

        impl $name {
            /// Create a new instance
            pub fn new($($param_name: $param_type),* $(, $($control_name: $control_type),*)?) -> Self {
                let mut param_ids = std::collections::HashMap::new();
                $(
                    if let Ok(id) = $crate::traits::ParameterId::new(stringify!($param_name)) {
                        param_ids.insert(stringify!($param_name).to_string(), id);
                    }
                )*
                
                // Count control inputs using array length - simpler and works
                let control_count = 0 $(+ { let _ = ($($control_name,)*); 1 })?;
                
                Self {
                    $($param_name),*,
                    $($($control_name: $control_default),*)?,
                    $($state_name: $state_default),*,
                    sample_rate: 44100.0,
                    control_values: vec![0.0; control_count],
                    param_ids,
                }
            }
            
            /// Get parameter ID by name
            pub fn param_id(&self, name: &str) -> Option<&$crate::traits::ParameterId> {
                self.param_ids.get(name)
            }
            
            /// Update control input value
            pub fn set_control(&mut self, index: usize, value: f32) {
                if index < self.control_values.len() {
                    self.control_values[index] = value;
                }
            }
        }

        impl $crate::traits::Sink<$type, { $crate::DEFAULT_BLOCK_SIZE }> for $name
        where
            $type: $crate::math::AudioNum + Send + Sync,
        {
            fn process(
                &mut self,
                inputs: &[&[$type; { $crate::DEFAULT_BLOCK_SIZE }]],
                control: &[f32],
            ) -> $crate::ProcessResult<()> {
                let num_in = inputs.len().min($num_inputs);
                
                if num_in == 0 {
                    return Ok(());
                }

                let sink_fn: fn(
                    &mut Self,
                    usize,
                    &[$type; { $crate::DEFAULT_BLOCK_SIZE }],
                    &[f32]
                ) = $sink;

                for ch in 0..num_in {
                    sink_fn(self, ch, inputs[ch], control);
                }

                Ok(())
            }

            fn num_audio_inputs(&self) -> usize {
                $num_inputs
            }

            fn num_control_inputs(&self) -> usize {
                self.control_values.len()
            }

            fn get_parameter(&self, id: &$crate::traits::ParameterId) -> Option<$crate::traits::ParamValue> {
                match id.as_str() {
                    $(
                        stringify!($param_name) => {
                            Some($crate::traits::ParamValue::Float(
                                self.$param_name
                            ))
                        }
                    )*
                    _ => None,
                }
            }

            fn set_parameter(
                &mut self,
                id: &$crate::traits::ParameterId,
                value: $crate::traits::ParamValue,
            ) -> $crate::ProcessResult<()> {
                match (id.as_str(), value) {
                    $(
                        (stringify!($param_name), $crate::traits::ParamValue::Float(v)) => {
                            self.$param_name = v;
                            Ok(())
                        }
                    )*
                    _ => Err($crate::ProcessError::Parameter(
                        format!("Unknown parameter: {}", id.as_str())
                    )),
                }
            }

            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
            }

            fn reset(&mut self) {
                $(
                    self.$state_name = $state_default;
                )*
                for val in &mut self.control_values {
                    *val = 0.0;
                }
            }
        }
    };
    // Backward-compatible version (defaults to f32)
    (
        $(#[$struct_meta:meta])*
        $vis:vis struct $name:ident {
            $($rest:tt)*
        }
    ) => {
        $crate::sink_node! {
            <f32>
            $(#[$struct_meta])*
            $vis struct $name {
                $($rest)*
            }
        }
    };
}

/// Simplified version for f32 sinks
#[macro_export]
macro_rules! sink_node_f32 {
    (
        $(#[$struct_meta:meta])*
        $vis:vis struct $name:ident $($rest:tt)*
    ) => {
        $crate::sink_node! {
            $(#[$struct_meta])*
            $vis struct $name $($rest)*
        }
    };
}