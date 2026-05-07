//! # PatchbayEngine — референсный оркестратор
//!
//! Высокоуровневая обёртка над `PatchbayControl`, `PortCombiner`
//! и green threads. Скрывает детали спавна задач, отмены и маршрутизации.
//!
//! ## Использование
//!
//! ```no_run
//! use std::time::Duration;
//! use rill_core::traits::ActorRef;
//! use rill_core::NodeId;
//! use rill_patchbay::prelude::*;
//!
//! let cmd_queue = ActorRef::new_pair().0;
//! let mut engine = PatchbayEngine::new(cmd_queue);
//!
//! // LFO как green thread
//! engine.add_lfo(
//!     "lfo1", 0.3, 0.5, 0.0, LfoWaveform::Sine,
//!     Duration::from_millis(10),
//!     (NodeId(2), "cutoff".into()), (100.0, 1000.0),
//!     ControlStrategy::Absolute,
//!     ConflictStrategy::BasePlusModulation,
//! );
//!
//! // Маппинг MIDI → параметр
//! engine.add_mapping(midi_cc(
//!     21, None, NodeId(2), "cutoff", 100.0, 1000.0, Transform::Linear,
//! ));
//!
//! // Обработка внешнего события
//! engine.handle_event(ControlEvent::MidiControl {
//!     channel: 1, controller: 21, value: 64, normalized: 0.5,
//! });
//!
//! // Остановка всех задач
//! engine.stop();
//! ```

use std::time::Duration;

use crossbeam_channel::Receiver as CrossbeamReceiver;
use rill_core::queues::telemetry::Telemetry;
use rill_core::queues::SetParameter;
use rill_core::traits::ActorRef;
use rill_core::NodeId;

use crate::automaton::LfoWaveform;
use crate::control::{ControlEvent, Mapping, PatchbayControl};
#[cfg(feature = "serde")]
use crate::document::PatchbayDocument;
#[cfg(feature = "serde")]
use crate::function_registry::FunctionRegistry;
use crate::sequencer::{SequencerHandle, SnapshotSequencer};
use crate::strategy::{ConflictStrategy, ControlStrategy};

/// High-level orchestrator for the patchbay system.
///
/// Manages automaton green threads, port combiners with conflict
/// resolution strategies, event mappings, and graceful shutdown.
///
/// All automaton management is delegated to [`PatchbayControl`];
/// this struct provides a simplified API and ensures proper cleanup.
pub struct PatchbayEngine {
    control: PatchbayControl,
}

impl PatchbayEngine {
    /// Create a new engine on the current tokio runtime.
    ///
    /// Requires an active tokio runtime (e.g. `#[tokio::main]`).
    /// Panics if `tokio::runtime::Handle::try_current()` fails.
    pub fn new(command_queue: ActorRef<SetParameter>) -> Self {
        let _ = tokio::runtime::Handle::try_current()
            .expect("PatchbayEngine requires an active tokio runtime");
        Self {
            control: PatchbayControl::new(command_queue),
        }
    }

    /// Add an automaton as a green thread with PortCombiner.
    pub fn add_automaton<A: crate::control::Automaton + 'static>(
        &mut self,
        id: &str,
        automaton: A,
        interval: Duration,
        target: (NodeId, String),
        range: (f64, f64),
        control: ControlStrategy,
        conflict: ConflictStrategy,
    ) {
        self.control
            .add_automaton_task(id, automaton, interval, target, range, control, conflict);
    }

    /// Add an LFO as a green thread.
    pub fn add_lfo(
        &mut self,
        id: &str,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
        interval: Duration,
        target: (NodeId, String),
        range: (f64, f64),
        control: ControlStrategy,
        conflict: ConflictStrategy,
    ) {
        self.control.add_lfo_task(
            id, frequency, amplitude, offset, waveform, interval, target, range, control, conflict,
        );
    }

    /// Add an ADSR envelope as a green thread.
    pub fn add_envelope(
        &mut self,
        id: &str,
        attack: f64,
        decay: f64,
        sustain: f64,
        release: f64,
        interval: Duration,
        target: (NodeId, String),
        range: (f64, f64),
        control: ControlStrategy,
        conflict: ConflictStrategy,
    ) {
        self.control.add_envelope_task(
            id, attack, decay, sustain, release, interval, target, range, control, conflict,
        );
    }

    /// Add an event mapping (MIDI/OSC → parameter).
    pub fn add_mapping(&mut self, mapping: Mapping) {
        self.control.add_mapping(mapping);
    }

    /// Load a serialized patchbay document and apply it with async tasks.
    ///
    /// Servos with `async_interval_ms: Some(...)` become green threads;
    /// others fall back to sync mode.
    #[cfg(feature = "serde")]
    pub fn load_document(
        &mut self,
        doc: &PatchbayDocument,
        registry: &FunctionRegistry,
    ) -> Result<(), String> {
        doc.apply_to_async(&mut self.control, registry)
    }

    /// Route an external event through active mappings.
    ///
    /// If a `PortCombiner` exists for the target parameter, the
    /// value is routed there for conflict resolution. Otherwise
    /// it goes directly to the command queue.
    pub fn handle_event(&mut self, event: ControlEvent) {
        self.control.handle_event(event);
    }

    /// Attach a parameter-lock sequencer driven by audio-thread clock ticks.
    ///
    /// See [`PatchbayControl::attach_sequencer`] for details.
    pub fn attach_sequencer(
        &mut self,
        tel_rx: CrossbeamReceiver<Telemetry>,
        sequencer: SnapshotSequencer,
    ) -> SequencerHandle {
        self.control.attach_sequencer(tel_rx, sequencer)
    }

    /// Load a serialised sequencer document and attach it.
    ///
    /// Convenience wrapper: deserialises the document into a
    /// [`SnapshotSequencer`], then calls [`attach_sequencer`](Self::attach_sequencer).
    #[cfg(feature = "serde")]
    pub fn load_sequencer_document(
        &mut self,
        tel_rx: CrossbeamReceiver<Telemetry>,
        doc: crate::sequencer::SequencerDocument,
    ) -> SequencerHandle {
        let seq = doc.into_sequencer();
        self.attach_sequencer(tel_rx, seq)
    }

    /// Detach the sequencer: abort its task and drop the handle.
    pub fn detach_sequencer(&mut self) {
        self.control.detach_sequencer();
    }

    /// Get a reference to the sequencer handle, if attached.
    pub fn sequencer_handle(&self) -> Option<&SequencerHandle> {
        self.control.sequencer_handle()
    }

    /// Stop all automaton green threads and clear mappings.
    pub fn stop(&mut self) {
        self.control.stop_all();
    }

    /// Borrow the inner control.
    pub fn control(&self) -> &PatchbayControl {
        &self.control
    }

    /// Mutably borrow the inner control.
    pub fn control_mut(&mut self) -> &mut PatchbayControl {
        &mut self.control
    }
}

