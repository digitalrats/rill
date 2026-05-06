//! Push model: запись с микрофона в WAV-файл.
//!
//! AudioInput (Source) — активный узел, управляет графом.
//! RecordingSink — накапливает сэмплы в памяти.
//! По завершении (Enter) пишет WAV на диск.
//!
//! Usage:
//!   cargo run --example record_mic --features pipewire [file.wav]
//!   cargo run --example record_mic [file.wav]  (ALSA по умолчанию)
//!
//! PipeWire: capture-only (output_channels=0), process_cb из capture callback.
//! ALSA: blocking snd_pcm_readi, без playback PCM.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rill_adrift::io::input::AudioInput;
use rill_adrift::io::signal_io::IoBackendPtr;
use rill_adrift::io::AudioConfig;
use rill_adrift::rill_core::io::IoBackend;
use rill_adrift::rill_core::queues::MpscQueue;
use rill_adrift::rill_core::time::{ClockTick, SystemClock};
use rill_adrift::rill_core::traits::active::{ActiveNode, GraphHandle};
use rill_adrift::rill_core::traits::processable::NodeVariant;
use rill_adrift::rill_core::traits::{
    NodeCategory, NodeId, NodeMetadata, NodeState, ParamValue, ParameterId, Port, ProcessResult,
    SignalNode, Sink,
};
use rill_adrift::rill_core::Transcendental;
use rill_adrift::rill_graph::GraphBuilder;

const BUF: usize = 256;
const RATE: f32 = 48000.0;

// ============================================================================
// RecordingSink — накапливает аудио в памяти
// ============================================================================

struct RecordingSink<T: Transcendental, const B: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, B>>,
    state: NodeState<T, B>,
    recorded: Arc<Mutex<Vec<f32>>>,
    calls: Arc<std::sync::atomic::AtomicU64>,
}

impl<T: Transcendental, const B: usize> RecordingSink<T, B> {
    fn new(recorded: Arc<Mutex<Vec<f32>>>) -> Self {
        Self {
            id: NodeId(0),
            metadata: NodeMetadata::new("RecordingSink", NodeCategory::Sink),
            inputs: vec![
                Port::input(NodeId(0), 0, "left"),
                Port::input(NodeId(0), 1, "right"),
            ],
            state: NodeState::new(RATE),
            recorded,
            calls: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }
    fn call_count(&self) -> u64 {
        self.calls.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl<T: Transcendental, const B: usize> SignalNode<T, B> for RecordingSink<T, B> {
    fn node_type_id(&self) -> rill_adrift::rill_core::NodeTypeId
    where
        Self: 'static + Sized,
    {
        rill_adrift::rill_core::NodeTypeId::of::<Self>()
    }
    fn id(&self) -> NodeId {
        self.id
    }
    fn set_id(&mut self, id: NodeId) {
        self.id = id;
    }
    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
    }
    fn init(&mut self, _sample_rate: f32) {}
    fn reset(&mut self) {
        self.state.sample_pos = 0;
    }

    fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
        None
    }
    fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
        Err(rill_adrift::rill_core::ProcessError::parameter("no params"))
    }
    fn input_port(&self, index: usize) -> Option<&Port<T, B>> {
        self.inputs.get(index)
    }
    fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<T, B>> {
        self.inputs.get_mut(index)
    }
    fn output_port(&self, _index: usize) -> Option<&Port<T, B>> {
        None
    }
    fn output_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, B>> {
        None
    }
    fn control_port(&self, _index: usize) -> Option<&Port<T, B>> {
        None
    }
    fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, B>> {
        None
    }
    fn num_signal_inputs(&self) -> usize {
        2
    }
    fn num_signal_outputs(&self) -> usize {
        0
    }
    fn state(&self) -> &NodeState<T, B> {
        &self.state
    }
    fn state_mut(&mut self) -> &mut NodeState<T, B> {
        &mut self.state
    }
}

impl<T: Transcendental, const B: usize> Sink<T, B> for RecordingSink<T, B> {
    fn consume(
        &mut self,
        _clock: &ClockTick,
        _signal_inputs: &[&[T; B]],
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
        _feedback_inputs: &[&[T; B]],
    ) -> ProcessResult<()> {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let (Some(lp), Some(rp)) = (self.inputs.first(), self.inputs.get(1)) {
            let l = lp.buffer.as_array();
            let r = rp.buffer.as_array();
            let mut dst = self.recorded.lock().unwrap();
            for i in 0..B {
                dst.push(l[i].to_f32());
                dst.push(r[i].to_f32());
            }
        }
        self.state.advance();
        Ok(())
    }
}

