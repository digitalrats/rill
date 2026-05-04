//! Patchbay manager — central coordinator (DEPRECATED).
//!
//! `PatchbayManager` is a legacy component. It runs in a dedicated
//! `std::thread` at a fixed update rate. Superseded by the async model:
//! `PatchbayControl::add_automaton_task()` + tokio tasks.
//!
//! Old functionality:
//! - Automata (LFO, envelopes, sequencers)
//! - Event mappings (MIDI/OSC)
//! - Servos (automaton-to-parameter bridge)
//!
//! Recommended replacements:
//! - `PatchbayControl::add_lfo_task()`
//! - `PatchbayControl::add_automaton_task()`
//! - `PatchbayControl::handle_event()`

use rill_core::prelude::*;
use rill_core::queues::MpscQueue;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::automaton::{
    EnvelopeAutomaton, FunctionAutomaton, LfoAutomaton, LfoWaveform, SequencerAutomaton, Step,
};
use crate::control::{
    midi_cc, osc_address, AnyServo, Automaton, BoxedServo, ControlEvent, EventPattern, Mapping,
    ParameterCommand, ParameterMapping, Transform,
};

// =============================================================================
// Event for logging and debugging
// =============================================================================

/// Events emitted by the patchbay manager for logging and debugging.
#[derive(Debug, Clone)]
pub enum PatchbayEvent {
    /// An automaton was updated with a new value.
    AutomatonUpdated {
        /// Automaton identifier.
        id: String,
        /// Current output value.
        value: f64,
        /// Current time.
        time: f64,
    },
    /// A mapping was triggered by an incoming event.
    MappingTriggered {
        /// Matched event pattern description.
        pattern: String,
        /// Target parameter description.
        target: String,
        /// Mapped and transformed value.
        value: f32,
    },
    /// A command was sent to the audio thread.
    CommandSent(ParameterCommand),
    /// An error occurred.
    Error(String),
}

// =============================================================================
// Patchbay statistics
// =============================================================================

/// Runtime statistics for the patchbay.
#[derive(Debug, Clone, Default)]
pub struct PatchbayStats {
    /// Number of active automata.
    pub automaton_count: usize,
    /// Number of active mappings.
    pub mapping_count: usize,
    /// Total commands sent to the audio thread.
    pub commands_sent: u64,
    /// Duration of the last update cycle.
    pub last_update: Option<Duration>,
    /// Average update time in microseconds.
    pub avg_update_time_us: f64,
    /// Maximum update time in microseconds.
    pub max_update_time_us: f64,
    /// Total error count.
    pub error_count: u64,
}

impl PatchbayStats {
    /// Update statistics with the measured duration of one update cycle.
    pub fn update(&mut self, update_duration: Duration) {
        let us = update_duration.as_micros() as f64;
        self.avg_update_time_us = self.avg_update_time_us * 0.9 + us * 0.1;
        self.max_update_time_us = self.max_update_time_us.max(us);
        self.last_update = Some(update_duration);
    }
}

// =============================================================================
// Patchbay configuration
// =============================================================================

/// Configuration for the patchbay.
#[derive(Debug, Clone)]
pub struct PatchbayConfig {
    /// Automaton update rate in Hz.
    pub update_rate_hz: f64,
    /// Command queue capacity.
    pub command_queue_size: usize,
    /// Whether to collect runtime statistics.
    pub collect_stats: bool,
    /// Whether to emit log events.
    pub log_events: bool,
}

impl Default for PatchbayConfig {
    fn default() -> Self {
        Self {
            update_rate_hz: 1000.0,
            command_queue_size: 1024,
            collect_stats: true,
            log_events: false,
        }
    }
}

// =============================================================================
// Main patchbay manager
// =============================================================================

/// The main patchbay manager.
///
/// Coordinates all control and automation components. Runs in a dedicated
/// thread at a configurable update rate.
pub struct PatchbayManager {
    config: PatchbayConfig,
    automata: HashMap<String, Box<dyn std::any::Any + Send>>,
    automaton_states: HashMap<String, Box<dyn std::any::Any + Send>>,
    servos: HashMap<String, BoxedServo>,
    mappings: Vec<Mapping>,
    command_queue: Arc<MpscQueue<ParameterCommand>>,
    event_tx: Option<crossbeam_channel::Sender<PatchbayEvent>>,
    time: f64,
    stats: PatchbayStats,
    running: Arc<AtomicBool>,
    update_thread: Option<std::thread::JoinHandle<()>>,
}

