//! Microphone recording through standard pipeline.
//!
//! 1. `RecordingSink` registered in Runtime via `register_node_fn`
//! 2. Graph topology defined via `GraphDef`
//! 3. Backend created by factory name
//!
//! Usage:
//!   cargo run --example record_mic --features pipewire [file.wav]
//!   cargo run --example record_mic [file.wav]  (ALSA by default)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rill_adrift::rill_core::traits::{
    Node, NodeCategory, NodeId, NodeMetadata, NodeState, NodeVariant, ParamValue, ParameterId,
    Params, Port, ProcessResult, Sink,
};
use rill_adrift::rill_core::Transcendental;
use rill_adrift::rill_graph::serialization::{ConnectionDef, GraphDef, NodeDef, SignalKind};
use rill_adrift::runtime::{Runtime, RuntimeConfig};

const BUF: usize = 256;
const RATE: f32 = 48000.0;

// ============================================================================
// RecordingSink
// ============================================================================

struct RecordingSink<T: Transcendental, const B: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, B>>,
    state: NodeState<T, B>,
    recorded: Arc<Mutex<Vec<f32>>>,
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
        }
    }
}

impl<T: Transcendental, const B: usize> Node<T, B> for RecordingSink<T, B> {
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
        _clock: &rill_adrift::rill_core::time::ClockTick,
        _signal_inputs: &[&[T; B]],
        _control_inputs: &[T],
        _clock_inputs: &[rill_adrift::rill_core::time::ClockTick],
        _feedback_inputs: &[&[T; B]],
    ) -> ProcessResult<()> {
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
// WAV writer
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
        writer.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16)?;
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

    // Select backend based on active feature flag
    #[cfg(feature = "pipewire")]
    let backend_name = "pipewire";
    #[cfg(all(feature = "jack", not(feature = "pipewire")))]
    let backend_name = "jack";
    #[cfg(all(feature = "cpal", not(any(feature = "pipewire", feature = "jack"))))]
    let backend_name = "cpal";
    #[cfg(all(
        feature = "alsa",
        not(any(feature = "pipewire", feature = "jack", feature = "cpal"))
    ))]
    let backend_name = "alsa";
    #[cfg(not(any(
        feature = "pipewire",
        feature = "jack",
        feature = "cpal",
        feature = "alsa"
    )))]
    let backend_name = "null";

    let recorded = Arc::new(Mutex::new(Vec::<f32>::new()));

    // Runtime — owns factories
    let mut rt = Runtime::<BUF>::new(RuntimeConfig::default());

    // Configure default backend
    let mut p = std::collections::HashMap::new();
    p.insert("sample_rate".into(), ParamValue::Int(RATE as i32));
    p.insert("buffer_size".into(), ParamValue::Int(BUF as i32));
    p.insert("channels".into(), ParamValue::Int(2));
    rt.set_default_backend(backend_name, p);

    // Register custom node
    let rec = recorded.clone();
    rt.register_node_fn("rill/record_sink", move |id: NodeId, params: &Params| {
        let mut sink = RecordingSink::new(rec.clone());
        Node::set_id(&mut sink, id);
        Node::init(&mut sink, params.sample_rate);
        NodeVariant::Sink(Box::new(sink))
    });

    // Graph topology via GraphDef
    let def = GraphDef {
        format_version: "rill/1".to_string(),
        sample_rate: RATE,
        block_size: BUF,
        resources: vec![],
        nodes: vec![
            NodeDef {
                id: 0,
                type_name: "rill/input".into(),
                name: "mic".into(),
                parameters: [(
                    "channels".into(),
                    rill_adrift::rill_core::ParamValue::Float(2.0),
                )]
                .into(),
            },
            NodeDef {
                id: 1,
                type_name: "rill/record_sink".into(),
                name: "recorder".into(),
                parameters: [].into(),
            },
        ],
        connections: vec![
            ConnectionDef {
                kind: SignalKind::Signal,
                from_node: 0,
                from_port: 0,
                to_node: 1,
                to_port: 0,
            },
            ConnectionDef {
                kind: SignalKind::Signal,
                from_node: 0,
                from_port: 0,
                to_node: 1,
                to_port: 1,
            },
        ],
        description: None,
    };

    // Build graph
    let mut builder = rt.create_builder();
    def.populate(&mut builder)
        .map_err(|e| format!("populate: {e}"))?;
    let graph = builder.build().map_err(|e| format!("graph build: {e}"))?;
    let _actor_ref = graph.handle();

    // Start
    let running = Arc::new(AtomicBool::new(true));
    let t_run = running.clone();
    let audio_thread = std::thread::spawn(move || {
        graph.run(t_run).ok();
    });

    println!("▶ Recording... Press Enter to stop.");
    let mut input_line = String::new();
    std::io::stdin().read_line(&mut input_line)?;
    running.store(false, Ordering::Release);
    audio_thread.thread().unpark();
    let _ = audio_thread.join();

    // Save WAV
    let data = recorded.lock().unwrap();
    let total_samples = data.len();
    let frames = total_samples / 2;
    let wav_rate = if frames > 0 { 48000 } else { 48000 };

    let max_amp = data.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    println!(
        "{} max={max_amp:.6}",
        if max_amp < 0.001 {
            "⚠ Silence"
        } else {
            "✓ Signal"
        }
    );

    write_wav(&out_path, wav_rate, 2, &data)?;
    println!("⏹ Saved: {out_path} — {total_samples} samples");
    Ok(())
}
