//! Audio graph implementation for Kama Audio
//!
//! Provides a flexible audio processing graph that can route audio between
//! different nodes (generators, effects, filters, etc.).
//!
//! # Example
//! ```
//! use kama_graph::AudioGraph;
//! use kama_core_traits::{AudioNode, PortId, NodeId, AudioError, ParamValue, NodeMetadata, NodeCategory, NodeTypeId};
//!
//! // Простой генератор синусоиды
//! struct SineOscillator {
//!     frequency: f32,
//!     phase: f32,
//!     sample_rate: f32,
//! }
//!
//! impl SineOscillator {
//!     fn new(frequency: f32) -> Self {
//!         Self {
//!             frequency,
//!             phase: 0.0,
//!             sample_rate: 44100.0,
//!         }
//!     }
//! }
//!
//! impl AudioNode for SineOscillator {
//!     fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
//!         if outputs.is_empty() {
//!             return Ok(());
//!         }
//!         
//!         let output = &mut outputs[0];
//!         let phase_increment = 2.0 * std::f32::consts::PI * self.frequency / self.sample_rate;
//!         
//!         for sample in output.iter_mut() {
//!             *sample = self.phase.sin();
//!             self.phase += phase_increment;
//!             if self.phase > 2.0 * std::f32::consts::PI {
//!                 self.phase -= 2.0 * std::f32::consts::PI;
//!             }
//!         }
//!         
//!         Ok(())
//!     }
//!     
//!     fn get_param(&self, _name: &str) -> Option<ParamValue> { None }
//!     fn set_param(&mut self, _name: &str, _value: ParamValue) -> Result<(), AudioError> { Ok(()) }
//!     fn init(&mut self, sample_rate: f32) { self.sample_rate = sample_rate; }
//!     fn reset(&mut self) { self.phase = 0.0; }
//!     fn num_inputs(&self) -> usize { 0 }
//!     fn num_outputs(&self) -> usize { 1 }
//!     
//!     fn node_type_id(&self) -> NodeTypeId {
//!         NodeTypeId::of::<Self>()
//!     }
//!     
//!     fn metadata(&self) -> NodeMetadata {
//!         NodeMetadata {
//!             name: "Sine Oscillator".to_string(),
//!             category: NodeCategory::Generator,
//!             description: "Simple sine wave generator".to_string(),
//!             author: "Kama".to_string(),
//!             version: "1.0".to_string(),
//!             parameters: vec![],
//!         }
//!     }
//! }
//!
//! // Узел усиления
//! struct GainNode {
//!     gain: f32,
//! }
//!
//! impl GainNode {
//!     fn new(gain: f32) -> Self {
//!         Self { gain }
//!     }
//! }
//!
//! impl AudioNode for GainNode {
//!     fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
//!         if inputs.is_empty() || outputs.is_empty() {
//!             return Ok(());
//!         }
//!         
//!         let input = inputs[0];
//!         let output = &mut outputs[0];
//!         
//!         for i in 0..input.len().min(output.len()) {
//!             output[i] = input[i] * self.gain;
//!         }
//!         
//!         Ok(())
//!     }
//!     
//!     fn get_param(&self, name: &str) -> Option<ParamValue> {
//!         match name {
//!             "gain" => Some(ParamValue::Float(self.gain)),
//!             _ => None,
//!         }
//!     }
//!     
//!     fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
//!         match (name, value) {
//!             ("gain", ParamValue::Float(g)) => {
//!                 self.gain = g;
//!                 Ok(())
//!             }
//!             _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
//!         }
//!     }
//!     
//!     fn init(&mut self, _sample_rate: f32) {}
//!     fn reset(&mut self) {}
//!     fn num_inputs(&self) -> usize { 1 }
//!     fn num_outputs(&self) -> usize { 1 }
//!     
//!     fn node_type_id(&self) -> NodeTypeId {
//!         NodeTypeId::of::<Self>()
//!     }
//!     
//!     fn metadata(&self) -> NodeMetadata {
//!         NodeMetadata {
//!             name: "Gain".to_string(),
//!             category: NodeCategory::Effect,
//!             description: "Simple gain control".to_string(),
//!             author: "Kama".to_string(),
//!             version: "1.0".to_string(),
//!             parameters: vec![],
//!         }
//!     }
//! }
//!
//! // Создаём граф и добавляем узлы
//! let mut graph = AudioGraph::new(44100.0);
//! let oscillator = Box::new(SineOscillator::new(440.0));
//! let gain = Box::new(GainNode::new(0.5));
//!
//! let osc_id = graph.add_node(oscillator);
//! let gain_id = graph.add_node(gain);
//!
//! // Соединяем осциллятор с усилителем
//! graph.connect(
//!     PortId::output(osc_id, 0),
//!     PortId::input(gain_id, 0),
//!     1.0
//! ).unwrap();
//!
//! // Обрабатываем аудио
//! let mut output = vec![0.0f32; 512];
//! graph.process(&[], &mut [&mut output]).unwrap();
//!
//! // Проверяем, что сигнал есть на выходе
//! assert!(output.iter().any(|&x| x != 0.0));
//! println!("First 10 samples: {:?}", &output[..10]);
//! ```
#![warn(missing_docs)]

