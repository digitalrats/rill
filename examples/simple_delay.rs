//! Простой пример использования Rilldelay

use rill_core::{AudioGraph, node::GainNode};
use rilldelay::TapeDelayNode;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Rilldelay Simple Example ===");
    
    // Создаём граф обработки
    let mut graph = AudioGraph::new(44100.0);
    
    // Добавляем узлы
    let gain_id = graph.add_node(Box::new(GainNode::new(0.8)));
    let delay_id = graph.add_node(Box::new(TapeDelayNode::default()));
    
    println!("Created graph with nodes:");
    println!("  - Gain node: {:?}", gain_id);
    println!("  - Delay node: {:?}", delay_id);
    
    // TODO: Добавить соединения и обработку
    
    println!("Example setup completed!");
    Ok(())
}
