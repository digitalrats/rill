//! # Макрос для создания активных приёмников (Sink)
//!
//! Позволяет быстро создавать узлы-приёмники для вывода звука.

/// Создаёт активный приёмник сигнала
///
/// # Пример
/// ```
/// use kama_core::macros::sink_node;
/// use kama_core::math::AudioNum;
///
/// sink_node! {
///     /// Простой выходной узел (заглушка)
///     pub NullSink<T: AudioNum, const BUF_SIZE: usize>
///     {
///         // Параметры (опционально)
///         params {
///             volume: T = T::from_f32(1.0),
///         }
///         
///         // Порты (только входные для приёмника)
///         ports {
///             audio_in: 1,
///         }
///         
///         // Функция потребления
///         consume: |this, inputs| {
///             // Просто игнорируем входные данные
///             Ok(())
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! sink_node {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident<$T:ident: $audio_num:path, const $BUF:ident: usize>
        $(where $($bounds:tt)*)?
        {
            $(params $params:tt)?
            $(ports $ports:tt)?
            consume: $consume:expr
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
            $( $crate::__parse_params!(params $params) )?
        }
        
        impl<$T: $audio_num, const $BUF: usize>
            $name<$T, $BUF>
        $(where $($bounds)*)?
        {
            pub fn new(sample_rate: f32) -> Self {
                let metadata = $crate::node::NodeMetadata::new(
                    stringify!($name),
                    $crate::node::NodeCategory::Output,
                );
                
                let mut node = Self {
                    state: $crate::node::NodeState::new(sample_rate),
                    id: $crate::node::NodeId(0),
                    metadata,
                    inputs: Vec::new(),
                    $( $crate::__parse_params_defaults!(params $params) )?
                };
                
                $( $crate::__init_ports!(ports $ports, node, inputs) )?;
                
                node
            }
            
            pub fn sample_rate(&self) -> f32 {
                self.state.sample_rate
            }
        }
        
        impl<$T: $audio_num, const $BUF: usize>
            $crate::node::AudioNode<$T, $BUF> for $name<$T, $BUF>
        $(where $($bounds)*)?
        {
            fn node_type(&self) -> $crate::node::NodeType {
                $crate::node::NodeType::Sink
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
                
                ($consume)(self, &self.inputs)?;
                
                self.state.advance();
                Ok(())
            }
            
            fn input_port(&self, index: usize) -> Option<&$crate::port::Port<$T, $BUF>> {
                self.inputs.get(index)
            }
            
            fn input_port_mut(&mut self, index: usize) -> Option<&mut $crate::port::Port<$T, $BUF>> {
                self.inputs.get_mut(index)
            }
            
            fn output_port(&self, _index: usize) -> Option<&$crate::port::Port<$T, $BUF>> {
                None
            }
            
            fn output_port_mut(&mut self, _index: usize) -> Option<&mut $crate::port::Port<$T, $BUF>> {
                None
            }
            
            fn control_port(&self, _index: usize) -> Option<&$crate::port::Port<$T, $BUF>> {
                None
            }
            
            fn control_port_mut(&mut self, _index: usize) -> Option<&mut $crate::port::Port<$T, $BUF>> {
                None
            }
            
            fn num_inputs(&self) -> usize {
                self.inputs.len()
            }
            
            fn num_outputs(&self) -> usize { 0 }
            
            fn num_controls(&self) -> usize { 0 }
            
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
        
        impl<$T: $audio_num, const $BUF: usize>
            $crate::node::Sink<$T, $BUF> for $name<$T, $BUF>
        $(where $($bounds)*)?
        {
            fn start(&self, _graph: std::sync::Arc<dyn $crate::graph::AudioGraph<$T, $BUF>>) -> $crate::error::Result<()> {
                Ok(())
            }
            
            fn stop(&self) -> $crate::error::Result<()> {
                Ok(())
            }
            
            fn is_running(&self) -> bool {
                true
            }
            
            fn sample_rate(&self) -> f32 {
                self.state.sample_rate
            }
            
            fn sink_name(&self) -> &str {
                self.metadata.name.as_str()
            }
        }
    };
}