// ============================================================================
// WAV writer (16-bit PCM)
// ============================================================================

fn write_wav(
    path: &str,
    sample_rate: u32,
    channels: u16,
    samples: &[f32],
) -> Result<(), Box<dyn std::error::Error>> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        let amp = s.clamp(-1.0, 1.0);
        let sample = (amp * 32767.0) as i16;
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}

// ============================================================================
// Main
// ============================================================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "output.wav".into());

    // ── Бэкенд ─────────────────────────────────────────────────────────────
    let config = AudioConfig::default()
        .with_sample_rate(RATE as u32)
        .with_buffer_size(BUF as u32)
        .with_input_channels(2)
        .with_output_channels(0);

    #[cfg(feature = "pipewire")]
    let backend =
        Box::new(rill_adrift::io::PipewireBackend::new(config).expect("PipewireBackend::new"));
    #[cfg(all(
        feature = "cpal",
        not(any(feature = "pipewire", feature = "alsa", feature = "jack"))
    ))]
    let backend = Box::new(rill_adrift::io::CpalBackend::new(config).expect("CpalBackend::new"));
    #[cfg(feature = "jack")]
    let backend = Box::new(rill_adrift::io::JackBackend::new(config).expect("JackBackend::new"));
    #[cfg(all(
        feature = "alsa",
        not(any(feature = "pipewire", feature = "cpal", feature = "jack"))
    ))]
    let backend = Box::new(rill_adrift::io::AlsaBackend::new(config).expect("AlsaBackend::new"));
    let backend_ptr = IoBackendPtr::from_ref(&*backend);

    // ── Граф: AudioInput → RecordingSink ──────────────────────────────────
    let recorded = Arc::new(Mutex::new(Vec::<f32>::new()));

    let mut builder = GraphBuilder::<f32, BUF>::new();

    let mut input = AudioInput::<f32, BUF>::new();
    input.set_io_ptr(backend_ptr);
    let src = builder.add_source(Box::new(input));

    let sink = RecordingSink::<f32, BUF>::new(recorded.clone());
    let snk = builder.add_sink(Box::new(sink));

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

    audio_input.start(handle);

    // ── Запуск аудиотреда ─────────────────────────────────────────────────
    let running = Arc::new(AtomicBool::new(true));
    let t_running = running.clone();
    let audio_thread = std::thread::spawn(move || {
        let _ = backend.run(t_running.clone());
        while t_running.load(Ordering::Acquire) {
            std::thread::park();
        }
        let _ = backend.stop();
    });

    let start = std::time::Instant::now();
    println!("▶ Запись микрофона... Нажмите Enter для остановки.");
    let mut input_line = String::new();
    std::io::stdin().read_line(&mut input_line)?;

    let elapsed = start.elapsed();

    running.store(false, Ordering::Release);
    audio_thread.thread().unpark();
    let _ = audio_thread.join();

    // ── Счётчик вызовов из RecordingSink ──────────────────────────────────
    let sink_calls = {
        let sink_idx = topo[1];
        let n = &mut graph.nodes_mut()[sink_idx];
        if let NodeVariant::Sink(ref mut s) = n {
            let rs: &mut RecordingSink<f32, BUF> = unsafe {
                &mut *(s.as_mut() as *mut dyn rill_adrift::rill_core::traits::Sink<f32, BUF>
                    as *mut RecordingSink<f32, BUF>)
            };
            rs.call_count()
        } else {
            0
        }
    };

    // ── Сохранение WAV ────────────────────────────────────────────────────
    let data = recorded.lock().unwrap();
    let total_samples = data.len();
    let channels = 2;
    let frames = total_samples / channels;
    let elapsed_secs = elapsed.as_secs_f64();
    let capture_rate_f = frames as f64 / elapsed_secs;
    let wav_rate = if capture_rate_f > 64000.0 {
        96000
    } else {
        48000
    };
    let file_dur = frames as f64 / wav_rate as f64;

    let max_amp = data.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    if max_amp < 0.001 {
        eprintln!("⚠ Тишина (max = {max_amp:.6}).");
    } else {
        eprintln!("✓ Сигнал (max = {max_amp:.6})");
    }

    write_wav(&out_path, wav_rate, channels as u16, &data)?;

    println!("⏹ Сохранено: {out_path}");
    println!("   Запись: {elapsed_secs:.2}s, файл: {file_dur:.2}s, freq: {:.0} Гц → WAV {} Гц, {channels}ch, {total_samples} сэмплов", capture_rate_f, wav_rate);
    Ok(())
}
