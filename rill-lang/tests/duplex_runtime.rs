//! Duplex stream test: `Runtime::launch_stream` drives a tape-echo through a
//! two-pass callback (recording then playback over the shared tape).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rill_core::io::{IoCapture, IoDriver, IoPlayback, IoResult};
use rill_core::time::ClockTick;
use rill_lang::builtin::Registry;
use rill_lang::graph::compile;
use rill_lang::graph::spec::*;
use rill_lang::runtime::Runtime;

const BUF: usize = 256;

fn test_registry() -> Registry<f32> {
    let mut reg = Registry::new();
    rill_core_dsp::lang::register::register_lang_builtins(&mut reg);
    rill_lang::register::register_core_builtins(&mut reg);
    rill_router::register::register_lang_builtins(&mut reg);
    rill_sampler::tape::lang::register_tape_builtins(&mut reg);
    reg
}

fn node(
    type_name: &str,
    params: &[(&str, f64)],
    backend: Option<NodeBackendKind>,
) -> GraphSpecNode {
    GraphSpecNode {
        type_name: type_name.to_string(),
        params: params
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect::<HashMap<_, _>>(),
        backend,
    }
}

fn edge(
    from: usize,
    from_port: usize,
    to: usize,
    to_port: usize,
    kind: GraphEdgeKind,
) -> GraphSpecEdge {
    GraphSpecEdge {
        from,
        from_port,
        to,
        to_port,
        kind,
    }
}

fn moonlight_spec() -> GraphSpec {
    GraphSpec {
        nodes: vec![
            node("mixer", &[], None), // 0 stereo_sum
            node(
                "write_head",
                &[("delay_time", 0.5), ("feedback", 0.35)],
                Some(NodeBackendKind::Passive),
            ), // 1
            node(
                "read_head",
                &[("delay", 0.05)],
                Some(NodeBackendKind::Passive),
            ), // 2
            node("mixer", &[], None), // 3 tap_mixer
            node("dry_wet", &[], None), // 4 mixL
            node("dry_wet", &[], None), // 5 mixR
            node(
                "biquad",
                &[("filter", 1.0), ("cutoff", 1800.0), ("q", 0.7)],
                None,
            ), // 6 fb_lp
        ],
        edges: vec![
            edge(0, 0, 1, 0, GraphEdgeKind::Signal), // stereo_sum.L -> write_head.dry
            edge(2, 0, 3, 0, GraphEdgeKind::Signal), // read_head -> tap_mixer
            edge(3, 0, 4, 0, GraphEdgeKind::Signal), // tap_mixer.L -> mixL.wet
            edge(3, 1, 5, 0, GraphEdgeKind::Signal), // tap_mixer.R -> mixR.wet
            edge(3, 0, 6, 0, GraphEdgeKind::Signal), // tap_mixer.L -> fb_lp
            edge(6, 0, 1, 1, GraphEdgeKind::Feedback), // fb_lp -> write_head.feedback
            edge(0, 0, 4, 0, GraphEdgeKind::Signal), // stereo_sum.L -> mixL.dry (dry cross)
            edge(0, 1, 5, 0, GraphEdgeKind::Signal), // stereo_sum.R -> mixR.dry (dry cross)
        ],
        resources: vec![GraphResourceSpec {
            name: "tape_0".into(),
            kind: "tape".into(),
            capacity: 96000,
        }],
        sample_rate: 44100.0,
        backends: vec![
            BackendAttachment {
                input: true,
                backend_name: "pipewire".into(),
                node: 0,
                port: 0,
            },
            BackendAttachment {
                input: false,
                backend_name: "pipewire".into(),
                node: 4,
                port: 0,
            },
            BackendAttachment {
                input: false,
                backend_name: "pipewire".into(),
                node: 5,
                port: 0,
            },
        ],
        boundary_out: Vec::new(),
        input_ports: Vec::new(),
    }
}

// ── mock I/O ────────────────────────────────────────────────────────────────

struct MockCapture {
    value: f32,
}
impl IoCapture for MockCapture {
    fn read_input(&self, _ch: usize, dst: &mut [f32]) -> usize {
        for s in dst.iter_mut() {
            *s = self.value;
        }
        dst.len()
    }
    fn num_input_channels(&self) -> usize {
        2
    }
}

#[derive(Default)]
struct MockPlayback {
    max_abs: Mutex<f32>,
}
impl IoPlayback for MockPlayback {
    fn write_output(&self, _ch: usize, src: &[f32]) -> usize {
        let mut peak = 0.0f32;
        for &s in src {
            if s.abs() > peak {
                peak = s.abs();
            }
        }
        let mut m = self.max_abs.lock().unwrap();
        if peak > *m {
            *m = peak;
        }
        src.len()
    }
    fn num_output_channels(&self) -> usize {
        2
    }
}

struct CbSlot(usize);
impl CbSlot {
    fn new() -> Self {
        Self(Box::into_raw(Box::new(None::<Box<dyn FnMut(&ClockTick)>>)) as usize)
    }
    unsafe fn set(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        *(self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>) = Some(cb);
    }
    unsafe fn call(&self, tick: &ClockTick) {
        if let Some(cb) = &mut *(self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>) {
            cb(tick);
        }
    }
}

struct MockDriver {
    process: CbSlot,
    iters: u64,
}
unsafe impl Send for MockDriver {}
unsafe impl Sync for MockDriver {}
impl Drop for MockDriver {
    fn drop(&mut self) {
        unsafe { self.process.free() }
    }
}
impl CbSlot {
    unsafe fn free(&self) {
        drop(Box::from_raw(
            self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>,
        ));
    }
}
impl IoDriver for MockDriver {
    fn set_callback(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        unsafe { self.process.set(cb) }
    }
    fn run(&self, running: Arc<AtomicBool>) -> IoResult<()> {
        for i in 0..self.iters {
            let tick = ClockTick::new(i * BUF as u64, BUF as u32, 44100.0, "mock".into());
            unsafe { self.process.call(&tick) }
        }
        running.store(false, Ordering::Release);
        Ok(())
    }
    fn stop(&self) -> IoResult<()> {
        Ok(())
    }
}

#[test]
fn duplex_stream_delivers_signal() {
    let spec = moonlight_spec();
    let stream = compile(&spec, &test_registry(), 44100.0).unwrap();
    let capture = Arc::new(MockCapture { value: 0.3 });
    let playback = Arc::new(MockPlayback::default());
    let driver = Arc::new(MockDriver {
        process: CbSlot::new(),
        iters: 400,
    });
    let running = Arc::new(AtomicBool::new(true));
    Runtime::launch_stream::<BUF>(
        driver,
        Some(capture),
        Some(playback.clone()),
        stream,
        running,
    )
    .unwrap();
    let out_max = *playback.max_abs.lock().unwrap();
    println!("duplex output max abs = {out_max:.6}");
    assert!(out_max > 0.05, "signal did not reach the output: {out_max}");
}
