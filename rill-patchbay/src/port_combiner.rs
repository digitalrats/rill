//! # PortCombiner — combining automaton and UI values
//!
//! Each active control port of a node can have its own `PortCombiner` —
//! a lightweight tokio task that:
//!
//! - Receives values from the automaton (green thread)
//! - Receives commands from UI (event mapping)
//! - Applies control and conflict resolution strategies
//! - Sends the final `ParameterCommand` to the audio thread

#[cfg(test)]
use rill_core::queues::MpscQueue;
use rill_core::queues::{SetParameter, SignalOrigin};
use rill_core::traits::{NodeId, ParamValue, ParameterId, PortId};
use rill_core_actor::ActorRef;
#[cfg(test)]
use std::sync::Arc;

use tokio::sync::{mpsc, watch};

use crate::strategy::{ConflictStrategy, ControlStrategy, UiCommand};

/// Handle for controlling PortCombiner from outside
pub struct PortCombinerHandle {
    /// Channel for sending automaton values
    pub automaton_tx: mpsc::Sender<f64>,
    /// Channel for sending UI commands
    pub ui_tx: mpsc::UnboundedSender<UiCommand>,
    /// Channel for cancellation signal
    cancel_tx: watch::Sender<bool>,
    /// Combiner task JoinHandle
    _handle: tokio::task::JoinHandle<()>,
}

impl PortCombinerHandle {
    /// Stop the combiner
    pub fn stop(&self) {
        let _ = self.cancel_tx.send(true);
    }

    /// Get the cancellation signal receiver (passed to the automaton)
    pub fn cancel_rx(&self) -> watch::Receiver<bool> {
        self.cancel_tx.subscribe()
    }
}

/// Start PortCombiner for a (node, parameter) pair
///
/// # Arguments
///
/// * `target` — (node ID, parameter name)
/// * `range` — (min, max) parameter value range
/// * `control` — control strategy (Absolute / Modulation)
/// * `conflict` — conflict resolution strategy
/// * `output_queue` — queue for sending commands to the audio thread
pub fn spawn_combiner(
    target: (NodeId, String),
    range: (f64, f64),
    control: ControlStrategy,
    conflict: ConflictStrategy,
    output_queue: ActorRef<SetParameter>,
) -> PortCombinerHandle {
    let (automaton_tx, automaton_rx) = mpsc::channel::<f64>(16);
    let (ui_tx, ui_rx) = mpsc::unbounded_channel::<UiCommand>();
    let (cancel_tx, cancel_rx) = watch::channel(false);

    let handle = tokio::spawn(combiner_loop(
        automaton_rx,
        ui_rx,
        cancel_rx,
        target,
        range,
        control,
        conflict,
        output_queue,
    ));

    PortCombinerHandle {
        automaton_tx,
        ui_tx,
        cancel_tx,
        _handle: handle,
    }
}

// ---------------------------------------------------------------------------
// Internal implementation
// ---------------------------------------------------------------------------

async fn combiner_loop(
    mut automaton_rx: mpsc::Receiver<f64>,
    mut ui_rx: mpsc::UnboundedReceiver<UiCommand>,
    mut cancel_rx: watch::Receiver<bool>,
    target: (NodeId, String),
    range: (f64, f64),
    control: ControlStrategy,
    conflict: ConflictStrategy,
    output_queue: ActorRef<SetParameter>,
) {
    let (node_id, param_name) = target;
    let (min, max) = range;
    let mut base = center(min, max);
    let mut frozen = false;
    let mut latest_mod = 0.0;

    loop {
        tokio::select! {
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() {
                    break;
                }
            }

            Some(mod_val) = automaton_rx.recv() => {
                latest_mod = mod_val;
                if frozen {
                    continue;
                }

                let value = combine(mod_val, base, control, min, max);
                let pid = ParameterId::new(&param_name).unwrap();
                output_queue.send(SetParameter::new(
                    PortId::param(node_id, 0), pid, ParamValue::Float(value as f32), SignalOrigin::Manual,
                ));
            }

            Some(cmd) = ui_rx.recv() => {
                match (cmd, conflict) {
                    (UiCommand::SetValue(v), ConflictStrategy::TouchOverride) => {
                        base = v;
                        frozen = true;
                        let pid = ParameterId::new(&param_name).unwrap();
                        output_queue.send(SetParameter::new(
                            PortId::param(node_id, 0), pid, ParamValue::Float(v as f32), SignalOrigin::Manual,
                        ));
                    }

                    (UiCommand::SetValue(v), ConflictStrategy::BasePlusModulation) => {
                        base = v;
                        let value = combine(latest_mod, v, control, min, max);
                        let pid = ParameterId::new(&param_name).unwrap();
                        output_queue.send(SetParameter::new(
                            PortId::param(node_id, 0), pid, ParamValue::Float(value as f32), SignalOrigin::Manual,
                        ));
                    }

                    (UiCommand::SetValue(v), ConflictStrategy::LastWriteWins) => {
                        let pid = ParameterId::new(&param_name).unwrap();
                        output_queue.send(SetParameter::new(
                            PortId::param(node_id, 0), pid, ParamValue::Float(v as f32), SignalOrigin::Manual,
                        ));
                    }

                    (UiCommand::Release, ConflictStrategy::TouchOverride) => {
                        frozen = false;
                        let value = combine(latest_mod, base, control, min, max);
                        let pid = ParameterId::new(&param_name).unwrap();
                        output_queue.send(SetParameter::new(
                            PortId::param(node_id, 0), pid, ParamValue::Float(value as f32), SignalOrigin::Manual,
                        ));
                    }

                    (UiCommand::Release, _) => {
                        // Other strategies ignore Release
                    }
                }
            }

            else => break,
        }
    }
}

