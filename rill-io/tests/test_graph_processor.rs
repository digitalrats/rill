#![cfg(feature = "graph")]

use rill_core::time::SystemClock;
use rill_graph::GraphBuilder;
use rill_io::{
    backends::NullBackend, processor::GraphProcessor, AudioBackend, AudioConfig, AudioEngine,
    AudioProcessor,
};

const BUF_SIZE: usize = 256;

#[test]
fn test_graph_processor_creation() {
    let builder = GraphBuilder::<f32, BUF_SIZE>::new();
    let clock = Box::new(SystemClock::with_sample_rate(44100.0));
    let processor = GraphProcessor::from_builder(builder, clock).unwrap();

    let config = AudioConfig::default();
    let backend = NullBackend::new(config);
    let mut engine = AudioEngine::new(backend, processor);

    engine.start().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    engine.stop().unwrap();
}

#[test]
fn test_graph_processor_empty_graph() {
    let builder = GraphBuilder::<f32, BUF_SIZE>::new();
    let clock = Box::new(SystemClock::with_sample_rate(44100.0));
    let processor = GraphProcessor::from_builder(builder, clock).unwrap();

    assert_eq!(processor.node_count(), 0);
    assert!(processor.topo_order().is_empty());
}
