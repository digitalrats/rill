//! Collector thread that drains probe/command queues and formats events.

use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use dashmap::DashMap;
use rill_core::queues::spsc::SpscQueue;
use rill_lang::debug::{CommandFrame, DebugControl, ProbeFrame, ProbeSlot};
use rill_lang::ir::ProbeId;

use super::formatter::{EventFormatter, JsonFormatter, TextFormatter};
use super::protocol::{AnalyzerCommand, AnalyzerConfig, AnalyzerResponse, OutputMode};
use super::state::{ProbeState, ProbeStateManager};

/// Manages the collector background thread.
pub struct CollectorThread {
    cmd_tx: mpsc::Sender<AnalyzerCommand>,
    resp_rx: mpsc::Receiver<AnalyzerResponse>,
}

impl CollectorThread {
    /// Spawn the collector thread and return a handle.
    pub fn spawn(
        config: AnalyzerConfig,
        probe_states: Arc<DashMap<ProbeId, ProbeState>>,
        probe_queues: Vec<Arc<SpscQueue<ProbeFrame, 64>>>,
        command_queue: Arc<SpscQueue<CommandFrame, 256>>,
        probe_slots: Vec<Arc<ProbeSlot>>,
        debug_control: DebugControl,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<AnalyzerCommand>();
        let (resp_tx, resp_rx) = mpsc::channel::<AnalyzerResponse>();

        let log_file = config.log_file.clone();
        let output_mode = config.output;

        thread::Builder::new()
            .name("rill-telemetry-collector".into())
            .spawn(move || {
                let manager = ProbeStateManager::new(probe_states, probe_slots, debug_control);
                let mut formatter: Box<dyn EventFormatter + Send> = match output_mode {
                    OutputMode::Text => Box::new(TextFormatter::new(log_file)),
                    OutputMode::Json => Box::new(JsonFormatter::new(log_file)),
                };

                loop {
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        if cmd == AnalyzerCommand::Quit {
                            let _ = resp_tx.send(AnalyzerResponse::Ok);
                            return;
                        }
                        let resp = manager.handle_command(cmd);
                        let _ = resp_tx.send(resp);
                    }

                    for (probe_id, queue) in probe_queues.iter().enumerate() {
                        while let Some(frame) = queue.pop() {
                            let name = manager.probe_name(probe_id as ProbeId);
                            formatter.format_probe(
                                probe_id as u32,
                                &name,
                                frame.value_bits,
                                frame.block_index,
                            );
                        }
                    }

                    while let Some(frame) = command_queue.pop() {
                        let param = frame.param_name.as_str();
                        let param_opt = if param.is_empty() { None } else { Some(param) };
                        formatter.format_command(
                            frame.block_index,
                            frame.command_kind.as_str(),
                            frame.node_name.as_str(),
                            param_opt,
                            frame.value_repr.as_str(),
                        );
                    }

                    formatter.flush();

                    thread::sleep(Duration::from_millis(5));
                }
            })
            .expect("failed to spawn collector thread");

        Self { cmd_tx, resp_rx }
    }

    /// Send a command to the collector thread.
    pub fn send(&self, cmd: AnalyzerCommand) -> Result<(), mpsc::SendError<AnalyzerCommand>> {
        self.cmd_tx.send(cmd)
    }

    /// Receive a response from the collector thread (non-blocking).
    pub fn try_recv(&self) -> Result<AnalyzerResponse, mpsc::TryRecvError> {
        self.resp_rx.try_recv()
    }

    /// Receive a response from the collector thread (blocking).
    pub fn recv(&self) -> Result<AnalyzerResponse, mpsc::RecvError> {
        self.resp_rx.recv()
    }
}