/// Combine automaton value with base according to strategy
fn combine(mod_val: f64, base: f64, control: ControlStrategy, min: f64, max: f64) -> f64 {
    match control {
        ControlStrategy::Absolute => {
            // mod_val expected in [0, 1] → map to [min, max]
            min + mod_val * (max - min)
        }
        ControlStrategy::Modulation { depth } => {
            // mod_val expected in [-1, 1] → modulate around base
            let value = base + mod_val * depth * (max - min);
            value.clamp(min, max)
        }
    }
}

fn center(min: f64, max: f64) -> f64 {
    (min + max) / 2.0
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::ControlStrategy;

    #[test]
    fn test_combine_absolute() {
        let result = combine(0.5, 0.0, ControlStrategy::Absolute, 0.0, 1.0);
        assert!((result - 0.5).abs() < 1e-9);

        let result = combine(0.0, 0.0, ControlStrategy::Absolute, 100.0, 1000.0);
        assert!((result - 100.0).abs() < 1e-9);

        let result = combine(1.0, 0.0, ControlStrategy::Absolute, 100.0, 1000.0);
        assert!((result - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn test_combine_modulation() {
        let strategy = ControlStrategy::Modulation { depth: 1.0 };

        // mod = 0.0 → base
        let result = combine(0.0, 500.0, strategy, 0.0, 1000.0);
        assert!((result - 500.0).abs() < 1e-9);

        // mod = 1.0 → base + depth * range = 500 + 1000 = 1500 → clamped to 1000
        let result = combine(1.0, 500.0, strategy, 0.0, 1000.0);
        assert!((result - 1000.0).abs() < 1e-9);

        // mod = -1.0 → base - depth * range = 500 - 1000 = -500 → clamped to 0
        let result = combine(-1.0, 500.0, strategy, 0.0, 1000.0);
        assert!((result - 0.0).abs() < 1e-9);

        // depth = 0.0 → always base
        let shallow = ControlStrategy::Modulation { depth: 0.0 };
        let result = combine(1.0, 300.0, shallow, 0.0, 1000.0);
        assert!((result - 300.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn test_combiner_absolute_touch_override() {
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let actor_ref = ActorRef::new(&mailbox);
        let handle = spawn_combiner(
            (NodeId(1), "cutoff".into()),
            (100.0, 1000.0),
            ControlStrategy::Absolute,
            ConflictStrategy::TouchOverride,
            actor_ref,
        );

        // Automaton sends a value
        handle.automaton_tx.send(0.5).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        assert!(!mailbox.is_empty());
        let cmd = mailbox.pop().unwrap();
        assert!((cmd.value.as_f32().unwrap() - 550.0).abs() < 1.0);

        // UI touches
        handle.ui_tx.send(UiCommand::SetValue(800.0)).unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let cmd = mailbox.pop().unwrap();
        assert!((cmd.value.as_f32().unwrap() - 800.0).abs() < 1.0);

        // Automaton sends a new value — it is ignored (frozen)
        handle.automaton_tx.send(0.1).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        // Queue should not have a new value from the automaton
        assert!(mailbox.is_empty());
    }

    #[tokio::test]
    async fn test_combiner_modulation_base_plus() {
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let actor_ref = ActorRef::new(&mailbox);
        let handle = spawn_combiner(
            (NodeId(1), "cutoff".into()),
            (100.0, 1000.0),
            ControlStrategy::Modulation { depth: 0.5 },
            ConflictStrategy::BasePlusModulation,
            actor_ref,
        );

        // UI sets the base (mod_val initially center ~ 550)
        handle.ui_tx.send(UiCommand::SetValue(500.0)).unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        // BasePlusModulation: combine(center, 500, Modulation, ...) = 500 + 0 * ...
        let cmd = mailbox.pop().unwrap();
        assert!((cmd.value.as_f32().unwrap() - 500.0).abs() < 1.0);

        // Automaton sends modulation
        handle.automaton_tx.send(0.5).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        // value = 500 + 0.5 * 0.5 * 900 = 500 + 225 = 725
        let cmd = mailbox.pop().unwrap();
        assert!((cmd.value.as_f32().unwrap() - 725.0).abs() < 1.0);
    }

    #[tokio::test]
    async fn test_combiner_last_write_wins() {
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let actor_ref = ActorRef::new(&mailbox);
        let handle = spawn_combiner(
            (NodeId(1), "gain".into()),
            (0.0, 1.0),
            ControlStrategy::Absolute,
            ConflictStrategy::LastWriteWins,
            actor_ref,
        );

        // UI writes
        handle.ui_tx.send(UiCommand::SetValue(0.8)).unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let cmd1 = mailbox.pop().unwrap();
        assert!((cmd1.value.as_f32().unwrap() - 0.8).abs() < 1e-6);

        // Automaton writes
        handle.automaton_tx.send(0.3).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let cmd2 = mailbox.pop().unwrap();
        assert!((cmd2.value.as_f32().unwrap() - 0.3).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_combiner_release_unfreezes() {
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let actor_ref = ActorRef::new(&mailbox);
        let handle = spawn_combiner(
            (NodeId(1), "cutoff".into()),
            (100.0, 1000.0),
            ControlStrategy::Absolute,
            ConflictStrategy::TouchOverride,
            actor_ref,
        );

        // UI touches → frozen
        handle.ui_tx.send(UiCommand::SetValue(800.0)).unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        mailbox.pop(); // drain UI value

        // Release
        handle.ui_tx.send(UiCommand::Release).unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        mailbox.pop(); // drain re-emit

        // Now the automaton works again
        handle.automaton_tx.send(0.2).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let cmd = mailbox.pop().unwrap();
        assert!((cmd.value.as_f32().unwrap() - 280.0).abs() < 1.0);
    }
}
