// FILE: kama-core/src/graph/utils.rs
use super::{AudioGraph, NodeId, PortId};

/// Утилиты для упрощённого создания графов
pub struct GraphBuilder<'a> {
    graph: &'a mut AudioGraph,
    node_ids: Vec<(String, NodeId)>,
}

impl<'a> GraphBuilder<'a> {
    pub fn new(graph: &'a mut AudioGraph) -> Self {
        Self {
            graph,
            node_ids: Vec::new(),
        }
    }
    
    /// Добавить узел с именем
    pub fn add_named_node(&mut self, name: &str, node: Box<dyn crate::node::AudioNode>) -> NodeId {
        let id = self.graph.add_node(node);
        self.node_ids.push((name.to_string(), id));
        id
    }
    
    /// Найти узел по имени
    pub fn find_node(&self, name: &str) -> Option<NodeId> {
        self.node_ids.iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
    }
    
    /// Подключить выход к входу по именам
    pub fn connect_by_name(
        &mut self, 
        from_name: &str, 
        from_port: u8,
        to_name: &str,
        to_port: u8,
        gain: f32,
    ) -> Result<(), crate::AudioError> {
        let from_id = self.find_node(from_name)
            .ok_or_else(|| crate::AudioError::Graph(format!("Node {} not found", from_name)))?;
        
        let to_id = self.find_node(to_name)
            .ok_or_else(|| crate::AudioError::Graph(format!("Node {} not found", to_name)))?;
        
        let from_port = PortId {
            node: from_id,
            index: from_port,
            is_input: false,
        };
        
        let to_port = PortId {
            node: to_id,
            index: to_port,
            is_input: true,
        };
        
        self.graph.connect(from_port, to_port, gain)
    }
    
    /// Создать патч по умолчанию (осциллятор -> фильтр -> усилитель)
    pub fn create_default_patch(&mut self) -> Result<(), crate::AudioError> {
        use crate::dsp::{SineOscillator, BiquadFilter, BiquadType};
        use crate::node::GainNode;
        
        let osc_id = self.add_named_node(
            "oscillator",
            Box::new(SineOscillator::new(440.0)),
        );
        
        let filter_id = self.add_named_node(
            "filter",
            Box::new(BiquadFilter::new_lowpass(1000.0, 0.707)),
        );
        
        let gain_id = self.add_named_node(
            "gain",
            Box::new(GainNode::new(0.5)),
        );
        
        self.connect_by_name("oscillator", 0, "filter", 0, 1.0)?;
        self.connect_by_name("filter", 0, "gain", 0, 1.0)?;
        
        Ok(())
    }
}