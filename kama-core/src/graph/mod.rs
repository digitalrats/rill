use std::collections::{HashMap, VecDeque};
use crate::node::AudioNode;
use crate::AudioError;

/// Идентификатор узла
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

/// Идентификатор порта
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortId {
    pub node: NodeId,
    pub index: u8,
    pub is_input: bool,
}

/// Соединение
#[derive(Debug, Clone)]
pub struct Connection {
    pub from: PortId,
    pub to: PortId,
    pub gain: f32,
}

/// Аудиограф
pub struct AudioGraph {
    nodes: HashMap<NodeId, Box<dyn AudioNode>>,
    connections: Vec<Connection>,
    processing_order: Vec<NodeId>,
    sample_rate: f32,
    next_id: u32,
}

impl AudioGraph {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            nodes: HashMap::new(),
            connections: Vec::new(),
            processing_order: Vec::new(),
            sample_rate,
            next_id: 0,
        }
    }
    
    pub fn add_node(&mut self, mut node: Box<dyn AudioNode>) -> NodeId {
        node.init(self.sample_rate);
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.nodes.insert(id, node);
        self.update_processing_order();
        id
    }
    
    pub fn connect(&mut self, from: PortId, to: PortId, gain: f32) -> Result<(), AudioError> {
        if !self.nodes.contains_key(&from.node) || !self.nodes.contains_key(&to.node) {
            return Err(AudioError::Graph("Invalid node ID".to_string()));
        }
        
        if !from.is_input && to.is_input {
            self.connections.push(Connection { from, to, gain });
            self.update_processing_order();
            Ok(())
        } else {
            Err(AudioError::Graph("Invalid connection direction".to_string()))
        }
    }
    
    fn update_processing_order(&mut self) {
        let mut dependencies: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut dependents: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        
        for conn in &self.connections {
            dependencies.entry(conn.to.node)
                .or_default()
                .push(conn.from.node);
                
            dependents.entry(conn.from.node)
                .or_default()
                .push(conn.to.node);
        }
        
        let mut order = Vec::new();
        let mut queue: VecDeque<NodeId> = self.nodes.keys()
            .filter(|&id| dependencies.get(id).map_or(true, |d| d.is_empty()))
            .copied()
            .collect();
            
        while let Some(node) = queue.pop_front() {
            order.push(node);
            
            if let Some(children) = dependents.get(&node) {
                for &child in children {
                    if let Some(pos) = dependencies.get_mut(&child) {
                        if let Some(idx) = pos.iter().position(|&n| n == node) {
                            pos.remove(idx);
                            if pos.is_empty() {
                                queue.push_back(child);
                            }
                        }
                    }
                }
            }
        }
        
        self.processing_order = order;
    }
    
    pub fn process(&mut self, _block_size: usize) -> Result<(), AudioError> {  // ФИКС: добавляем _ перед block_size
        // TODO: Реализовать обработку с буферами
        Ok(())
    }
    
    pub fn get_node(&self, id: NodeId) -> Option<&dyn AudioNode> {
        self.nodes.get(&id).map(|n| n.as_ref())
    }
    
    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut dyn AudioNode> {
        self.nodes.get_mut(&id).map(|n| n.as_mut())
    }
}