use std::sync::Arc;

use parking_lot::RwLock;

use rill_adrift::io::audio_io::AudioIo;
#[cfg(feature = "serialization")]
use rill_adrift::io::audio_io::AudioIoPtr;
use rill_adrift::io::buffer::IoRingBuffer;
#[cfg(feature = "serialization")]
use rill_adrift::io::output::AudioOutput;
#[cfg(feature = "serialization")]
use rill_adrift::registration;
#[cfg(feature = "serialization")]
use rill_adrift::rill_core::time::SystemClock;
#[cfg(feature = "serialization")]
use rill_adrift::rill_core::traits::processable::NodeVariant;
#[cfg(feature = "serialization")]
use rill_adrift::rill_core::traits::SignalNode;
#[cfg(feature = "serialization")]
use rill_adrift::rill_core::ParamValue;
#[cfg(feature = "serialization")]
use rill_adrift::rill_graph::serialization::{ConnectionDef, GraphDocument, NodeDef, SignalKind};

#[cfg(feature = "serialization")]
const BUF: usize = 256;
#[cfg(feature = "serialization")]
const RATE: f32 = 48000.0;

// ---------------------------------------------------------------------------
// Helper: synchronous mock backend (drives the graph on demand)
// ---------------------------------------------------------------------------

struct SyncBackend {
    cb_ptr: *mut Option<Box<dyn Fn()>>,
    input: Arc<RwLock<IoRingBuffer>>,
    output: Arc<RwLock<IoRingBuffer>>,
}
unsafe impl Send for SyncBackend {}
unsafe impl Sync for SyncBackend {}

impl SyncBackend {
    fn new(cap: usize) -> (Self, Arc<RwLock<IoRingBuffer>>, Arc<RwLock<IoRingBuffer>>) {
        let input = Arc::new(RwLock::new(IoRingBuffer::new(cap)));
        let output = Arc::new(RwLock::new(IoRingBuffer::new(cap)));
        let cb_ptr = Box::into_raw(Box::new(None));
        (
            Self {
                cb_ptr,
                input: input.clone(),
                output: output.clone(),
            },
            input,
            output,
        )
    }

    fn trigger(&self) {
        unsafe {
            if let Some(ref cb) = *self.cb_ptr {
                cb();
            }
        }
    }
}

impl Drop for SyncBackend {
    fn drop(&mut self) {
        unsafe {
            drop(Box::from_raw(self.cb_ptr));
        }
    }
}

type AudioIoResult<T> = Result<T, String>;

impl AudioIo for SyncBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn()>) {
        unsafe {
            *self.cb_ptr = Some(cb);
        }
    }

    fn read_input(&self, left: &mut [f32], right: &mut [f32]) -> usize {
        let frames = left.len().min(right.len());
        let mut buf = self.input.write();
        let mut temp = vec![0.0f32; frames * 2];
        let n = buf.read(&mut temp);
        drop(buf);
        let out = n / 2;
        for i in 0..out.min(frames) {
            left[i] = temp[i * 2];
            right[i] = temp[i * 2 + 1];
        }
        out
    }

    fn write_output(&self, left: &[f32], right: &[f32]) -> usize {
        let frames = left.len().min(right.len());
        let mut temp = vec![0.0f32; frames * 2];
        for i in 0..frames {
            temp[i * 2] = left[i];
            temp[i * 2 + 1] = right[i];
        }
        let mut buf = self.output.write();
        buf.write(&temp) / 2
    }

    fn start(&self) -> AudioIoResult<()> {
        Ok(())
    }

    fn stop(&self) -> AudioIoResult<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers: build a SineOsc → AudioOutput graph from a GraphDocument
// ---------------------------------------------------------------------------

