//! Macro for creating Processor nodes
//!
//! # Examples
//! ```
//! use kama_core::prelude::*;
//! use kama_core::macros::processor_node;
//! use kama_core::DEFAULT_BLOCK_SIZE;
//!
//! processor_node! {
//!     /// Simple VCF filter with cutoff modulation
//!     #[derive(Debug)]
//!     pub struct VcfFilter {
//!         params: {
//!             /// Base cutoff frequency
//!             cutoff: f32 = 1000.0,
//!             
//!             /// Resonance
//!             resonance: f32 = 0.7,
//!         },
//!         control_inputs: {
//!             /// Cutoff modulation (0.0 to 1.0 normalized)
//!             cutoff_mod: f32 = 0.0,
//!         },
//!         state: {
//!             last_output: f32 = 0.0,
//!         },
//!         inputs: 1,
//!         outputs: 1,
//!         process: |this, _channel, input, output, control| {
//!             let modulated_cutoff = this.cutoff * (1.0 + control[0]);
//!             // Filter implementation...
//!             for i in 0..DEFAULT_BLOCK_SIZE {
//!                 output[i] = input[i]; // Simplified
//!                 this.last_output = output[i];
//!             }
//!         }
//!     }
//! }
//! ```
#[macro_export]
macro_rules! processor_node {
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
            inputs: $num_inputs:expr,
            outputs: $num_outputs:expr,
            process: $process:expr
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

        impl $crate::traits::Processor<f32, { $crate::DEFAULT_BLOCK_SIZE }> for $name {
            fn process(
                &mut self,
                inputs: &[&[f32; { $crate::DEFAULT_BLOCK_SIZE }]],
                outputs: &mut [&mut [f32; { $crate::DEFAULT_BLOCK_SIZE }]],
                control: &[f32],
            ) -> $crate::ProcessResult<()> {
                let num_in = inputs.len().min($num_inputs);
                let num_out = outputs.len().min($num_outputs);
                
                if num_in == 0 || num_out == 0 {
                    return Ok(());
                }

                let process_fn: fn(
                    &mut Self,
                    usize,
                    &[f32; { $crate::DEFAULT_BLOCK_SIZE }],
                    &mut [f32; { $crate::DEFAULT_BLOCK_SIZE }],
                    &[f32]
                ) = $process;

                for ch in 0..num_out.min(num_in) {
                    process_fn(self, ch, inputs[ch], outputs[ch], control);
                }

                Ok(())
            }

            fn num_audio_inputs(&self) -> usize {
                $num_inputs
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

/// Simplified version for f32 processors
#[macro_export]
macro_rules! processor_node_f32 {
    (
        $(#[$struct_meta:meta])*
        $vis:vis struct $name:ident $($rest:tt)*
    ) => {
        $crate::processor_node! {
            $(#[$struct_meta])*
            $vis struct $name $($rest)*
        }
    };
}