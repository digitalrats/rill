#![cfg(feature = "graph")]

use kama_core_traits::{AudioNode, ParamValue, PortId};
use kama_digital_filters::{BiquadFilter, FilterType};
use kama_graph::AudioGraph;
use kama_io::{
    backends::NullBackend, processor::GraphProcessor, AudioBackend, AudioConfig, AudioEngine,
    AudioProcessor,
};
use kama_oscillators::audio::SineOsc;

#[test]
fn test_graph_processor_creation() {
    let sample_rate = 44100.0;
    let mut graph = AudioGraph::new(sample_rate);

    let osc = SineOsc::new(440.0).with_amplitude(0.5);
    let osc_id = graph.add_node(Box::new(osc));

    let filter = BiquadFilter::new(FilterType::LowPass, 1000.0, 0.707, 0.0);
    let filter_id = graph.add_node(Box::new(filter));

    graph
        .connect(PortId::output(osc_id, 0), PortId::input(filter_id, 0), 1.0)
        .unwrap();

    let processor = GraphProcessor::new(graph, Some(osc_id), Some(filter_id));

    let config = AudioConfig::default();
    let backend = NullBackend::new(config);
    let mut engine = AudioEngine::new(backend, processor);

    engine.start().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    engine.stop().unwrap();
}

#[test]
fn test_graph_processor_set_param() {
    let sample_rate = 44100.0;
    let mut graph = AudioGraph::new(sample_rate);

    let osc = SineOsc::new(440.0);
    let osc_id = graph.add_node(Box::new(osc));

    let processor = GraphProcessor::new(graph, Some(osc_id), Some(osc_id));

    processor
        .set_node_param(osc_id, "frequency", ParamValue::Float(880.0))
        .unwrap();

    let freq = processor.with_graph_read(|g| {
        if let Some(node) = g.get_node(osc_id) {
            if let Some(ParamValue::Float(f)) = node.get_param("frequency") {
                f
            } else {
                0.0
            }
        } else {
            0.0
        }
    });
    assert!((freq - 880.0).abs() < 0.001);
}