impl PatchbayManager {
    /// Create a new patchbay manager.
    pub fn new(config: PatchbayConfig, command_queue: Arc<MpscQueue<ParameterCommand>>) -> Self {
        Self {
            config,
            automata: HashMap::new(),
            automaton_states: HashMap::new(),
            servos: HashMap::new(),
            mappings: Vec::new(),
            command_queue,
            event_tx: None,
            time: 0.0,
            stats: PatchbayStats::default(),
            running: Arc::new(AtomicBool::new(false)),
            update_thread: None,
        }
    }

    /// Set the event notification channel.
    pub fn with_event_channel(mut self, tx: crossbeam_channel::Sender<PatchbayEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    // =========================================================================
    // Automaton management
    // =========================================================================

    /// Add an automaton.
    ///
    /// # Errors
    ///
    /// Returns `Err` if an automaton with the same ID already exists.
    pub fn add_automaton<A: Automaton + 'static>(
        &mut self,
        id: impl Into<String>,
        automaton: A,
    ) -> Result<(), &'static str>
    where
        A::State: 'static,
    {
        let id = id.into();
        if self.automata.contains_key(&id) {
            return Err("Automaton with this ID already exists");
        }

        let state = automaton.initial_state();
        self.automata.insert(
            id.clone(),
            Box::new(automaton) as Box<dyn std::any::Any + Send>,
        );
        self.automaton_states.insert(id, Box::new(state));

        Ok(())
    }