#[cfg(feature = "serialization")]
fn make_sine_output_doc<const B: usize>() -> GraphDocument {
    GraphDocument {
        format_version: "rill/1".to_string(),
        sample_rate: RATE,
        block_size: B,
        resources: vec![],
        nodes: vec![
            NodeDef {
                id: 0,
                type_name: "rill/sine".into(),
                name: "osc".into(),
                parameters: [
                    ("freq".into(), ParamValue::Float(440.0)),
                    ("amp".into(), ParamValue::Float(0.5)),
                ]
                .into(),
            },
            NodeDef {
                id: 1,
                type_name: "rill/output".into(),
                name: "out".into(),
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
    }
}

#[cfg(feature = "serialization")]
fn build_pull_graph<const B: usize>(
    backend_ptr: AudioIoPtr,
    source_idx: usize,
) -> (*mut [NodeVariant<f32, B>], &'static mut AudioOutput<f32, B>) {
    let doc = make_sine_output_doc::<B>();
    let registry = registration::registry::<B>();
    let builder = doc.into_builder::<f32, B>(registry).expect("into_builder");
    let clock = Box::new(SystemClock::with_sample_rate(RATE));
    let graph = builder.build(clock).expect("graph build");
    let (mut nodes, topo, _tick) = graph.into_parts();

    let out_idx = topo
        .iter()
        .position(|&i| nodes[i].metadata().name == "AudioOutput")
        .expect("AudioOutput in graph");

    // Set backend on AudioOutput, set it active with source index.
    {
        if let NodeVariant::Sink(ref mut s) = nodes[out_idx] {
            let output: &mut AudioOutput<f32, B> = unsafe {
                &mut *(s.as_mut() as *mut dyn rill_adrift::rill_core::traits::Sink<f32, B>
                    as *mut AudioOutput<f32, B>)
            };
            output.set_backend(backend_ptr);
            output.set_active(source_idx);
        }
    }

    let nodes_ptr: *mut [NodeVariant<f32, B>] = Box::leak(nodes.into_boxed_slice());

    let audio_output: &mut AudioOutput<f32, B> = {
        let sink = &mut unsafe { &mut *nodes_ptr }[out_idx];
        if let NodeVariant::Sink(ref mut s) = sink {
            unsafe {
                &mut *(s.as_mut() as *mut dyn rill_adrift::rill_core::traits::Sink<f32, B>
                    as *mut AudioOutput<f32, B>)
            }
        } else {
            panic!("expected AudioOutput at index {out_idx}");
        }
    };

    (nodes_ptr, audio_output)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(feature = "serialization")]
#[test]
fn test_pull_model_sync_inject_and_verify() {
    const B: usize = 64;

    let (backend, _input_ring, output_ring) = SyncBackend::new(1024);
    let ptr = AudioIoPtr::from_ref(&backend);

    let (nodes_ptr, audio_output) = build_pull_graph::<B>(ptr, 0);

    let drain_fn: Box<dyn Fn(&mut [NodeVariant<f32, B>]) + Send> = Box::new(|_| {});

    // Start the pull model — callback stored in SyncBackend.
    audio_output.start(nodes_ptr, drain_fn, 48000.0);

    // Trigger one processing cycle:
    //   1. process_block(source_idx=0) → SineOsc::generate() fills its output port
    //   2. propagate from source output → data copied to AudioOutput input ports
    backend.trigger();

    // After propagate, data flows: SineOsc → propagate → AudioOutput.consume()
    // consume() reads from its input ports and writes to the backend's output ring.
    let mut temp = vec![0.0f32; B * 2];
    let n = output_ring.write().read(&mut temp);
    assert!(n > 0, "output ring should contain data after one cycle");

    let has_variation = temp[..n.min(B * 2)]
        .windows(2)
        .any(|w| (w[0] - w[1]).abs() > 0.001);
    assert!(has_variation, "sine output should vary between samples");

    let within_range = temp[..n.min(B * 2)].iter().all(|&s| s >= -1.0 && s <= 1.0);
    assert!(within_range, "sine samples should be in [-1, 1]");
}

#[cfg(all(feature = "serialization", feature = "alsa"))]
#[test]
fn test_alsa_pull_model() {
    use std::time::Duration;

    let has_loopback = std::process::Command::new("aplay")
        .args(["-l"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("Loopback"))
        .unwrap_or(false);

    if !has_loopback {
        eprintln!("SKIP: snd-aloop not loaded (try: sudo modprobe snd-aloop)");
        return;
    }

    // Create ALSA backend pointed at the loopback device.
    let config = rill_adrift::io::AudioConfig::default()
        .with_sample_rate(RATE as u32)
        .with_buffer_size(BUF as u32)
        .with_channels(2)
        .with_output_device("hw:Loopback,0,0");

    let backend = rill_adrift::io::AlsaBackend::new(config).unwrap();
    let ptr = AudioIoPtr::from_ref(&backend as &dyn AudioIo);

    let (nodes_ptr, audio_output) = build_pull_graph::<BUF>(ptr, 0);

    let drain_fn: Box<dyn Fn(&mut [NodeVariant<f32, BUF>]) + Send> = Box::new(|_| {});

    audio_output.start(nodes_ptr, drain_fn, RATE);

    // Let ALSA process a few blocks.
    std::thread::sleep(Duration::from_millis(500));

    // Verify the backend ran without xruns.
    let xruns = backend.xruns();
    assert_eq!(xruns, 0, "ALSA xruns during pull-model processing");

    audio_output.stop();
}
