//! # Макрос для создания активных источников (Source)
//!
//! Позволяет быстро создавать узлы-источники с минимальным кодом.

/// Создаёт активный источник сигнала
///
/// # Пример
/// ```
/// use kama_core::macros::source_node;
/// use kama_core::math::AudioNum;
/// use std::f32::consts::PI;
///
/// source_node! {
///     /// Простой синусоидальный генератор
///     pub SineOsc<T: AudioNum, const BUF_SIZE: usize>
///     {
///         // Параметры узла
///         params {
///             frequency: f32 = 440.0,
///             amplitude: T = T::from_f32(0.5),
///         }
///         
///         // Порты (только выходные для источника)
///         ports {
///             audio_out: 1,
///         }
///         
///         // Генератор семплов
///         generate: |this, output| {
///             let phase_inc = this.frequency / this.sample_rate();
///             
///             for i in 0..output.len() {
///                 let sample = (this.state.phase * 2.0 * PI).sin();
///                 output[i] = T::from_f32(sample) * this.amplitude;
///                 
///                 this.state.phase += phase_inc;
///                 if this.state.phase >= 1.0 {
///                     this.state.phase -= 1.0;
///                 }
///             }
///             
///             Ok(())
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! source_node {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident<$T:ident: $audio_num:path, const $BUF:ident: usize>
        $(where $($bounds:tt)*)?
        {
            $(params $params:tt)?
            $(ports $ports:tt)?
            generate: $generate:expr
        }
    ) => {
        // Состояние узла
        #[derive(Debug, Clone)]
        $vis struct $name<$T: $audio_num, const $BUF: usize>
        $(where $($bounds)*)?
        {
            /// Внутреннее состояние
            state: $crate::node::NodeState<$BUF>,
            
            /// Идентификатор узла
            id: $crate::node::NodeId,
            
            /// Метаданные
            metadata: $crate::node::NodeMetadata,
            
            /// Выходные порты
            outputs: Vec<$crate::port::Port<$T, $BUF>>,
            
            $( $crate::macros::__parse_params!(params $params) )?
        }
        
        impl<$T: $audio_num, const $BUF: usize>
            $name<$T, $BUF>
        $(where $($bounds)*)?
        {
            /// Создать новый источник
            pub fn new(sample_rate: f32) -> Self {
                let metadata = $crate::node::NodeMetadata::new(
                    stringify!($name),
                    $crate::node::NodeCategory::Generator,
                );
                
                let mut node = Self {
                    state: $crate::node::NodeState::new(sample_rate),
                    id: $crate::node::NodeId(0),
                    metadata,
                    outputs: Vec::new(),
                    $( $crate::__parse_params_defaults!(params $params) )?
                };
                
                $( $crate::__init_ports!(ports $ports, node, outputs) )?;
                
                node
            }
            
            /// Получить частоту дискретизации
            pub fn sample_rate(&self) -> f32 {
                self.state.sample_rate
            }
            
            /// Получить состояние узла
            pub fn state(&self) -> &$crate::node::NodeState<$BUF> {
                &self.state
            }
            
            /// Получить мутабельное состояние
            pub fn state_mut(&mut self) -> &mut $crate::node::NodeState<$BUF> {
                &mut self.state
            }
        }
        
        impl<$T: $audio_num, const $BUF: usize>
            $crate::node::AudioNode<$T, $BUF> for $name<$T, $BUF>
        $(where $($bounds)*)?
        {
            fn node_type(&self) -> $crate::node::NodeType {
                $crate::node::NodeType::Source
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
                
                // Получаем выходной буфер
                let output = if let Some(port) = self.outputs.first_mut() {
                    port.write()?
                } else {
                    return Ok(());
                };
                
                // Вызываем пользовательскую функцию генерации
                ($generate)(self, output)?;
                
                self.state.advance();
                Ok(())
            }
            
            fn input_port(&self, _index: usize) -> Option<&$crate::port::Port<$T, $BUF>> {
                None // У источника нет входов
            }
            
            fn input_port_mut(&mut self, _index: usize) -> Option<&mut $crate::port::Port<$T, $BUF>> {
                None
            }
            
            fn output_port(&self, index: usize) -> Option<&$crate::port::Port<$T, $BUF>> {
                self.outputs.get(index)
            }
            
            fn output_port_mut(&mut self, index: usize) -> Option<&mut $crate::port::Port<$T, $BUF>> {
                self.outputs.get_mut(index)
            }
            
            fn control_port(&self, _index: usize) -> Option<&$crate::port::Port<$T, $BUF>> {
                None
            }
            
            fn control_port_mut(&mut self, _index: usize) -> Option<&mut $crate::port::Port<$T, $BUF>> {
                None
            }
            
            fn num_inputs(&self) -> usize { 0 }
            
            fn num_outputs(&self) -> usize {
                self.outputs.len()
            }
            
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
            $crate::node::Source<$T, $BUF> for $name<$T, $BUF>
        $(where $($bounds)*)?
        {
            fn start(&self, _graph: std::sync::Arc<dyn $crate::graph::AudioGraph<$T, $BUF>>) -> $crate::error::Result<()> {
                // Базовая реализация может быть переопределена
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
            
            fn source_name(&self) -> &str {
                self.metadata.name.as_str()
            }
        }
    };
}