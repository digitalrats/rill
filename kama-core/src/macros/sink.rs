//! # Макрос для создания активных приёмников (Sink)

/// Создаёт активный приёмник сигнала
#[macro_export]
macro_rules! sink_node {
    (
        $(#[$meta:meta])*
        $vis:vis $struct_name:ident<$T:ident: $audio_num:path, const $BUF:ident: usize>
        $(where $($bounds:tt)*)?
        {
            params { $($param_name:ident: $param_ty:ty = $param_default:expr),* $(,)? }
            $(ports { $($ports:tt)* } )?
            consume: $consume:expr
        }
    ) => {
        #[derive(Debug, Clone)]
        $vis struct $struct_name<$T: $audio_num, const $BUF: usize>
        $(where $($bounds)*)?
        {
            state: $crate::traits::node::NodeState<T,$BUF>,
            id: $crate::NodeId,
            metadata: $crate::NodeMetadata,
            inputs: Vec<$crate::Port<$T, $BUF>>,
            $(
                pub $param_name: $param_ty,
            )*
        }
        
        impl<$T: $audio_num, const $BUF: usize>
            $struct_name<$T, $BUF>
        $(where $($bounds)*)?
        {
            pub fn new(sample_rate: f32) -> Self {
                let metadata = $crate::NodeMetadata::new(
                    stringify!($struct_name),
                    $crate::NodeCategory::Sink,
                );
                
                let mut node = Self {
                    state: $crate::traits::node::NodeState::new(sample_rate),
                    id: $crate::NodeId(0),
                    metadata,
                    inputs: Vec::new(),
                    $(
                        $param_name: $param_default,
                    )*
                };
                
                $(
                    __init_ports!(ports { $($ports)* }, node, inputs)
                )?;
                
                node
            }
            
            pub fn sample_rate(&self) -> f32 {
                self.state.sample_rate
            }
        }
        
        impl<$T: $audio_num, const $BUF: usize>
            $crate::AudioNode<$T, $BUF> for $struct_name<$T, $BUF>
        $(where $($bounds)*)?
        {
            fn node_type_id(&self) -> $crate::NodeTypeId 
            where 
                Self: 'static + Sized 
            {
                $crate::NodeTypeId::of::<Self>()
            }
            
            fn id(&self) -> $crate::NodeId {
                self.id
            }
            
            fn set_id(&mut self, id: $crate::NodeId) {
                self.id = id;
            }
            
            fn metadata(&self) -> $crate::NodeMetadata {
                self.metadata.clone()
            }
            
            fn init(&mut self, sample_rate: f32) {
                self.state.sample_rate = sample_rate;
            }
            
            fn reset(&mut self) {
                self.state.sample_pos = 0;
                self.state.blocks_processed = 0;
            }
            
            fn get_parameter(&self, id: &$crate::ParameterId) -> Option<$crate::ParamValue> {
                let name = id.as_str();
                match name {
                    $(
                        stringify!($param_name) => Some($crate::ParamValue::Float(
                            <_ as $crate::math::AudioNum>::to_f32(self.$param_name)
                        )),
                    )*
                    _ => None,
                }
            }
            
            fn set_parameter(&mut self, id: &$crate::ParameterId, value: $crate::ParamValue) -> $crate::ProcessResult<()> {
                let name = id.as_str();
                if let Some(v) = value.as_f32() {
                    match name {
                        $(
                            stringify!($param_name) => {
                                self.$param_name = $crate::math::AudioNum::from_f32(v);
                                Ok(())
                            },
                        )*
                        _ => Err($crate::ProcessError::parameter(format!("Unknown parameter: {}", name))),
                    }
                } else {
                    Err($crate::ProcessError::parameter("Expected float value"))
                }
            }
            
            fn input_port(&self, index: usize) -> Option<&$crate::Port<$T, $BUF>> {
                self.inputs.get(index)
            }
            
            fn input_port_mut(&mut self, index: usize) -> Option<&mut $crate::Port<$T, $BUF>> {
                self.inputs.get_mut(index)
            }
            
            fn output_port(&self, _index: usize) -> Option<&$crate::Port<$T, $BUF>> {
                None
            }
            
            fn output_port_mut(&mut self, _index: usize) -> Option<&mut $crate::Port<$T, $BUF>> {
                None
            }
            
            fn control_port(&self, _index: usize) -> Option<&$crate::Port<$T, $BUF>> {
                None
            }
            
            fn control_port_mut(&mut self, _index: usize) -> Option<&mut $crate::Port<$T, $BUF>> {
                None
            }
            
            fn num_inputs(&self) -> usize {
                self.inputs.len()
            }
            
            fn num_outputs(&self) -> usize { 0 }
            
            fn state(&self) -> &$crate::traits::node::NodeState<T,$BUF> {
                &self.state
            }
            
            fn state_mut(&mut self) -> &mut $crate::traits::node::NodeState<T,$BUF> {
                &mut self.state
            }
        }
        
        impl<$T: $audio_num, const $BUF: usize>
            $crate::Sink<$T, $BUF> for $struct_name<$T, $BUF>
        $(where $($bounds)*)?
        {
            fn consume(
                &mut self,
                clock: &$crate::ClockTick,
                audio_inputs: &[&[$T; $BUF]],
                control_inputs: &[$T],
                clock_inputs: &[$crate::ClockTick],
                feedback_inputs: &[&[$T; $BUF]],
                control_outputs: &mut [$T],
                clock_outputs: &mut [$crate::ClockTick],
            ) -> $crate::ProcessResult<()> {
                // Передаем только self, внутри замыкания обращаемся к его полям
                ($consume)(self)?;
                Ok(())
            }
        }
    };
}