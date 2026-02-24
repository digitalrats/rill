/// Макрос для создания простого источника (только для тестов)
#[macro_export]
macro_rules! source_node_f32 {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field_name:ident: f32 = $field_default:expr
            ),* $(,)?
        }
        
        ports {
            audio_out: $num_outputs:expr,
        }
        
        generate_fn = $generate:expr
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $($(#[$field_meta])* $field_vis $field_name: f32),*,
            phase: f32,
            sample_rate: f32,
        }

        impl $name {
            pub fn new($($field_name: f32),*) -> Self {
                Self {
                    $($field_name),*,
                    phase: 0.0,
                    sample_rate: 44100.0,
                }
            }
        }

        impl $crate::traits::AudioNode for $name {
            fn process(
                &mut self,
                _inputs: &[&[f32]],
                outputs: &mut [&mut [f32]],
            ) -> Result<(), $crate::traits::AudioError> {
                let out_channels = outputs.len().min($num_outputs);
                if out_channels == 0 {
                    return Ok(());
                }

                let buffer_size = outputs[0].len();
                let generate_fn: fn(&mut Self) -> f32 = $generate;

                for i in 0..buffer_size {
                    let sample = generate_fn(self);
                    for ch in 0..out_channels {
                        outputs[ch][i] = sample;
                    }
                }

                Ok(())
            }

            fn num_ports(&self, port_type: $crate::traits::PortType) -> usize {
                match port_type {
                    $crate::traits::PortType::AudioOut => $num_outputs,
                    $crate::traits::PortType::Node => 1,
                    _ => 0,
                }
            }

            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
                self.phase = 0.0;
            }

            fn reset(&mut self) {
                self.phase = 0.0;
            }

            fn node_type_id(&self) -> $crate::traits::NodeTypeId {
                $crate::traits::NodeTypeId::of::<Self>()
            }

            fn metadata(&self) -> $crate::traits::NodeMetadata {
                $crate::traits::NodeMetadata {
                    name: stringify!($name).to_string(),
                    category: $crate::traits::NodeCategory::Generator,
                    description: stringify!($name).to_string(),
                    author: "Kama Audio".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    parameters: vec![],
                }
            }
        }

        impl $crate::traits::Source for $name {
            fn phase(&self) -> f32 { self.phase }
            fn reset_phase(&mut self) { self.phase = 0.0; }
        }
    };
}