    /// Add an LFO as an automaton.
    ///
    /// # Errors
    ///
    /// Returns `Err` if an automaton with the same ID already exists.
    pub fn add_lfo(
        &mut self,
        id: impl Into<String>,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
    ) -> Result<(), &'static str> {
        let id_str = id.into();
        let automaton = LfoAutomaton::new(&id_str, frequency, amplitude, offset, waveform);
        self.add_automaton(id_str, automaton)
    }

    /// Add an envelope ADSR as an automaton.
    ///
    /// # Errors
    ///
    /// Returns `Err` if an automaton with the same ID already exists.
    pub fn add_envelope(
        &mut self,
        id: impl Into<String>,
        attack: f64,
        decay: f64,
        sustain: f64,
        release: f64,
    ) -> Result<(), &'static str> {
        let id_str = id.into();
        let automaton = EnvelopeAutomaton::adsr(&id_str, attack, decay, sustain, release);
        self.add_automaton(id_str, automaton)
    }

    /// Add a sequencer as an automaton.
    ///
    /// # Errors
    ///
    /// Returns `Err` if an automaton with the same ID already exists.
    pub fn add_sequencer(
        &mut self,
        id: impl Into<String>,
        steps: Vec<Step>,
    ) -> Result<(), &'static str> {
        let id_str = id.into();
        let automaton = SequencerAutomaton::new(&id_str, steps);
        self.add_automaton(id_str, automaton)
    }

    /// Add a function-based automaton.
    ///
    /// # Errors
    ///
    /// Returns `Err` if an automaton with the same ID already exists.
    pub fn add_function<F>(
        &mut self,
        id: impl Into<String>,
        generator: F,
    ) -> Result<(), &'static str>
    where
        F: Fn(f64) -> f64 + Send + Sync + 'static,
    {
        let id_str = id.into();
        let automaton = FunctionAutomaton::new(&id_str, generator);
        self.add_automaton(id_str, automaton)
    }

    /// Reset an automaton to its initial state.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the automaton is not found or the type does not match.
    pub fn reset_automaton<A: Automaton + 'static>(
        &mut self,
        id: &str,
    ) -> Result<(), &'static str> {
        let automaton = self
            .automata
            .get(id)
            .and_then(|a| a.downcast_ref::<A>())
            .ok_or("Automaton not found or type mismatch")?;
        let state = automaton.initial_state();
        self.automaton_states
            .insert(id.to_string(), Box::new(state));
        Ok(())
    }

    /// Remove an automaton by ID.
    pub fn remove_automaton(&mut self, id: &str) -> bool {
        self.automata.remove(id).is_some() && self.automaton_states.remove(id).is_some()
    }

    // =========================================================================
    // Servo management
    // =========================================================================

    /// Add a servo connecting an automaton to a parameter.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the referenced automaton is not found.
    pub fn add_servo(
        &mut self,
        id: impl Into<String>,
        automaton_id: impl Into<String>,
        target_node: NodeId,
        target_param: impl Into<String>,
        _mapping: ParameterMapping,
        _min: f64,
        _max: f64,
    ) -> Result<(), &'static str> {
        let id_str = id.into();
        let automaton_id_str = automaton_id.into();
        let target_param_str = target_param.into();
        let _automaton = self
            .automata
            .get(&automaton_id_str)
            .ok_or("Automaton not found")?;

        let servo = Box::new(TestServo {
            id: id_str.clone(),
            target_node,
            target_param: target_param_str,
            last_value: 0.0,
        });

        self.servos.insert(id_str, servo);

        Ok(())
    }

    /// Add an LFO servo (convenience method).
    ///
    /// # Errors
    ///
    /// Returns `Err` if an automaton with that ID already exists.
    pub fn add_lfo_servo(
        &mut self,
        id: impl Into<String>,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
        target_node: NodeId,
        target_param: impl Into<String>,
        min: f64,
        max: f64,
    ) -> Result<(), &'static str> {
        let id_str = id.into();
        let automaton_id = format!("{}_auto", &id_str);
        self.add_lfo(&automaton_id, frequency, amplitude, offset, waveform)?;
        self.add_servo(
            id_str,
            automaton_id,
            target_node,
            target_param,
            ParameterMapping::Linear,
            min,
            max,
        )
    }

    /// Get a servo by ID.
    pub fn get_servo(&self, id: &str) -> Option<&dyn AnyServo> {
        self.servos.get(id).map(|b| b.as_ref())
    }

    /// Get a mutable servo by ID.
    pub fn get_servo_mut(&mut self, id: &str) -> Option<&mut BoxedServo> {
        self.servos.get_mut(id)
    }

    /// Remove a servo by ID.
    pub fn remove_servo(&mut self, id: &str) -> bool {
        self.servos.remove(id).is_some()
    }

    // =========================================================================
    // Mapping management
    // =========================================================================

    /// Add an event mapping.
    pub fn add_mapping(&mut self, mapping: Mapping) {
        self.mappings.push(mapping);
    }

    /// Add a MIDI CC mapping (convenience method).
    pub fn add_midi_mapping(
        &mut self,
        controller: u8,
        channel: Option<u8>,
        target_node: NodeId,
        target_param: impl Into<String>,
        min: f32,
        max: f32,
        transform: Transform,
    ) {
        let mapping = midi_cc(
            controller,
            channel,
            target_node,
            &target_param.into(),
            min,
            max,
            transform,
        );
        self.add_mapping(mapping);
    }

    /// Add an OSC address mapping (convenience method).
    pub fn add_osc_mapping(
        &mut self,
        address: &str,
        target_node: NodeId,
        target_param: impl Into<String>,
        min: f32,
        max: f32,
        transform: Transform,
    ) {
        let mapping = osc_address(
            address,
            target_node,
            &target_param.into(),
            min,
            max,
            transform,
        );
        self.add_mapping(mapping);
    }

    /// Remove all mappings matching a given pattern.
    ///
    /// Returns the number of removed mappings.
    pub fn remove_mappings(&mut self, pattern: &EventPattern) -> usize {
        let before = self.mappings.len();
        self.mappings.retain(|m| &m.pattern != pattern);
        before - self.mappings.len()
    }

    /// Clear all mappings.
    pub fn clear_mappings(&mut self) {
        self.mappings.clear();
    }

    // =========================================================================
    // Event handling
    // =========================================================================

    /// Handle an external event (MIDI/OSC).
    pub fn handle_event(&mut self, event: ControlEvent) {
        let mut commands = Vec::new();

        for mapping in &self.mappings {
            if let Some(cmd) = mapping.apply(&event) {
                let value = cmd.value;
                commands.push(cmd);

                if self.config.log_events {
                    self.emit_event(PatchbayEvent::MappingTriggered {
                        pattern: format!("{:?}", mapping.pattern),
                        target: format!(
                            "{}:{}",
                            mapping.target.node_id.0, mapping.target.param_name
                        ),
                        value,
                    });
                }
            }
        }

        for cmd in commands {
            let _ = self.command_queue.push(cmd.clone());
            self.stats.commands_sent += 1;

            if self.config.log_events {
                self.emit_event(PatchbayEvent::CommandSent(cmd));
            }
        }
    }

    /// Handle a MIDI message (convenience method).
    pub fn handle_midi(&mut self, channel: u8, controller: u8, value: u8) {
        let event = ControlEvent::MidiControl {
            channel,
            controller,
            value,
            normalized: value as f32 / 127.0,
        };
        self.handle_event(event);
    }

    /// Handle an OSC message (convenience method).
    pub fn handle_osc(&mut self, address: &str, args: Vec<f32>) {
        let event = ControlEvent::Osc {
            address: address.to_string(),
            args,
        };
        self.handle_event(event);
    }

    fn emit_event(&self, event: PatchbayEvent) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event);
        }
    }

    // =========================================================================
    // Start and stop
    // =========================================================================

    /// Start the manager in a separate thread.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the manager is already running.
    pub fn start(&mut self) -> Result<(), &'static str> {
        if self.running.load(Ordering::Relaxed) {
            return Err("Already running");
        }

        self.running.store(true, Ordering::Relaxed);

        let running = self.running.clone();
        let update_interval = Duration::from_secs_f64(1.0 / self.config.update_rate_hz);
        let collect_stats = self.config.collect_stats;

        let automata = std::mem::take(&mut self.automata);
        let mut automaton_states = std::mem::take(&mut self.automaton_states);
        let mut servos = std::mem::take(&mut self.servos);
        let command_queue = self.command_queue.clone();
        let _event_tx = self.event_tx.clone();

        self.update_thread = Some(std::thread::spawn(move || {
            let mut last_time = Instant::now();
            let mut stats = PatchbayStats::default();
            let mut time = 0.0;

            while running.load(Ordering::Relaxed) {
                let frame_start = Instant::now();

                let now = Instant::now();
                let dt = now.duration_since(last_time).as_secs_f64();
                last_time = now;
                time += dt;

                let mut commands = Vec::new();

                for id in automata.keys() {
                    if let Some(_state) = automaton_states.get_mut(id) {
                        if let Some(servo) = servos.get_mut(id) {
                            if let Some(cmd) = servo.update(time) {
                                commands.push(cmd);
                            }
                        }
                    }
                }

                for cmd in commands {
                    let _ = command_queue.push(cmd.clone());
                    stats.commands_sent += 1;
                }

                if collect_stats {
                    stats.update(frame_start.elapsed());
                }

                let elapsed = frame_start.elapsed();
                if elapsed < update_interval {
                    std::thread::sleep(update_interval - elapsed);
                }
            }
        }));

        Ok(())
    }

    /// Stop the manager.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);

        if let Some(thread) = self.update_thread.take() {
            let _ = thread.join();
        }
    }

    /// Return a reference to the runtime statistics.
    pub fn stats(&self) -> &PatchbayStats {
        &self.stats
    }

    /// Reset the runtime statistics.
    pub fn reset_stats(&mut self) {
        self.stats = PatchbayStats::default();
    }

    /// Return the current internal time in seconds.
    pub fn current_time(&self) -> f64 {
        self.time
    }

    /// Check whether the manager is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

