//! Microphone recording — builds the graph manually.
//!
//! 1. Custom `RecordingSink` registered in `NodeFactory`
//! 2. `rill/input` (Source, active IoBackend) + `RecordingSink` (Sink, passive)
//! 3. Backend created by factory name
//!
//! Usage:
//!   cargo run --example record_mic --features "io,serialization,sampler,portaudio"
//!   cargo run --example record_mic --features "io,serialization,sampler,pipewire" -- pipewire [file.wav]
//!   cargo run --example record_mic --features "io,serialization,sampler,alsa" -- alsa [file.wav]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rill_adrift::registration;
use rill_adrift::rill_core::traits::{
    Node, NodeCategory, NodeId, NodeMetadata, NodeState, NodeVariant, ParamValue, ParameterId,
    Params, Port, ProcessResult, Sink,
};
use rill_adrift::rill_core::Transcendental;
use rill_adrift::rill_core_actor::ActorSystem;
use rill_adrift::rill_graph::{GraphBuilder, NodeFactory};
use std::collections::HashMap;

use rill_adrift::rill_graph::backend_factory::BackendFactory;

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
        Err(rill_adrift::rill_core::traits::ProcessError::parameter(
            "no params",
        ))
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
        _ctx: &rill_adrift::rill_core::time::RenderContext,
        _signal_inputs: &[&[T; B]],
        _control_inputs: &[T],
        _clock_inputs: &[rill_adrift::rill_core::time::RenderContext],
        _feedback_inputs: &[&[T; B]],
        _tick: &rill_adrift::rill_core::time::ClockTick,
    ) -> ProcessResult<()> {
        let all_received = self.inputs.iter().all(|p| p.data_received);
        if all_received {
            if let (Some(lp), Some(rp)) = (self.inputs.first(), self.inputs.get(1)) {
                let l = lp.buffer.as_array();
                let r = rp.buffer.as_array();
                let mut dst = self.recorded.lock().unwrap();
                for i in 0..B {
                    dst.push(l[i].to_f32());
                    dst.push(r[i].to_f32());
                }
            }
            for p in &mut self.inputs {
                p.data_received = false;
            }
            self.state.advance();
        }
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
    let args: Vec<String> = std::env::args().collect();
    let positional: Vec<&String> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();
    let (backend_arg, out_path): (Option<&str>, &str) = match positional.len() {
        0 => (None, "output.wav"),
        1 => {
            let v = positional[0].as_str();
            if v.ends_with(".wav") || std::path::Path::new(v).is_file() {
                (None, v)
            } else {
                (Some(v), "output.wav")
            }
        }
        _ => (Some(positional[0].as_str()), positional[1].as_str()),
    };

    let recorded = Arc::new(Mutex::new(Vec::<f32>::new()));

    let backend_name = backend_arg.unwrap_or("portaudio").to_string();
    let backend_display = backend_name.clone();

    // Start
    let running = Arc::new(AtomicBool::new(true));
    let t_run = running.clone();
    let rec = recorded.clone();
    let audio_thread = std::thread::spawn(move || {
        // ── Register node types and backends ──────────────────
        let mut factory = NodeFactory::<f32, BUF>::new();
        registration::register_all_nodes::<BUF>(&mut factory);

        // Register custom RecordingSink
        let rec2 = rec.clone();
        factory.register_fn("rill/record_sink", move |id, params| {
            let mut sink = RecordingSink::new(rec2.clone());
            Node::set_id(&mut sink, id);
            Node::init(&mut sink, params.sample_rate);
            NodeVariant::Sink(Box::new(sink))
        });

        // ── Build graph ──────────────────────────────────────────
        let mut builder = GraphBuilder::new(Arc::new(factory));

        let mic = builder.add_node("rill/input", &Params::new(RATE));
        let recorder = builder.add_node("rill/record_sink", &Params::new(RATE));
        builder.connect_signal(mic, 0, recorder, 0);
        builder.connect_signal(mic, 1, recorder, 1);

        // ── Build and run ────────────────────────────────────────
        let system = ActorSystem::new();
        match builder.build(&system) {
            Ok(graph) => {
                eprintln!("Graph built ({} nodes). Recording...", graph.node_count());
                let mut bf = BackendFactory::new();
                registration::register_backends(&mut bf);
                let mut be_params = HashMap::new();
                be_params.insert("sample_rate".into(), ParamValue::Float(RATE));
                be_params.insert("buffer_size".into(), ParamValue::Int(BUF as i32));
                be_params.insert("channels".into(), ParamValue::Int(2));
                let mut state = graph.into_processing_state();
                if let Err(e) = state.run_with_backend(&bf, &backend_name, &be_params, t_run) {
                    eprintln!("Backend error: {e}");
                }
            }
            Err(e) => eprintln!("Build error: {e:?}"),
        }
    });

    println!(
        "▶ Recording from {} backend... Press Enter to stop.",
        backend_display
    );
    let mut input_line = String::new();
    std::io::stdin().read_line(&mut input_line)?;
    running.store(false, Ordering::Release);
    audio_thread.thread().unpark();
    let _ = audio_thread.join();

    // Save WAV
    let data = recorded.lock().unwrap();
    let total_samples = data.len();
    let wav_rate = 48000u32;

    let max_amp = data.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    println!(
        "{} max={max_amp:.6}",
        if max_amp < 0.001 {
            "⚠ Silence"
        } else {
            "✓ Signal"
        }
    );

    write_wav(out_path, wav_rate, 2, &data)?;
    println!("⏹ Saved: {out_path} — {total_samples} samples");
    Ok(())
}