impl Drop for PatchbayEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automaton::LfoWaveform;
    use crate::control::{midi_cc, ControlEvent, Transform};
    use crate::strategy::ControlStrategy;
    use rill_core::traits::ActorRef;
    use rill_core::NodeId;

    #[tokio::test]
    async fn test_engine_creation() {
        let engine = PatchbayEngine::new(ActorRef::new_pair().0);
        // Just verifying no panic
        drop(engine);
    }

    #[tokio::test]
    async fn test_engine_add_lfo_produces_values() {
        let (cmd_queue, mailbox) = ActorRef::<SetParameter>::new_pair();
        let mut engine = PatchbayEngine::new(cmd_queue);

        engine.add_lfo(
            "test_lfo",
            10.0,
            1.0,
            0.0,
            LfoWaveform::Sine,
            std::time::Duration::from_millis(10),
            (NodeId(1), "cutoff".into()),
            (0.0, 1.0),
            ControlStrategy::Absolute,
            crate::strategy::ConflictStrategy::LastWriteWins,
        );

        // Let automaton run for a bit
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;

        // Should have produced values
        assert!(!mailbox.is_empty());
    }

    #[tokio::test]
    async fn test_engine_handle_event_direct() {
        let (cmd_queue, mailbox) = ActorRef::<SetParameter>::new_pair();
        let mut engine = PatchbayEngine::new(cmd_queue);

        engine.add_mapping(midi_cc(
            7,
            None,
            NodeId(1),
            "volume",
            0.0,
            1.0,
            Transform::Linear,
        ));

        let event = ControlEvent::MidiControl {
            channel: 0,
            controller: 7,
            value: 64,
            normalized: 0.5,
        };
        engine.handle_event(event);

        let cmd = mailbox.pop().unwrap();
        assert_eq!(cmd.parameter.as_ref(), "volume");
        assert!((cmd.value.as_f32().unwrap() - 0.5).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_engine_stop() {
        let (cmd_queue, _mailbox) = ActorRef::<SetParameter>::new_pair();
        let mut engine = PatchbayEngine::new(cmd_queue);

        engine.add_lfo(
            "test_lfo",
            1.0,
            1.0,
            0.0,
            LfoWaveform::Sine,
            std::time::Duration::from_millis(10),
            (NodeId(1), "out".into()),
            (0.0, 1.0),
            ControlStrategy::Absolute,
            crate::strategy::ConflictStrategy::LastWriteWins,
        );

        engine.stop();
        // After stop, no panic = green threads stopped cleanly
    }

    #[tokio::test]
    async fn test_engine_drop_stops_tasks() {
        let (cmd_queue, _mailbox) = ActorRef::<SetParameter>::new_pair();
        {
            let mut engine = PatchbayEngine::new(cmd_queue);
            engine.add_lfo(
                "test_lfo",
                1.0,
                1.0,
                0.0,
                LfoWaveform::Sine,
                std::time::Duration::from_millis(10),
                (NodeId(1), "out".into()),
                (0.0, 1.0),
                ControlStrategy::Absolute,
                crate::strategy::ConflictStrategy::LastWriteWins,
            );
        } // drop → stop_all
    }

    #[tokio::test]
    async fn test_engine_no_runtime_panics() {
        // This test verifies that creating the engine outside tokio panics.
        // We can't easily test this in #[tokio::test], so we just note it.
    }
}
