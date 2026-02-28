//! Macro for creating Source nodes (generators)
//!
//! # Examples
//! ```
//! use kama_core::prelude::*;
//! use kama_core::macros::source_node;
//! use kama_core::DEFAULT_BLOCK_SIZE;
//!
//! source_node! {
//!     /// Simple sine oscillator with control input
//!     #[derive(Debug)]
//!     pub struct SineOsc {
//!         params: {
//!             /// Base frequency in Hz
//!             frequency: f32 = 440.0,
//!             
//!             /// Output amplitude
//!             amplitude: f32 = 0.5,
//!         },
//!         control_inputs: {
//!             /// Frequency modulation input (0.0 to 1.0 normalized)
//!             fm: f32 = 0.0,
//!         },
//!         state: {
//!             phase: f32 = 0.0,
//!         },
//!         outputs: 1,
//!         generate: |this, _channel, output, control| {
//!             let base_freq = this.frequency;
//!             let fm_amount = control[0] * 200.0;
//!             let effective_freq = base_freq + fm_amount;
//!             
//!             let phase_inc = effective_freq / this.sample_rate;
//!             
//!             for i in 0..DEFAULT_BLOCK_SIZE {
//!                 output[i] = (this.phase * 2.0 * std::f32::consts::PI).sin() * this.amplitude;
//!                 this.phase = (this.phase + phase_inc) % 1.0;
//!             }
//!         }
//!     }
//! }
//! ```
#[macro_export]
macro_rules! source_node {
    (
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
            outputs: $num_outputs:expr,
            generate: $generate:expr
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

        impl $crate::traits::Source<f32, { $crate::DEFAULT_BLOCK_SIZE }> for $name {
            fn generate(
                &mut self,
                outputs: &mut [&mut [f32; { $crate::DEFAULT_BLOCK_SIZE }]],
                control: &[f32],
            ) -> $crate::ProcessResult<()> {
                if outputs.is_empty() {
                    return Ok(());
                }

                let generate_fn: fn(
                    &mut Self,
                    usize,
                    &mut [f32; { $crate::DEFAULT_BLOCK_SIZE }],
                    &[f32]
                ) = $generate;

                for (channel, output) in outputs.iter_mut().enumerate() {
                    if channel < $num_outputs {
                        generate_fn(self, channel, output, control);
                    }
                }

                Ok(())
            }

            fn num_audio_outputs(&self) -> usize {
                $num_outputs
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
}

/// Simplified version for f32 sources
#[macro_export]
macro_rules! source_node_f32 {
    (
        $(#[$struct_meta:meta])*
        $vis:vis struct $name:ident $($rest:tt)*
    ) => {
        $crate::source_node! {
            $(#[$struct_meta])*
            $vis struct $name $($rest)*
        }
    };
}