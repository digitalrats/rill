//! Пример использования микшера с kama-core

use kama_mixer::*;
use kama_core::{AudioGraph, node::GainNode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama Mixer Demo ===");
    
    // 1. Создаем аудиограф
    let mut graph = AudioGraph::new(44100.0);
    
    // 2. Создаем микшер
    let mixer = MixerFactory::create_5ch_mixer(44100.0);
    let mixer_id = graph.add_node(Box::new(mixer));
    
    println!("Created mixer with ID: {:?}", mixer_id);
    println!("Mixer has {} inputs, 2 outputs", graph.get_node(mixer_id).unwrap().num_inputs());
    
    // 3. Создаем несколько Gain узлов как источники
    let gain_nodes: Vec<_> = (0..5)
        .map(|i| {
            let gain = 0.5 + i as f32 * 0.1;
            let node = GainNode::new(gain);
            graph.add_node(Box::new(node))
        })
        .collect();
    
    println!("Created 5 gain nodes");
    
    // 4. Создаем Bitcrusher отдельно
    let bitcrusher = BitcrusherNode::new(44100.0);
    let bitcrusher_id = graph.add_node(Box::new(bitcrusher));
    
    println!("Created bitcrusher node");
    
    // 5. Демонстрация параметров микшера
    if let Some(mixer_node) = graph.get_node_mut(mixer_id) {
        // Получаем метаданные
        let metadata = mixer_node.metadata();
        println!("\nMixer parameters:");
        
        for param in metadata.parameters.iter().take(10) {
            println!("  - {}: {:?}", param.name, param.typ);
        }
        
        // Меняем параметры
        mixer_node.set_param("master_level", kama_core::param::ParamValue::Float(0.7))
            .expect("Failed to set master level");
        
        mixer_node.set_param("0_level", kama_core::param::ParamValue::Float(0.9))
            .expect("Failed to set channel 0 level");
            
        mixer_node.set_param("0_pan", kama_core::param::ParamValue::Float(-0.8))
            .expect("Failed to set channel 0 pan");
    }
    
    // 6. Экспорт/импорт конфигурации
    if let Some(mixer_node) = graph.get_node(mixer_id) {
        if let Some(mixer) = mixer_node.as_any().downcast_ref::<MixerNode>() {
            let config = mixer.export_config();
            println!("\nExported mixer config:");
            println!("  Name: {}", config.name);
            println!("  Channels: {}", config.channels.len());
            println!("  Master level: {:.2}", config.master_level);
            
            // Можно сохранить в файл:
            // let json = serde_json::to_string_pretty(&config)?;
            // std::fs::write("mixer_preset.json", json)?;
        }
    }
    
    println!("\n=== Mixer Demo Complete ===");
    
    Ok(())
}