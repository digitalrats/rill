//! Пример гранулярного синтеза

use kama_buffers::{MultiHeadBuffer, ReadMode};
use kama_core_traits::{AudioNode, NodeId, ParamValue, PortId};
use kama_digital_effects::Delay;
use kama_digital_filters::{BiquadFilter, FilterType};
use kama_graph::AudioGraph;
use kama_io::{
    backends::{CpalBackend, NullBackend},
    processor::GraphProcessor,
    AudioConfig, AudioEngine, BackendType,
};
use kama_oscillators::audio::SineOsc;

#[cfg(feature = "alsa")]
use kama_io::backends::AlsaBackend;

fn create_backend(
    config: AudioConfig,
) -> Result<Box<dyn kama_io::AudioBackend>, Box<dyn std::error::Error>> {
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

fn create_granular_graph(sample_rate: f32) -> (AudioGraph, NodeId, NodeId) {
    let mut graph = AudioGraph::new(sample_rate);

    // Создаём гранулярный буфер
    let mut buffer = MultiHeadBuffer::new(4096, sample_rate);

    // Добавляем гранулярную головку
    buffer.add_head_with_params(
        1.0, // speed
        0.0, // pan
        0.8, // volume
        ReadMode::Granular {
            grain_size: 256,
            spacing: 512,
            randomization: 0.3,
        },
    );

    let buffer_id = graph.add_node(Box::new(buffer));

    // Создаём фильтр
    let filter = BiquadFilter::new(FilterType::LowPass, 2000.0, 0.707, 0.0);
    let filter_id = graph.add_node(Box::new(filter));

    // Создаём задержку
    let delay = Delay::new(0.2, 0.3, 0.5);
    let delay_id = graph.add_node(Box::new(delay));

    // Соединяем
    graph
        .connect(
            PortId::output(buffer_id, 0),
            PortId::input(filter_id, 0),
            1.0,
        )
        .unwrap();

    graph
        .connect(
            PortId::output(filter_id, 0),
            PortId::input(delay_id, 0),
            1.0,
        )
        .unwrap();

    (graph, buffer_id, delay_id)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama IO Granular Processing Demo ===\n");

    let config = AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(256)
        .with_channels(2);

    println!(
        "Audio config: {} Hz, {} samples",
        config.sample_rate, config.buffer_size
    );

    // Создаём граф
    let (graph, input_id, output_id) = create_granular_graph(config.sample_rate as f32);

    // Создаём процессор
    let processor = GraphProcessor::new(graph, Some(input_id), Some(output_id));

    // Создаём бэкенд
    let backend = create_backend(config.clone())?;
    println!("Using backend: {}", backend.backend_type().name());

    // Создаём движок
    let mut engine = AudioEngine::new(backend, processor);

    println!("\nStarting audio engine...");
    engine.start()?;

    println!("Playing granular synthesis for 3 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(3));

    println!("Stopping...");
    engine.stop()?;

    println!("\nDone! Xruns: {}", engine.xruns());

    Ok(())
}
