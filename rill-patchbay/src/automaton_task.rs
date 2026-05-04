//! # Automaton Task — обёртка Automaton trait в green thread
//!
//! Позволяет запустить любой `Automaton` как независимый tokio task
//! с собственным интервалом тиков. Значения отправляются в `PortCombiner`
//! через mpsc-канал.

use std::time::Duration;

use tokio::sync::mpsc;
use tokio::sync::watch;

use crate::control::{Automaton, Time};

/// Запустить автомат как green thread (tokio task)
///
/// # Arguments
///
/// * `automaton` — реализация `Automaton` trait
/// * `interval` — частота обновления (например, 10 мс для 100 Hz)
/// * `value_tx` — канал для отправки значений в PortCombiner
/// * `cancel_rx` — сигнал отмены (из PortCombinerHandle::cancel_rx)
///
/// Возвращает `JoinHandle` задачи. При дропе хэндла задача продолжает
/// работать. Для остановки используется сигнал отмены.
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
    let mut state = automaton.initial_state();
    let mut time: Time = 0.0;
    let mut ticker = tokio::time::interval(interval);
    // Пропускаем первый тик (немедленный)
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                time += interval.as_secs_f64();
                let (new_state, value_opt) = automaton.step(time, &A::Action::default(), &state);
                if let Some(value) = value_opt {
                    if value_tx.send(value).await.is_err() {
                        // Канал закрыт — PortCombiner остановлен
                        break;
                    }
                }
                state = new_state;
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
// Тесты
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

        // Должны получить несколько значений
        for _ in 0..3 {
            let val = tokio::time::timeout(Duration::from_millis(50), value_rx.recv()).await;
            assert!(val.is_ok(), "task should produce values");
            let v = val.unwrap().unwrap();
            assert!(v >= -1.0 && v <= 1.0, "value {} out of range", v);
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