impl Drop for PatchbayManager {
    fn drop(&mut self) {
        self.stop();
    }
}

// =============================================================================
// Helper types for testing
// =============================================================================

/// Stub servo used for testing.
struct TestServo {
    id: String,
    target_node: NodeId,
    target_param: String,
    last_value: f64,
}

impl AnyServo for TestServo {
    fn update(&mut self, time: f64) -> Option<ParameterCommand> {
        let value = (time * 2.0).sin() * 0.5 + 0.5;

        if (value - self.last_value).abs() > 0.01 {
            self.last_value = value;
            Some(ParameterCommand::new(
                self.target_node,
                &self.target_param,
                value as f32,
            ))
        } else {
            None
        }
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn set_enabled(&mut self, _enabled: bool) {}
}

// =============================================================================
// PatchbayManager builder
// =============================================================================

/// Builder for creating a [`PatchbayManager`] with a fluent API.
pub struct PatchbayManagerBuilder {
    config: PatchbayConfig,
    command_queue: Option<Arc<MpscQueue<ParameterCommand>>>,
    event_channel: Option<crossbeam_channel::Sender<PatchbayEvent>>,
}

impl PatchbayManagerBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: PatchbayConfig::default(),
            command_queue: None,
            event_channel: None,
        }
    }

    /// Set the patchbay configuration.
    pub fn with_config(mut self, config: PatchbayConfig) -> Self {
        self.config = config;
        self
    }

    /// Set the update rate in Hz.
    pub fn with_update_rate(mut self, hz: f64) -> Self {
        self.config.update_rate_hz = hz;
        self
    }

    /// Set the command queue.
    pub fn with_command_queue(mut self, queue: Arc<MpscQueue<ParameterCommand>>) -> Self {
        self.command_queue = Some(queue);
        self
    }

    /// Set the event notification channel.
    pub fn with_event_channel(mut self, tx: crossbeam_channel::Sender<PatchbayEvent>) -> Self {
        self.event_channel = Some(tx);
        self.config.log_events = true;
        self
    }

    /// Enable or disable statistics collection.
    pub fn with_stats(mut self, enabled: bool) -> Self {
        self.config.collect_stats = enabled;
        self
    }

    /// Build the [`PatchbayManager`].
    pub fn build(self) -> PatchbayManager {
        let queue = self
            .command_queue
            .unwrap_or_else(|| Arc::new(MpscQueue::with_capacity(self.config.command_queue_size)));

        let mut manager = PatchbayManager::new(self.config, queue);

        if let Some(tx) = self.event_channel {
            manager = manager.with_event_channel(tx);
        }

        manager
    }
}

