use kama_control::{
    backends::midi::MidiBackend, ControlBackend, ControlNode, Mapping, Target, Transform,
};
use kama_core::traits::{AudioNode, NodeId};
use parking_lot::RwLock;
use std::sync::Arc;

// Заглушка для графа (в реальности будет использоваться kama-graph)
struct DummyGraph {
    nodes: Vec<NodeId>,
}

impl DummyGraph {
    fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    fn add_node(&mut self, _node: Box<dyn AudioNode>) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(id);
        id
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama Control MIDI Demo ===\n");

    // Создаем MIDI бэкенд
    let mut midi = MidiBackend::new("Kama Control")?;

    // Списываем доступные порты
    println!("Available MIDI ports:");
    for (i, device) in midi.list_devices().iter().enumerate() {
        println!("  {}: {}", i, device.name);
    }

    // Открываем первый порт
    if !midi.list_devices().is_empty() {
        midi.open_port(0)?;
        println!("\nOpened MIDI port 0");
    } else {
        println!("\nNo MIDI ports found, using dummy");
    }

    // Получаем receiver событий
    let event_rx = midi.subscribe();

    // Создаем узел управления
    let mut control_node = ControlNode::new(event_rx);

    // Создаем заглушку графа
    let mut graph = DummyGraph::new();

    // Добавляем усилитель для управления
    // В реальности это будет настоящий AudioNode
    let gain_id = NodeId(0);

    // Создаем маппинг: MIDI контроллер 7 -> громкость
    use kama_control::EventPattern;

    control_node.add_mapping(Mapping::new(
        EventPattern::MidiControl {
            channel: None,
            controller: 7,
        },
        Target {
            node_id: gain_id,
            param_name: "gain".to_string(),
            min: 0.0,
            max: 1.0,
        },
        Transform::Exponential,
    ));

    // Добавляем узел управления в граф
    let control_id = graph.add_node(Box::new(control_node));

    println!("\nControl node ID: {:?}", control_id);
    println!("Gain node ID: {:?}", gain_id);
    println!("\nMIDI Controller 7 now controls gain");
    println!("Move a fader/knob on your MIDI controller...");

    // Запускаем MIDI бэкенд
    midi.start()?;

    // Держим программу запущенной
    tokio::signal::ctrl_c().await?;

    println!("\nShutting down...");
    midi.stop()?;

    Ok(())
}
