//! Пример использования GraphProcessor

use rill_core::traits::{AudioNode, NodeId, ParamValue, PortId};
use rill_digital_effects::Delay;
use rill_digital_filters::{BiquadFilter, FilterType};
use rill_graph::AudioGraph;
use rill_io::{
    backends::{CpalBackend, NullBackend},
    processor::GraphProcessor,
    AudioConfig, AudioEngine, BackendType,
};
use rill_oscillators::audio::SineOsc;

#[cfg(feature = "alsa")]
use rill_io::backends::AlsaBackend;

fn create_backend(
    config: AudioConfig,
) -> Result<Box<dyn rill_io::AudioBackend>, Box<dyn std::error::Error>> {
    #[cfg(all(target_os = "linux", feature = "alsa"))]
    {
        let backend = AlsaBackend::new(config.clone())?;
        return Ok(Box::new(backend));
    }

    #[cfg(feature = "cpal")]
    {
        let backend = CpalBackend::new(config.clone())?;
        return Ok(Box::new(backend));
    }

    Ok(Box::new(NullBackend::new(config)))
}

fn create_audio_graph(sample_rate: f32) -> (AudioGraph, NodeId, NodeId) {
    let mut graph = AudioGraph::new(sample_rate);

    // Создаём узлы
    let osc = SineOsc::new(440.0).with_amplitude(0.5);
    let osc_id = graph.add_node(Box::new(osc));

    let filter = BiquadFilter::new(FilterType::LowPass, 1000.0, 0.707, 0.0);
    let filter_id = graph.add_node(Box::new(filter));

    let delay = Delay::new(0.3, 0.4, 0.7);
    let delay_id = graph.add_node(Box::new(delay));

    // Соединяем
    graph
        .connect(PortId::output(osc_id, 0), PortId::input(filter_id, 0), 1.0)
        .unwrap();

    graph
        .connect(
            PortId::output(filter_id, 0),
            PortId::input(delay_id, 0),
            1.0,
        )
        .unwrap();

    (graph, osc_id, delay_id)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Rill IO Graph Processing Demo ===\n");

    let config = AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(256)
        .with_channels(2);

    println!(
        "Audio config: {} Hz, {} samples",
        config.sample_rate, config.buffer_size
    );

    // Создаём граф
    let (graph, input_id, output_id) = create_audio_graph(config.sample_rate as f32);

    // Создаём процессор
    let processor = GraphProcessor::new(graph, Some(input_id), Some(output_id));

    // Создаём бэкенд
    let backend = create_backend(config.clone())?;
    println!("Using backend: {}", backend.backend_type().name());

    // Создаём движок
    let mut engine = AudioEngine::new(backend, processor);

    println!("\nStarting audio engine...");
    engine.start()?;

    println!("Playing processed sine wave for 3 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(3));

    println!("Stopping...");
    engine.stop()?;

    println!("\nDone! Xruns: {}", engine.xruns());

    Ok(())
}
