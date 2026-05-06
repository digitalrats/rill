//! Push model: микрофон → AudioInput → AudioOutput → динамики.
//!
//! AudioInput (Source) — активный узел: его `start()` регистрирует
//! callback на бэкенде, который дёргается на каждом блоке аудио.
//! AudioOutput (Sink) — пассивный: его `consume()` достигается через
//! `Port::propagate()` из AudioInput.
//!
//! Бэкенд: PipeWire (полный дуплекс — захват + воспроизведение).
//!
//! Usage:
//!   cargo run --example push_model --features pipewire
//!
//! Нажмите Enter для остановки.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::io::input::AudioInput;
use rill_adrift::io::output::AudioOutput;
use rill_adrift::io::signal_io::IoBackendPtr;
use rill_adrift::io::AudioConfig;
use rill_adrift::rill_core::io::IoBackend;
use rill_adrift::rill_core::queues::MpscQueue;
use rill_adrift::rill_core::time::SystemClock;
use rill_adrift::rill_core::traits::active::{ActiveNode, GraphHandle};
use rill_adrift::rill_core::traits::processable::NodeVariant;
use rill_adrift::rill_core::traits::Sink;
use rill_adrift::rill_graph::GraphBuilder;

const BUF: usize = 256;
const RATE: f32 = 48000.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── Бэкенд ──────────────────────────────────────────────────────────
    let config = AudioConfig::default()
        .with_sample_rate(RATE as u32)
        .with_buffer_size(BUF as u32)
        .with_channels(2);

    // Priority: pipewire > jack > cpal > alsa
    #[cfg(feature = "pipewire")]
    let backend =
        Box::new(rill_adrift::io::PipewireBackend::new(config).expect("PipewireBackend::new"));
    #[cfg(all(feature = "jack", not(feature = "pipewire")))]
    let backend = Box::new(rill_adrift::io::JackBackend::new(config).expect("JackBackend::new"));
    #[cfg(all(feature = "cpal", not(any(feature = "pipewire", feature = "jack"))))]
    let backend = Box::new(rill_adrift::io::CpalBackend::new(config).expect("CpalBackend::new"));
    #[cfg(all(
        feature = "alsa",
        not(any(feature = "pipewire", feature = "jack", feature = "cpal"))
    ))]
    let backend = Box::new(rill_adrift::io::AlsaBackend::new(config).expect("AlsaBackend::new"));
    let backend_ptr = IoBackendPtr::from_ref(&*backend);

    // ── Граф: AudioInput → AudioOutput ────────────────────────────────────
    let mut builder = GraphBuilder::<f32, BUF>::new();

    let mut input = AudioInput::<f32, BUF>::new();
    input.set_io_ptr(backend_ptr);
    let src = builder.add_source(Box::new(input));

    let mut output = AudioOutput::<f32, BUF>::new();
    output.set_backend(backend_ptr);
    let snk = builder.add_sink(Box::new(output));

    builder.connect_signal(src, 0, snk, 0);
    builder.connect_signal(src, 1, snk, 1);

    let mut graph = builder
        .build(Box::new(SystemClock::with_sample_rate(RATE)), None)
        .expect("graph build");

    let topo = graph.topo_order().to_vec();
    let source_idx = topo[0];
    let nodes_ptr = graph.nodes_mut() as *mut [NodeVariant<f32, BUF>] as *mut u8;
    let len = graph.nodes().len();

    // ── Push model: AudioInput управляет графом ───────────────────────────
    let queue = Arc::new(MpscQueue::with_capacity(64));
    let handle = GraphHandle {
        nodes: nodes_ptr,
        len,
        source_idx,
        sample_rate: RATE,
        queue: Arc::as_ptr(&queue) as *const _,
    };

    // AudioOutput не active — consume() вызывается через propagate из AudioInput
    let audio_input: &mut AudioInput<f32, BUF> = {
        let n = &mut graph.nodes_mut()[source_idx];
        if let NodeVariant::Source(ref mut s) = n {
            unsafe {
                &mut *(s.as_mut() as *mut dyn rill_adrift::rill_core::traits::Source<f32, BUF>
                    as *mut AudioInput<f32, BUF>)
            }
        } else {
            panic!("expected AudioInput at index {source_idx}");
        }
    };

    // Регистрируем callback (AudioInput::start() → set_process_callback)
    audio_input.start(handle);

    // Запускаем бэкенд на выделенном аудиотреде
    let running = Arc::new(AtomicBool::new(true));
    let t_running = running.clone();
    let audio_thread = std::thread::spawn(move || {
        let _ = backend.run(t_running.clone());
        while t_running.load(Ordering::Acquire) {
            std::thread::park();
        }
        let _ = backend.stop();
    });

    println!("▶ Push model: микрофон → динамики. Нажмите Enter для остановки.");
    let mut input_line = String::new();
    std::io::stdin().read_line(&mut input_line)?;

    running.store(false, Ordering::Release);
    audio_thread.thread().unpark();
    let _ = audio_thread.join();

    println!("⏹ Остановлено.");
    Ok(())
}
