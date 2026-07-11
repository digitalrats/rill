//! Probe state manager for handling analyzer commands.

use std::sync::Arc;

use dashmap::DashMap;
use rill_lang::debug::{DebugControl, ProbeSlot};
use rill_lang::ir::ProbeId;

use super::protocol::{AnalyzerCommand, AnalyzerResponse, ProbeInfo};

/// Human-readable metadata for a probe.
pub struct ProbeState {
    pub name: String,
    pub node_name: String,
}

/// Manages probe state and processes analyzer commands.
pub struct ProbeStateManager {
    probe_states: Arc<DashMap<ProbeId, ProbeState>>,
    probe_slots: Vec<Arc<ProbeSlot>>,
    debug_control: DebugControl,
}

impl ProbeStateManager {
    /// Create a new ProbeStateManager.
    pub fn new(
        probe_states: Arc<DashMap<ProbeId, ProbeState>>,
        probe_slots: Vec<Arc<ProbeSlot>>,
        debug_control: DebugControl,
    ) -> Self {
        Self {
            probe_states,
            probe_slots,
            debug_control,
        }
    }

    /// Process an analyzer command and return the response.
    pub fn handle_command(&self, cmd: AnalyzerCommand) -> AnalyzerResponse {
        match cmd {
            AnalyzerCommand::SetBreakpoint { probe_id } => {
                if let Some(slot) = self.probe_slots.get(probe_id as usize) {
                    slot.enabled.store(true, std::sync::atomic::Ordering::Release);
                    slot.break_flag
                        .store(true, std::sync::atomic::Ordering::Release);
                    AnalyzerResponse::Ok
                } else {
                    AnalyzerResponse::Error(format!("Unknown probe {}", probe_id))
                }
            }
            AnalyzerCommand::ClearBreakpoint { probe_id } => {
                if let Some(slot) = self.probe_slots.get(probe_id as usize) {
                    slot.break_flag
                        .store(false, std::sync::atomic::Ordering::Release);
                    AnalyzerResponse::Ok
                } else {
                    AnalyzerResponse::Error(format!("Unknown probe {}", probe_id))
                }
            }
            AnalyzerCommand::Continue => {
                self.debug_control.cont();
                AnalyzerResponse::Ok
            }
            AnalyzerCommand::Step => {
                self.debug_control.cont();
                AnalyzerResponse::Ok
            }
            AnalyzerCommand::Pause => {
                self.debug_control.pause();
                AnalyzerResponse::Paused
            }
            AnalyzerCommand::GetProbeValue { probe_id } => {
                if let Some(slot) = self.probe_slots.get(probe_id as usize) {
                    let value_bits =
                        slot.last_value.load(std::sync::atomic::Ordering::Acquire);
                    AnalyzerResponse::ProbeValue {
                        probe_id,
                        value_bits,
                    }
                } else {
                    AnalyzerResponse::Error(format!("Unknown probe {}", probe_id))
                }
            }
            AnalyzerCommand::GetProbeValues => {
                let values: Vec<(ProbeId, u64)> = self
                    .probe_slots
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.enabled.load(std::sync::atomic::Ordering::Acquire))
                    .map(|(i, s)| {
                        (
                            i as ProbeId,
                            s.last_value.load(std::sync::atomic::Ordering::Acquire),
                        )
                    })
                    .collect();
                AnalyzerResponse::ProbeValues(values)
            }
            AnalyzerCommand::ListNodes => {
                AnalyzerResponse::Error("ListNodes not yet implemented".to_string())
            }
            AnalyzerCommand::ListProbes => {
                let probes: Vec<ProbeInfo> = self
                    .probe_states
                    .iter()
                    .map(|entry| {
                        let state = entry.value();
                        let probe_id = *entry.key();
                        let (enabled, has_breakpoint) =
                            if let Some(slot) = self.probe_slots.get(probe_id as usize) {
                                (
                                    slot.enabled
                                        .load(std::sync::atomic::Ordering::Acquire),
                                    slot.break_flag
                                        .load(std::sync::atomic::Ordering::Acquire),
                                )
                            } else {
                                (false, false)
                            };
                        ProbeInfo {
                            probe_id,
                            name: state.name.clone(),
                            node_name: state.node_name.clone(),
                            enabled,
                            has_breakpoint,
                        }
                    })
                    .collect();
                AnalyzerResponse::ProbeList(probes)
            }
            AnalyzerCommand::ListCommands => {
                AnalyzerResponse::Error("ListCommands not yet implemented".to_string())
            }
            AnalyzerCommand::EnableProbe { probe_id } => {
                if let Some(slot) = self.probe_slots.get(probe_id as usize) {
                    slot.enabled.store(true, std::sync::atomic::Ordering::Release);
                    AnalyzerResponse::Ok
                } else {
                    AnalyzerResponse::Error(format!("Unknown probe {}", probe_id))
                }
            }
            AnalyzerCommand::DisableProbe { probe_id } => {
                if let Some(slot) = self.probe_slots.get(probe_id as usize) {
                    slot.enabled.store(false, std::sync::atomic::Ordering::Release);
                    slot.break_flag
                        .store(false, std::sync::atomic::Ordering::Release);
                    AnalyzerResponse::Ok
                } else {
                    AnalyzerResponse::Error(format!("Unknown probe {}", probe_id))
                }
            }
            AnalyzerCommand::Quit => AnalyzerResponse::Ok,
        }
    }
}
