/// Макрос для создания простого процессора (без DSP, для тестов)
#[macro_export]
macro_rules! processor_node_f32 {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident {
            // Секция параметров (может быть пустой)
            params {
                $(
                    $(#[$field_meta:meta])*
                    $field_vis:vis $field_name:ident: f32 = $field_default:expr
                ),* $(,)?
            }
            
            // Секция состояния (обязательная)
            state {
                $state_vis:vis $state_name:ident: $state_type:ty = $state_default:expr
            }
        }
        
        ports {
            audio_in: $num_inputs:expr,
            audio_out: $num_outputs:expr,
        }
        
        process_fn: $process:expr,
        reset_fn: $reset:expr,
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $($(#[$field_meta])* $field_vis $field_name: f32),*,
            $state_vis $state_name: $state_type,
            sample_rate: f32,
        }

        impl $name {
            pub fn new($($field_name: f32),*) -> Self {
                Self {
                    $($field_name),*,
                    $state_name: $state_default,
                    sample_rate: 44100.0,
                }
            }
        }

        impl $crate::traits::AudioNode for $name {
            fn process(
                &mut self,
                inputs: &[&[f32]],
                outputs: &mut [&mut [f32]],
            ) -> Result<(), $crate::traits::AudioError> {
                let in_channels = inputs.len().min($num_inputs);
                let out_channels = outputs.len().min($num_outputs);
                
                if in_channels == 0 || out_channels == 0 {
                    return Ok(());
                }

                let buffer_size = outputs[0].len();
                let process_fn: fn(&mut Self, &[f32], &mut [f32]) = $process;

                for ch in 0..out_channels {
                    if ch < in_channels {
                        process_fn(self, inputs[ch], &mut outputs[ch][..buffer_size]);
                    } else {
                        outputs[ch][..buffer_size].fill(0.0);
                    }
                }

                Ok(())
            }

            fn num_ports(&self, port_type: $crate::traits::PortType) -> usize {
                match port_type {
                    $crate::traits::PortType::AudioIn => $num_inputs,
                    $crate::traits::PortType::AudioOut => $num_outputs,
                    $crate::traits::PortType::Node => 1,
                    _ => 0,
                }
            }

            fn get_port_param(
                &self,
                port: $crate::traits::PortId,
                param: $crate::traits::ParameterId,
            ) -> Option<$crate::traits::ParamValue> {
                if port.port_type() != $crate::traits::PortType::Node {
                    return None;
                }

                match param.as_str() {
                    $(
                        stringify!($field_name) => {
                            Some($crate::traits::ParamValue::Float(self.$field_name))
                        }
                    )*
                    _ => None,
                }
            }

            fn set_port_param(
                &mut self,
                port: $crate::traits::PortId,
                param: $crate::traits::ParameterId,
                value: $crate::traits::ParamValue,
            ) -> Result<(), $crate::traits::AudioError> {
                if port.port_type() != $crate::traits::PortType::Node {
                    return Err($crate::traits::AudioError::Parameter(
                        "Parameters only supported on Node port".into()
                    ));
                }

                match (param.as_str(), value) {
                    $(
                        (stringify!($field_name), $crate::traits::ParamValue::Float(v)) => {
                            self.$field_name = v;
                            Ok(())
                        }
                    )*
                    _ => Err($crate::traits::AudioError::Parameter(
                        format!("Unknown parameter: {}", param.as_str())
                    )),
                }
            }

            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
            }

            fn reset(&mut self) {
                let reset_fn: fn(&mut Self) = $reset;
                reset_fn(self);
            }

            fn node_type_id(&self) -> $crate::traits::NodeTypeId {
                $crate::traits::NodeTypeId::of::<Self>()
            }

            fn metadata(&self) -> $crate::traits::NodeMetadata {
                let params = vec![
                    $(
                        $crate::traits::ParamMetadata {
                            name: stringify!($field_name).to_string(),
                            description: stringify!($field_name).to_string(),
                            typ: $crate::traits::ParamType::Float,
                            default: $crate::traits::ParamValue::Float($field_default),
                            min: None,
                            max: None,
                            step: None,
                            unit: None,
                            choices: None,
                        }
                    ),*
                ];
                
                $crate::traits::NodeMetadata {
                    name: stringify!($name).to_string(),
                    category: $crate::traits::NodeCategory::Effect,
                    description: stringify!($name).to_string(),
                    author: "Kama Audio".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    parameters: params,
                }
            }
        }

        impl $crate::traits::Processor for $name {}
    };
}