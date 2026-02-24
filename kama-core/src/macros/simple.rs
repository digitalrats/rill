//! Макросы для создания простых тестовых узлов (без DSP).

/// Макрос для создания простого узла с f32 (только для тестов)
#[macro_export]
macro_rules! simple_node_f32 {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident {
            params {
                $(
                    $(#[$param_meta:meta])*
                    $param_name:ident: f32 = $param_default:expr
                ),* $(,)?
            }
        }
        
        ports {
            audio_in: $audio_in:expr,
            audio_out: $audio_out:expr,
        }
        
        process_fn = $process:expr
    ) => {
        $(#[$struct_meta])*
        $vis struct $name {
            $(pub $param_name: f32),*,
            sample_rate: f32,
        }

        impl $name {
            pub fn new($($param_name: f32),*) -> Self {
                Self {
                    $($param_name),*,
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
                if inputs.is_empty() || outputs.is_empty() {
                    return Ok(());
                }

                let input = inputs[0];
                let output = &mut outputs[0];
                let len = input.len().min(output.len());

                for i in 0..len {
                    output[i] = $process(self, input[i]);
                }

                Ok(())
            }

            fn num_ports(&self, port_type: $crate::traits::PortType) -> usize {
                match port_type {
                    $crate::traits::PortType::AudioIn => $audio_in,
                    $crate::traits::PortType::AudioOut => $audio_out,
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
                        stringify!($param_name) => {
                            Some($crate::traits::ParamValue::Float(self.$param_name))
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
                        (stringify!($param_name), $crate::traits::ParamValue::Float(v)) => {
                            self.$param_name = v;
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

            fn reset(&mut self) {}

            fn node_type_id(&self) -> $crate::traits::NodeTypeId {
                $crate::traits::NodeTypeId::of::<Self>()
            }

            fn metadata(&self) -> $crate::traits::NodeMetadata {
                let params = vec![
                    $(
                        $crate::traits::ParamMetadata {
                            name: stringify!($param_name).to_string(),
                            description: stringify!($param_name).to_string(),
                            typ: $crate::traits::ParamType::Float,
                            default: $crate::traits::ParamValue::Float($param_default),
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
    };
}

/// Тривиальный узел без параметров
#[macro_export]
macro_rules! trivial_node_f32 {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident
        ports {
            audio_in: $audio_in:expr,
            audio_out: $audio_out:expr,
        }
        process_fn = $process:expr
    ) => {
        $(#[$struct_meta])*
        $vis struct $name {
            sample_rate: f32,
        }

        impl $name {
            pub fn new() -> Self {
                Self { sample_rate: 44100.0 }
            }
        }

        impl $crate::traits::AudioNode for $name {
            fn process(
                &mut self,
                inputs: &[&[f32]],
                outputs: &mut [&mut [f32]],
            ) -> Result<(), $crate::traits::AudioError> {
                if inputs.is_empty() || outputs.is_empty() {
                    return Ok(());
                }

                let input = inputs[0];
                let output = &mut outputs[0];
                let len = input.len().min(output.len());

                for i in 0..len {
                    output[i] = $process(self, input[i]);
                }

                Ok(())
            }

            fn num_ports(&self, port_type: $crate::traits::PortType) -> usize {
                match port_type {
                    $crate::traits::PortType::AudioIn => $audio_in,
                    $crate::traits::PortType::AudioOut => $audio_out,
                    $crate::traits::PortType::Node => 1,
                    _ => 0,
                }
            }

            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
            }

            fn reset(&mut self) {}

            fn node_type_id(&self) -> $crate::traits::NodeTypeId {
                $crate::traits::NodeTypeId::of::<Self>()
            }

            fn metadata(&self) -> $crate::traits::NodeMetadata {
                $crate::traits::NodeMetadata {
                    name: stringify!($name).to_string(),
                    category: $crate::traits::NodeCategory::Effect,
                    description: stringify!($name).to_string(),
                    author: "Kama Audio".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    parameters: vec![],
                }
            }
        }
    };
}