//! # Automaton Task — wrapping the Automaton trait in a green thread
//!
//! Allows running any `Automaton` as an independent tokio task
//! with its own tick interval. Values are sent via an mpsc channel.
//! (`PortCombiner` was removed — it duplicated `CommandEnum`.)

use std::time::Duration;

use tokio::sync::mpsc;
use tokio::sync::watch;

use crate::engine::{Automaton, Time};
use rill_core::traits::ParamValue;

/// Run an automaton as a green thread (tokio task)
///
/// # Arguments
///
/// * `automaton` — implementation of the `Automaton` trait
/// * `interval` — update frequency (e.g. 10 ms for 100 Hz)
/// * `value_tx` — channel for sending computed values
/// * `cancel_rx` — cancellation signal
///
/// Returns a `JoinHandle`. Dropping the handle does not stop the
/// task. Use the cancellation signal to stop it.
pub fn spawn_automaton_task<A>(
    automaton: A,
    interval: Duration,
    value_tx: mpsc::Sender<f64>,
    cancel_rx: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()>
where
    A: Automaton + 'static,
{
    tokio::spawn(automaton_loop(automaton, interval, value_tx, cancel_rx))
}

async fn automaton_loop<A>(
    automaton: A,
    interval: Duration,
    value_tx: mpsc::Sender<f64>,
    mut cancel_rx: watch::Receiver<bool>,
) where
    A: Automaton,
{
    let mut internal = automaton.initial_internal();
    let mut current = ParamValue::Float(0.0);
    let mut time: Time = 0.0;
    let mut ticker = tokio::time::interval(interval);
    // Skip the first tick (immediate)
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                time += interval.as_secs_f64();
                current = automaton.step(&mut internal, &current, time, &A::Action::default());
                let value = current.as_f32().unwrap_or(0.0) as f64;
                if value_tx.send(value).await.is_err() {
                    // Channel closed — receiver dropped
                    break;
                }
            }

            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() {
                    break;
                }
            }
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automaton::LfoAutomaton;
    use crate::automaton::LfoWaveform;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_lfo_task_produces_values() {
        let lfo = LfoAutomaton::new("test", 10.0, 1.0, 0.0, LfoWaveform::Sine);
        let (value_tx, mut value_rx) = mpsc::channel::<f64>(16);
        let (cancel_tx, cancel_rx) = watch::channel(false);

        let _handle = spawn_automaton_task(lfo, Duration::from_millis(10), value_tx, cancel_rx);

        // Should receive several values
        for _ in 0..3 {
            let val = tokio::time::timeout(Duration::from_millis(50), value_rx.recv()).await;
            assert!(val.is_ok(), "task should produce values");
            let v = val.unwrap().unwrap();
            assert!((-1.0..=1.0).contains(&v), "value {} out of range", v);
        }

        let _ = cancel_tx.send(true);
    }

    #[tokio::test]
    async fn test_task_stops_on_cancel() {
        let lfo = LfoAutomaton::new("test", 10.0, 1.0, 0.0, LfoWaveform::Sine);
        let (value_tx, _value_rx) = mpsc::channel::<f64>(16);
        let (cancel_tx, cancel_rx) = watch::channel(false);

        let handle = spawn_automaton_task(lfo, Duration::from_millis(10), value_tx, cancel_rx);

        let _ = cancel_tx.send(true);
        let result = tokio::time::timeout(Duration::from_millis(100), handle).await;
        assert!(result.is_ok(), "task should stop on cancel");
    }
}
