//! Пример использования GraphProcessor с новым rill-graph API
//!
//! Граф строится через GraphBuilder, затем оборачивается в GraphProcessor
//! для использования в AudioEngine.

use rill_core::time::{ClockSource, SystemClock};
use rill_graph::GraphBuilder;
use rill_io::{
    backends::{CpalBackend, NullBackend},
    processor::GraphProcessor,
    AudioBackend, AudioConfig, AudioEngine,
};

#[cfg(feature = "alsa")]
use rill_io::backends::AlsaBackend;

const BUF_SIZE: usize = 256;

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

fn build_graph() -> GraphProcessor<f32, BUF_SIZE> {
    let builder = GraphBuilder::<f32, BUF_SIZE>::new();

    // В реальном проекте здесь добавляются узлы и соединения:
    //   let osc = builder.add_source(Box::new(my_oscillator));
    //   let filter = builder.add_processor(Box::new(my_filter));
    //   builder.connect_audio(osc, 0, filter, 0);
    //
    // Пока граф пустой — полная интеграция с GraphExecutor будет добавлена позже.

    let clock: Box<dyn ClockSource> = Box::new(SystemClock::with_sample_rate(44100.0));
    GraphProcessor::from_builder(builder, clock).expect("failed to build graph")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Rill IO Graph Processing Demo ===\n");

    let config = AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(BUF_SIZE as u32)
        .with_channels(2);

    println!(
        "Audio config: {} Hz, {} samples",
        config.sample_rate, config.buffer_size
    );

    let graph_processor = build_graph();
    println!("Graph has {} node(s)", graph_processor.node_count());
    println!("Topo order: {:?}", graph_processor.topo_order());

    let backend = create_backend(config.clone())?;
    println!("Using backend: {}", backend.backend_type().name());

    let mut engine = AudioEngine::new(backend, graph_processor);

    println!("\nStarting audio engine...");
    engine.start()?;

    println!("Playing for 3 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(3));

    println!("Stopping...");
    engine.stop()?;

    println!("\nDone! Xruns: {}", engine.xruns());

    Ok(())
}