mod error;
mod connection;
mod processor;
mod graph;

pub use error::{GraphError, GraphResult};
pub use connection::Connection;
pub use graph::AudioGraph;

// Реэкспортируем Processor для удобства
pub use processor::{BufferManager, NodeProcessor};

#[cfg(test)]
mod tests {
    use super::*;
    use kama_core_traits::{AudioNode, AudioError, ParamValue, NodeMetadata, NodeCategory, NodeTypeId, NodeId, PortId};
    
    // Простой тестовый узел для демонстрации
    struct GainNode {
        gain: f32,
    }
    
    impl GainNode {
        fn new(gain: f32) -> Self {
            Self { gain }
        }
    }
    
    impl AudioNode for GainNode {
        fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
            if inputs.is_empty() || outputs.is_empty() {
                return Ok(());
            }
            
            let input = inputs[0];
            let output = &mut outputs[0];
            
            for i in 0..input.len().min(output.len()) {
                output[i] = input[i] * self.gain;
            }
            
            Ok(())
        }
        
        fn get_param(&self, name: &str) -> Option<ParamValue> {
            match name {
                "gain" => Some(ParamValue::Float(self.gain)),
                _ => None,
            }
        }
        
        fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
            match (name, value) {
                ("gain", ParamValue::Float(g)) => {
                    self.gain = g;
                    Ok(())
                }
                _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
            }
        }
        
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
        fn num_inputs(&self) -> usize { 1 }
        fn num_outputs(&self) -> usize { 1 }
        
        fn node_type_id(&self) -> NodeTypeId {
            NodeTypeId::of::<Self>()
        }
        
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "Gain".to_string(),
                category: NodeCategory::Effect,
                description: "Simple gain control".to_string(),
                author: "Kama".to_string(),
                version: "1.0".to_string(),
                parameters: vec![],
            }
        }
    }
    
    #[test]
    fn test_graph_process() {
        let mut graph = AudioGraph::new(44100.0);
        
        let gain = Box::new(GainNode::new(0.5));
        let gain_id = graph.add_node(gain);
        
        let mut output = vec![0.0f32; 10];
        graph.process(&[], &mut [&mut output]).unwrap();
        
        // Граф без входов должен работать (генераторы обрабатываются)
        assert_eq!(output.len(), 10);
    }
    
    #[test]
    fn test_remove_node() {
        let mut graph = AudioGraph::new(44100.0);
        
        let node1 = Box::new(GainNode::new(0.5));
        let node2 = Box::new(GainNode::new(0.7));
        
        let id1 = graph.add_node(node1);
        let id2 = graph.add_node(node2);
        
        assert_eq!(graph.node_count(), 2);
        
        graph.connect(
            PortId::output(id1, 0),
            PortId::input(id2, 0),
            1.0
        ).unwrap();
        
        assert_eq!(graph.connection_count(), 1);
        
        graph.remove_node(id1);
        
        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.connection_count(), 0);
    }
}