impl Default for PatchbayManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_manager_creation() {
        let queue = Arc::new(MpscQueue::with_capacity(1024));
        let manager = PatchbayManager::new(PatchbayConfig::default(), queue);

        assert_eq!(manager.automata.len(), 0);
        assert_eq!(manager.mappings.len(), 0);
        assert!(!manager.is_running());
    }

    #[test]
    fn test_add_automaton() {
        let queue = Arc::new(MpscQueue::with_capacity(1024));
        let mut manager = PatchbayManager::new(PatchbayConfig::default(), queue);

        let result = manager.add_lfo("test_lfo", 1.0, 0.5, 0.0, LfoWaveform::Sine);
        assert!(result.is_ok());
        assert_eq!(manager.automata.len(), 1);
    }

    #[test]
    fn test_add_mapping() {
        let queue = Arc::new(MpscQueue::with_capacity(1024));
        let mut manager = PatchbayManager::new(PatchbayConfig::default(), queue);

        manager.add_midi_mapping(7, None, NodeId(1), "volume", 0.0, 1.0, Transform::Linear);
        assert_eq!(manager.mappings.len(), 1);
    }

    #[test]
    fn test_handle_event() {
        let queue = Arc::new(MpscQueue::with_capacity(1024));
        let mut manager = PatchbayManager::new(PatchbayConfig::default(), queue.clone());

        manager.add_midi_mapping(7, None, NodeId(1), "volume", 0.0, 1.0, Transform::Linear);

        let event = ControlEvent::MidiControl {
            channel: 1,
            controller: 7,
            value: 64,
            normalized: 0.5,
        };

        manager.handle_event(event);
    }

    #[test]
    fn test_start_stop() {
        let queue = Arc::new(MpscQueue::with_capacity(1024));
        let mut manager = PatchbayManager::new(PatchbayConfig::default(), queue);

        let result = manager.start();
        assert!(result.is_ok());
        assert!(manager.is_running());

        thread::sleep(Duration::from_millis(100));

        manager.stop();
        assert!(!manager.is_running());
    }

    #[test]
    fn test_builder() {
        let queue = Arc::new(MpscQueue::with_capacity(1024));

        let manager = PatchbayManagerBuilder::new()
            .with_update_rate(500.0)
            .with_command_queue(queue)
            .with_stats(true)
            .build();

        assert_eq!(manager.config.update_rate_hz, 500.0);
        assert!(manager.config.collect_stats);
    }
}
