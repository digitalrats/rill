//! # Макрос для создания пассивных процессоров (Processor)
//!
//! Позволяет быстро создавать узлы-процессоры с входами и выходами.

/// Создаёт пассивный процессор сигнала
///
/// # Пример
/// ```
/// use kama_core::macros::processor_node;
/// use kama_core::math::AudioNum;
///
/// processor_node! {
///     /// Простой усилитель
///     pub Gain<T: AudioNum, const BUF_SIZE: usize>
///     {
///         // Параметры узла
///         params {
///             gain: T = T::from_f32(1.0),
///         }
///         
///         // Порты (вход и выход)
///         ports {
///             audio_in: 1,
///             audio_out: 1,
///         }
///         
///         // Функция обработки
///         process: |this, inputs, outputs| {
///             if let (Some(input), Some(output)) = (inputs.first(), outputs.first_mut()) {
///                 let input_slice = input.read()?;
///                 let output_slice = output.write()?;
///                 
///                 for i in 0..input_slice.len().min(output_slice.len()) {
///                     output_slice[i] = input_slice[i] * this.gain;
///                 }
///             }
///             Ok(())
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! processor_node {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident<$T:ident: $audio_num:path, const $BUF:ident: usize>
        $(where $($bounds:tt)*)?
        {
            $(params $params:tt)?
            $(ports $ports:tt)?
            process: $process:expr
        }
    ) => {
        #[derive(Debug, Clone)]
        $vis struct $name<$T: $audio_num, const $BUF: usize>
        $(where $($bounds)*)?
        {
            state: $crate::node::NodeState<$BUF>,
            id: $crate::node::NodeId,
            metadata: $crate::node::NodeMetadata,
            inputs: Vec<$crate::port::Port<$T, $BUF>>,
            outputs: Vec<$crate::port::Port<$T, $BUF>>,
            controls: Vec<$crate::port::Port<$T, $BUF>>,
            $( $crate::__parse_params!(params $params) )?
        }
        
        impl<$T: $audio_num, const $BUF: usize>
            $name<$T, $BUF>
        $(where $($bounds)*)?
        {
            pub fn new(sample_rate: f32) -> Self {
                let metadata = $crate::node::NodeMetadata::new(
                    stringify!($name),
                    $crate::node::NodeCategory::Effect,
                );
                
                let mut node = Self {
                    state: $crate::node::NodeState::new(sample_rate),
                    id: $crate::node::NodeId(0),
                    metadata,
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                    controls: Vec::new(),
                    $( $crate::__parse_params_defaults!(params $params) )?
                };
                
                $( $crate::__init_ports!(ports $ports, node, inputs, outputs, controls) )?;
                
                node
            }
            
            pub fn sample_rate(&self) -> f32 {
                self.state.sample_rate
            }
            
            pub fn state(&self) -> &$crate::node::NodeState<$BUF> {
                &self.state
            }
            
            pub fn state_mut(&mut self) -> &mut $crate::node::NodeState<$BUF> {
                &mut self.state
            }
        }
        
        impl<$T: $audio_num, const $BUF: usize>
            $crate::node::AudioNode<$T, $BUF> for $name<$T, $BUF>
        $(where $($bounds)*)?
        {
            fn node_type(&self) -> $crate::node::NodeType {
                $crate::node::NodeType::Processor
            }
            
            fn id(&self) -> $crate::node::NodeId {
                self.id
            }
            
            fn set_id(&mut self, id: $crate::node::NodeId) {
                self.id = id;
            }
            
            fn metadata(&self) -> $crate::node::NodeMetadata {
                self.metadata.clone()
            }
            
            fn init(&mut self, sample_rate: f32) -> $crate::error::Result<()> {
                self.state.sample_rate = sample_rate;
                Ok(())
            }
            
            fn reset(&mut self) -> $crate::error::Result<()> {
                self.state.sample_pos = 0;
                self.state.blocks_processed = 0;
                Ok(())
            }
            
            fn process(&mut self) -> $crate::error::Result<()> {
                if !self.state.active {
                    return Ok(());
                }
                
                ($process)(self, &self.inputs, &mut self.outputs)?;
                
                self.state.advance();
                Ok(())
            }
            
            fn input_port(&self, index: usize) -> Option<&$crate::port::Port<$T, $BUF>> {
                self.inputs.get(index)
            }
            
            fn input_port_mut(&mut self, index: usize) -> Option<&mut $crate::port::Port<$T, $BUF>> {
                self.inputs.get_mut(index)
            }
            
            fn output_port(&self, index: usize) -> Option<&$crate::port::Port<$T, $BUF>> {
                self.outputs.get(index)
            }
            
            fn output_port_mut(&mut self, index: usize) -> Option<&mut $crate::port::Port<$T, $BUF>> {
                self.outputs.get_mut(index)
            }
            
            fn control_port(&self, index: usize) -> Option<&$crate::port::Port<$T, $BUF>> {
                self.controls.get(index)
            }
            
            fn control_port_mut(&mut self, index: usize) -> Option<&mut $crate::port::Port<$T, $BUF>> {
                self.controls.get_mut(index)
            }
            
            fn num_inputs(&self) -> usize {
                self.inputs.len()
            }
            
            fn num_outputs(&self) -> usize {
                self.outputs.len()
            }
            
            fn num_controls(&self) -> usize {
                self.controls.len()
            }
            
            fn set_parameter(&mut self, name: &str, value: $T) -> $crate::error::Result<()> {
                $crate::__set_param!($($params)?, self, name, value)
            }
            
            fn get_parameter(&self, name: &str) -> Option<$T> {
                $crate::__get_param!($($params)?, self, name)
            }
            
            fn parameter_names(&self) -> Vec<&str> {
                $crate::__param_names!($($params)?)
            }
            
            fn state(&self) -> &$crate::node::NodeState<$BUF> {
                &self.state
            }
            
            fn state_mut(&mut self) -> &mut $crate::node::NodeState<$BUF> {
                &mut self.state
            }
        }
        
        impl<$T: $crate::math::AudioNum, const $BUF: usize>
            $crate::node::Processor<$T, $BUF> for $name<$T, $BUF>
        $(where $($bounds)*)?
        {
            fn latency(&self) -> usize {
                0
            }
        }
    };
}