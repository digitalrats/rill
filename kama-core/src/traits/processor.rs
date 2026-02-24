/// Макрос для создания узла-процессора (M AudioIn, N AudioOut)
///
/// Порты:
/// - Node (1): общие параметры узла
/// - AudioIn (M): входные аудиосигналы
/// - AudioOut (N): выходные аудиосигналы
/// - Control (K): управляющие сигналы (модуляция, automation)
#[macro_export]
macro_rules! processor_node {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident {
            // Поля узла (соответствуют параметрам на Node порте)
            $($field_vis:vis $field_name:ident: $field_type:ty = $field_default:expr),* $(,)?
            
            // Внутренние буферы (опционально)
            $(buffers {
                $($buf_vis:vis $buf_name:ident: $buf_type:ty = $buf_init:expr),* $(,)?
            })?
            
            // Состояние (опционально)
            $(state: $state_type:ty = $state_default:expr)?
        }
        
        // Количество портов
        ports {
            audio_in: $audio_in:expr,
            audio_out: $audio_out:expr,
            $(control: $control:expr)?
        }
        
        // Параметры для каждого типа портов
        params {
            // Параметры на Node порте
            node {
                $($node_param_name:expr => $node_param_type:ident($node_default:expr) {
                    doc: $node_doc:expr,
                    min: $node_min:expr,
                    max: $node_max:expr,
                    step: $node_step:expr,
                    unit: $node_unit:expr,
                }),* $(,)?
            }
            
            // Параметры на AudioIn портах
            $(audio_in {
                $($in_port_pattern:tt => {
                    $($in_param_name:expr => $in_param_type:ident($in_default:expr) {
                        doc: $in_doc:expr,
                        min: $in_min:expr,
                        max: $in_max:expr,
                        step: $in_step:expr,
                        unit: $in_unit:expr,
                    }),* $(,)?
                }),* $(,)?
            })?
            
            // Параметры на AudioOut портах
            $(audio_out {
                $($out_port_pattern:tt => {
                    $($out_param_name:expr => $out_param_type:ident($out_default:expr) {
                        doc: $out_doc:expr,
                        min: $out_min:expr,
                        max: $out_max:expr,
                        step: $out_step:expr,
                        unit: $out_unit:expr,
                    }),* $(,)?
                }),* $(,)?
            })?
            
            // Параметры на Control портах
            $(control {
                $($ctrl_port_pattern:tt => {
                    $($ctrl_param_name:expr => $ctrl_param_type:ident($ctrl_default:expr) {
                        doc: $ctrl_doc:expr,
                        min: $ctrl_min:expr,
                        max: $ctrl_max:expr,
                        step: $ctrl_step:expr,
                        unit: $ctrl_unit:expr,
                    }),* $(,)?
                }),* $(,)?
            })?
        }
        
        // Функция обработки блока
        // Аргументы: &mut Self, channel: usize, input: &[f32], output: &mut [f32], control: &[f32]
        process_fn = $process:expr
    } => {
        $(#[$meta])*
        $vis struct $name {
            // Поля для параметров Node порта
            $($field_vis $field_name: $field_type),*,
            
            // Внутренние буферы
            $($($buf_vis $buf_name: $buf_type),*)?
            
            // Состояние
            $(state: $state_type,)?
            
            // Системные поля
            sample_rate: f32,
            
            // Хранилища для параметров портов
            audio_in_params: std::collections::HashMap<(usize, String), $crate::traits::ParamValue>,
            audio_out_params: std::collections::HashMap<(usize, String), $crate::traits::ParamValue>,
            control_params: std::collections::HashMap<(usize, String), $crate::traits::ParamValue>,
        }

        // Вспомогательные макросы для инициализации параметров
        #[doc(hidden)]
        #[macro_export]
        macro_rules! __processor_init_in_params {
            ($self:expr, [$($ports:tt)*]) => {
                $(
                    __processor_init_in_param_pattern!($ports, $self);
                )*
            };
        }

        #[doc(hidden)]
        #[macro_export]
        macro_rules! __processor_init_in_param_pattern {
            ("*", $self:expr, $($name:expr, $type:ident, $default:expr),*) => {
                for idx in 0..$audio_in {
                    $(
                        $self.audio_in_params.insert(
                            (idx, $name.to_string()),
                            $crate::traits::ParamValue::$type($default)
                        );
                    )*
                }
            };
            ([$idx:literal], $self:expr, $($name:expr, $type:ident, $default:expr),*) => {
                $(
                    $self.audio_in_params.insert(
                        ($idx, $name.to_string()),
                        $crate::traits::ParamValue::$type($default)
                    );
                )*
            };
            ([$start:literal..$end:literal], $self:expr, $($name:expr, $type:ident, $default:expr),*) => {
                for idx in $start..$end {
                    $(
                        $self.audio_in_params.insert(
                            (idx, $name.to_string()),
                            $crate::traits::ParamValue::$type($default)
                        );
                    )*
                }
            };
        }

        #[doc(hidden)]
        #[macro_export]
        macro_rules! __processor_init_out_params {
            ($self:expr, [$($ports:tt)*]) => {
                $(
                    __processor_init_out_param_pattern!($ports, $self);
                )*
            };
        }

        #[doc(hidden)]
        #[macro_export]
        macro_rules! __processor_init_out_param_pattern {
            ("*", $self:expr, $($name:expr, $type:ident, $default:expr),*) => {
                for idx in 0..$audio_out {
                    $(
                        $self.audio_out_params.insert(
                            (idx, $name.to_string()),
                            $crate::traits::ParamValue::$type($default)
                        );
                    )*
                }
            };
            ([$idx:literal], $self:expr, $($name:expr, $type:ident, $default:expr),*) => {
                $(
                    $self.audio_out_params.insert(
                        ($idx, $name.to_string()),
                        $crate::traits::ParamValue::$type($default)
                    );
                )*
            };
        }

        #[doc(hidden)]
        #[macro_export]
        macro_rules! __processor_init_control_params {
            ($self:expr, [$($ports:tt)*]) => {
                $(
                    __processor_init_control_param_pattern!($ports, $self);
                )*
            };
        }

        #[doc(hidden)]
        #[macro_export]
        macro_rules! __processor_init_control_param_pattern {
            ("*", $self:expr, $($name:expr, $type:ident, $default:expr),*) => {
                for idx in 0..$crate::__count_control!() {
                    $(
                        $self.control_params.insert(
                            (idx, $name.to_string()),
                            $crate::traits::ParamValue::$type($default)
                        );
                    )*
                }
            };
            ([$idx:literal], $self:expr, $($name:expr, $type:ident, $default:expr),*) => {
                $(
                    $self.control_params.insert(
                        ($idx, $name.to_string()),
                        $crate::traits::ParamValue::$type($default)
                    );
                )*
            };
        }

        impl $name {
            pub fn new($($field_name: $field_type),*) -> Self {
                let mut node = Self {
                    $($field_name),*,
                    $($($buf_name: $buf_init),*)?
                    $(state: $state_default,)?
                    sample_rate: 44100.0,
                    audio_in_params: std::collections::HashMap::new(),
                    audio_out_params: std::collections::HashMap::new(),
                    control_params: std::collections::HashMap::new(),
                };
                
                // Инициализация параметров значениями по умолчанию
                $(
                    __processor_init_in_params!(&mut node, [$($in_port_pattern)*]);
                )?
                
                $(
                    __processor_init_out_params!(&mut node, [$($out_port_pattern)*]);
                )?
                
                $(
                    __processor_init_control_params!(&mut node, [$($ctrl_port_pattern)*]);
                )?
                
                node
            }

            /// Получить параметр AudioIn порта
            pub fn audio_in_param(&self, index: usize, name: &str) -> Option<$crate::traits::ParamValue> {
                self.audio_in_params.get(&(index, name.to_string())).cloned()
            }

            /// Получить параметр AudioOut порта
            pub fn audio_out_param(&self, index: usize, name: &str) -> Option<$crate::traits::ParamValue> {
                self.audio_out_params.get(&(index, name.to_string())).cloned()
            }

            /// Получить параметр Control порта
            pub fn control_param(&self, index: usize, name: &str) -> Option<$crate::traits::ParamValue> {
                self.control_params.get(&(index, name.to_string())).cloned()
            }
        }

        impl $crate::traits::AudioNode for $name {
            fn process(
                &mut self,
                inputs: &[&[f32]],
                outputs: &mut [&mut [f32]],
            ) -> Result<(), $crate::traits::AudioError> {
                let in_channels = inputs.len().min($audio_in + $($control).unwrap_or(0));
                let out_channels = outputs.len().min($audio_out);
                
                if out_channels == 0 {
                    return Ok(());
                }

                // Разделяем входы: сначала AudioIn, потом Control
                let audio_in = &inputs[0..$audio_in.min(in_channels)];
                let mut next_idx = $audio_in;
                
                let control = if $($control > 0)? {
                    let ctrl = &inputs[next_idx..next_idx + $control.min(in_channels.saturating_sub($audio_in))];
                    next_idx += ctrl.len();
                    ctrl
                } else {
                    &[]
                };

                let buffer_size = outputs[0].len();
                let process_fn: fn(&mut Self, usize, &[f32], &mut [f32], &[f32]) = $process;

                // Для каждого выходного канала
                for ch in 0..out_channels.min($audio_out) {
                    if ch < audio_in.len() {
                        process_fn(
                            self,
                            ch,
                            audio_in[ch],
                            &mut outputs[ch][..buffer_size],
                            control,
                        );
                    } else {
                        outputs[ch][..buffer_size].fill(0.0);
                    }
                }

                Ok(())
            }

            fn num_ports(&self, port_type: $crate::traits::PortType) -> usize {
                match port_type {
                    $crate::traits::PortType::Node => 1,
                    $crate::traits::PortType::AudioIn => $audio_in,
                    $crate::traits::PortType::AudioOut => $audio_out,
                    $crate::traits::PortType::Control => $($control).unwrap_or(0),
                    _ => 0,
                }
            }

            fn get_port_param(
                &self,
                port: $crate::traits::PortId,
                param: $crate::traits::ParameterId,
            ) -> Option<$crate::traits::ParamValue> {
                let idx = port.index() as usize;
                let name = param.as_str();
                
                match port.port_type() {
                    $crate::traits::PortType::Node => {
                        $(
                            if name == $node_param_name {
                                return Some($crate::traits::ParamValue::$node_param_type(
                                    self.$field_name as _
                                ));
                            }
                        )*
                        None
                    }
                    
                    $crate::traits::PortType::AudioIn => {
                        self.audio_in_params.get(&(idx, name.to_string())).cloned()
                    }
                    
                    $crate::traits::PortType::AudioOut => {
                        self.audio_out_params.get(&(idx, name.to_string())).cloned()
                    }
                    
                    $crate::traits::PortType::Control => {
                        self.control_params.get(&(idx, name.to_string())).cloned()
                    }
                    
                    _ => None,
                }
            }

            fn set_port_param(
                &mut self,
                port: $crate::traits::PortId,
                param: $crate::traits::ParameterId,
                value: $crate::traits::ParamValue,
            ) -> Result<(), $crate::traits::AudioError> {
                let idx = port.index() as usize;
                let name = param.as_str();
                
                match port.port_type() {
                    $crate::traits::PortType::Node => {
                        match (name, value) {
                            $(
                                ($node_param_name, $crate::traits::ParamValue::$node_param_type(v)) => {
                                    self.$field_name = v as _;
                                    Ok(())
                                }
                            )*
                            _ => Err($crate::traits::AudioError::Parameter(
                                format!("Unknown node parameter: {}", name)
                            )),
                        }
                    }
                    
                    $crate::traits::PortType::AudioIn => {
                        if idx >= $audio_in {
                            return Err($crate::traits::AudioError::Parameter(
                                format!("AudioIn port index {} out of range", idx)
                            ));
                        }
                        self.audio_in_params.insert((idx, name.to_string()), value);
                        Ok(())
                    }
                    
                    $crate::traits::PortType::AudioOut => {
                        if idx >= $audio_out {
                            return Err($crate::traits::AudioError::Parameter(
                                format!("AudioOut port index {} out of range", idx)
                            ));
                        }
                        self.audio_out_params.insert((idx, name.to_string()), value);
                        Ok(())
                    }
                    
                    $crate::traits::PortType::Control => {
                        if idx >= $($control).unwrap_or(0) {
                            return Err($crate::traits::AudioError::Parameter(
                                format!("Control port index {} out of range", idx)
                            ));
                        }
                        self.control_params.insert((idx, name.to_string()), value);
                        Ok(())
                    }
                    
                    _ => Err($crate::traits::AudioError::Parameter(
                        format!("Cannot set parameters on {:?} ports", port.port_type())
                    )),
                }
            }

            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
                $(
                    $( self.$buf_name.reset(); )*
                )?
            }

            fn reset(&mut self) {
                $( self.state = $state_default; )?
                $(
                    $( self.$buf_name.reset(); )*
                )?
            }

            fn node_type_id(&self) -> $crate::traits::NodeTypeId {
                $crate::traits::NodeTypeId::of::<Self>()
            }

            fn metadata(&self) -> $crate::traits::NodeMetadata {
                let mut params = vec![
                    $(
                        $crate::traits::ParamMetadata {
                            name: $node_param_name.to_string(),
                            description: $node_doc.to_string(),
                            typ: $crate::traits::ParamType::$node_param_type,
                            default: $crate::traits::ParamValue::$node_param_type($node_default),
                            min: $node_min,
                            max: $node_max,
                            step: $node_step,
                            unit: $node_unit.map(|s| s.to_string()),
                            choices: None,
                        }
                    ),*
                ];
                
                // Добавляем метаданные для параметров AudioIn портов
                $(
                    $(
                        params.push($crate::traits::ParamMetadata {
                            name: format!("in_{}_{}", 
                                stringify!($in_port_pattern), 
                                $in_param_name),
                            description: $in_doc.to_string(),
                            typ: $crate::traits::ParamType::$in_param_type,
                            default: $crate::traits::ParamValue::$in_param_type($in_default),
                            min: $in_min,
                            max: $in_max,
                            step: $in_step,
                            unit: $in_unit.map(|s| s.to_string()),
                            choices: None,
                        });
                    )*
                )?
                
                // Добавляем метаданные для параметров AudioOut портов
                $(
                    $(
                        params.push($crate::traits::ParamMetadata {
                            name: format!("out_{}_{}", 
                                stringify!($out_port_pattern), 
                                $out_param_name),
                            description: $out_doc.to_string(),
                            typ: $crate::traits::ParamType::$out_param_type,
                            default: $crate::traits::ParamValue::$out_param_type($out_default),
                            min: $out_min,
                            max: $out_max,
                            step: $out_step,
                            unit: $out_unit.map(|s| s.to_string()),
                            choices: None,
                        });
                    )*
                )?
                
                // Добавляем метаданные для параметров Control портов
                $(
                    $(
                        params.push($crate::traits::ParamMetadata {
                            name: format!("ctrl_{}_{}", 
                                stringify!($ctrl_port_pattern), 
                                $ctrl_param_name),
                            description: $ctrl_doc.to_string(),
                            typ: $crate::traits::ParamType::$ctrl_param_type,
                            default: $crate::traits::ParamValue::$ctrl_param_type($ctrl_default),
                            min: $ctrl_min,
                            max: $ctrl_max,
                            step: $ctrl_step,
                            unit: $ctrl_unit.map(|s| s.to_string()),
                            choices: None,
                        });
                    )*
                )?
                
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

        impl $crate::traits::Processor for $name {
            fn is_linear(&self) -> bool {
                false // Можно переопределить в реализации
            }

            fn reset_state(&mut self) {
                self.reset();
            }
        }
    };
}