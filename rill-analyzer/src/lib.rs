//! Interactive debugger and analyzer for Rill signal graphs.
use std::sync::{mpsc, Arc};

use dashmap::DashMap;
use rill_core::queues::spsc::SpscQueue;
use rill_lang::debug::{CommandFrame, DebugControl, ProbeFrame, ProbeSlot};
use rill_lang::ir::ProbeId;
use rill_telemetry::debug::collector_thread::CollectorThread;
use rill_telemetry::debug::protocol::{AnalyzerCommand, AnalyzerConfig, NodeInfo};
use rill_telemetry::debug::state::ProbeState;

pub mod lua;
pub mod prelude;
pub mod repl;

pub struct Analyzer {
    #[allow(dead_code)]
    collector: CollectorThread,
    repl_handle: Option<std::thread::JoinHandle<()>>,
    #[allow(dead_code)]
    cmd_tx: mpsc::Sender<AnalyzerCommand>,
}

impl Analyzer {
    /// Launch the analyzer with a collector thread and an interactive REPL.
    ///
    /// Spawns the collector thread on a dedicated OS thread and the REPL on
    /// another thread. Returns immediately — call [`wait`](Self::wait) to block
    /// until the REPL exits.
    pub fn launch(
        config: AnalyzerConfig,
        probes: Arc<DashMap<ProbeId, ProbeState>>,
        signal_queues: Vec<Arc<SpscQueue<ProbeFrame, 64>>>,
        command_queue: Arc<SpscQueue<CommandFrame, 256>>,
        probe_slots: Vec<Arc<ProbeSlot>>,
        debug_control: DebugControl,
        nodes: Vec<NodeInfo>,
    ) -> Self {
        let (resp_tx, resp_rx) = mpsc::channel();

        let (collector, cmd_tx) = CollectorThread::spawn(
            config,
            probes,
            signal_queues,
            command_queue,
            probe_slots,
            debug_control,
            resp_tx,
            None,
        );

        let repl_cmd_tx = cmd_tx.clone();
        let self_cmd_tx = cmd_tx.clone();
        let repl_handle = std::thread::spawn(move || {
            crate::repl::run(repl_cmd_tx, resp_rx, nodes);
        });

        Self {
            collector,
            repl_handle: Some(repl_handle),
            cmd_tx: self_cmd_tx,
        }
    }

    /// Block until the REPL thread exits (user types `quit`).
    pub fn wait(mut self) {
        if let Some(h) = self.repl_handle.take() {
            let _ = h.join();
        }
    }
}
