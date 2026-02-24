//! Audio graph implementation for Kama Audio
//!
//! Provides a flexible audio processing graph that can route audio between
//! different nodes (generators, effects, filters, etc.).

#![warn(missing_docs)]

mod connection;
mod error;
mod graph;
mod node;
mod registry;

pub use connection::Connection;
pub use error::{GraphError, GraphResult};
pub use graph::AudioGraph;

use kama_core::traits::{
    AudioError, AudioNode, NodeCategory, NodeId, NodeMetadata, NodeTypeId, 
    ParamValue, ParameterId, PortId, PortType
};

#[cfg(test)]
mod tests {
    use super::*;
    use kama_core::traits::{
        AudioError, AudioNode, NodeCategory, NodeId, NodeMetadata, NodeTypeId, ParamValue, PortId,
    };

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

        fn num_ports(&self, port_type: PortType) -> usize {
            match port_type {
                PortType::AudioIn => 1,
                PortType::AudioOut => 1,
                _ => 0,
            }
        }

        fn get_port_param(&self, port: PortId, param: ParameterId) -> Option<ParamValue> {
            if port.port_type() == PortType::AudioOut && param.as_str() == "gain" {
                Some(ParamValue::Float(self.gain))
            } else {
                None
            }
        }

        fn set_port_param(&mut self, port: PortId, param: ParameterId, value: ParamValue) -> Result<(), AudioError> {
            if port.port_type() == PortType::AudioOut && param.as_str() == "gain" {
                if let ParamValue::Float(g) = value {
                    self.gain = g;
                    Ok(())
                } else {
                    Err(AudioError::Parameter("Expected float".into()))
                }
            } else {
                Err(AudioError::Parameter(format!("Unknown port/param: {}/{}", port, param)))
            }
        }

        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
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

        graph
            .connect(PortId::audio_out(id1, 0), PortId::audio_in(id2, 0), 1.0)
            .unwrap();

        assert_eq!(graph.connection_count(), 1);

        graph.remove_node(id1);

        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.connection_count(), 0);
    }
}
