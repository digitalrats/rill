use std::collections::HashMap;

use super::pattern::{Pattern, StepPlayMode};
use super::snapshot::Snapshot;
use crate::control::ParameterCommand;

/// The core sequencer state machine.
///
/// Driven by incoming CLOCK_TICK telemetry from the audio thread.  Call
/// [`tick`](Self::tick) or [`tick_ext`](Self::tick_ext) every time a clock
/// sample-position arrives; the sequencer checks whether the current step's
/// duration has elapsed and advances if so, returning the p-lock parameter
/// commands for the new step.
///
/// # Thread safety
///
/// `SnapshotSequencer` is `Send` but **not** `Sync`.  It should live inside
/// a single task (typically the tokio task that drains the telemetry
/// receiver).  External control (start/stop/pattern change) uses a command
/// channel (see [`SequencerHandle`]).
#[derive(Debug, Clone)]
pub struct SnapshotSequencer {
    /// Named snapshots for quick recall.
    snapshots: HashMap<String, Snapshot>,
    /// Registered patterns.
    patterns: HashMap<String, Pattern>,
    /// Currently active pattern ID.
    active_pattern: String,
    /// Index of the current step within the active pattern.
    current_step: usize,
    /// Absolute sample position when the current step started.
    step_start_sample: u64,
    /// Play direction for PingPong mode (1 = forward, -1 = backward).
    direction: i8,
    /// Whether the sequencer is running.
    running: bool,
    /// Latest beat position received from telemetry.
    latest_beat_position: f32,
    /// Whether the latest tick was a new beat boundary.
    latest_new_beat: bool,
    /// Whether the latest tick was a new bar boundary.
    latest_new_bar: bool,
}

impl SnapshotSequencer {
    /// Create a new, empty sequencer (no patterns, not running).
    pub fn new() -> Self {
        Self {
            snapshots: HashMap::new(),
            patterns: HashMap::new(),
            active_pattern: String::new(),
            current_step: 0,
            step_start_sample: 0,
            direction: 1,
            running: false,
            latest_beat_position: 0.0,
            latest_new_beat: false,
            latest_new_bar: false,
        }
    }

    /// Create a sequencer pre-loaded with snapshots and patterns.
    ///
    /// The first pattern in the vec becomes the active pattern.
    pub fn with_lib(snapshots: Vec<Snapshot>, patterns: Vec<Pattern>) -> Self {
        let mut s = Self::new();
        for snap in snapshots {
            s.add_snapshot(snap);
        }
        for pat in patterns {
            s.add_pattern(pat);
        }
        if let Some(first) = s.patterns.keys().next() {
            s.active_pattern = first.clone();
        }
        s
    }

    // ── Snapshots ────────────────────────────────────────────────────

    /// Register or replace a named snapshot.
    pub fn add_snapshot(&mut self, snapshot: Snapshot) {
        self.snapshots.insert(snapshot.id.clone(), snapshot);
    }

    /// Get a snapshot by ID, if present.
    pub fn get_snapshot(&self, id: &str) -> Option<&Snapshot> {
        self.snapshots.get(id)
    }

    /// Remove a snapshot by ID.  Does *not* affect existing patterns.
    pub fn remove_snapshot(&mut self, id: &str) -> bool {
        self.snapshots.remove(id).is_some()
    }

    // ── Patterns ─────────────────────────────────────────────────────

    /// Register or replace a named pattern.
    pub fn add_pattern(&mut self, pattern: Pattern) {
        if self.active_pattern.is_empty() {
            self.active_pattern = pattern.id.clone();
        }
        self.patterns.insert(pattern.id.clone(), pattern);
    }

    /// Get a pattern by ID, if present.
    pub fn get_pattern(&self, id: &str) -> Option<&Pattern> {
        self.patterns.get(id)
    }

    /// Remove a pattern by ID.  If it is the active pattern the sequencer
    /// stops.
    pub fn remove_pattern(&mut self, id: &str) -> bool {
        if self.patterns.remove(id).is_some() {
            if self.active_pattern == id {
                self.active_pattern.clear();
                self.running = false;
            }
            true
        } else {
            false
        }
    }

    /// Switch to a different pattern (may be empty).
    ///
    /// Resets the step counter to 0.  If the pattern does not exist the
    /// call is ignored.
    pub fn set_active_pattern(&mut self, id: &str) {
        if self.patterns.contains_key(id) || id.is_empty() {
            self.active_pattern = id.to_string();
            self.current_step = 0;
            self.step_start_sample = 0;
            self.direction = 1;
        }
    }

    /// Active pattern ID.
    pub fn active_pattern(&self) -> &str {
        &self.active_pattern
    }

    // ── Transport ────────────────────────────────────────────────────

    /// Start or resume the sequencer from the current step.
    pub fn start(&mut self) {
        self.running = true;
    }

    /// Pause the sequencer (keeps current step position).
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Reset the sequencer to step 0 at the given sample position.
    pub fn reset(&mut self, sample_pos: u64) {
        self.current_step = 0;
        self.step_start_sample = sample_pos;
        self.direction = 1;
    }

    /// Whether the sequencer is running.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Index of the current step within the active pattern.
    pub fn current_step(&self) -> usize {
        self.current_step
    }

    /// Latest beat position received via [`tick_ext`](Self::tick_ext).
    ///
    /// Updated on every clock tick; `0.0` if no tempo data or
    /// [`tick`](Self::tick) is used.
    pub fn latest_beat_position(&self) -> f32 {
        self.latest_beat_position
    }

    /// Whether the latest clock tick was at a beat boundary.
    pub fn is_new_beat(&self) -> bool {
        self.latest_new_beat
    }

    /// Whether the latest clock tick was at a bar boundary.
    pub fn is_new_bar(&self) -> bool {
        self.latest_new_bar
    }

    // ── Tick (the main state-machine entry point) ────────────────────

    /// Advance the sequencer by one clock tick (basic version).
    ///
    /// Convenience wrapper around [`tick_ext`](Self::tick_ext) that passes
    /// default beat info (beat_position=0, no beat/bar boundaries).
    /// Prefer `tick_ext` when beat-aware CLOCK_TICK telemetry is available.
    pub fn tick(&mut self, sample_pos: u64, sample_rate: f32, tempo: f32) -> Vec<ParameterCommand> {
        self.tick_ext(sample_pos, sample_rate, tempo, 0.0, false, false)
    }

    /// Advance the sequencer by one clock tick with beat-aware telemetry.
    ///
    /// Call this from the telemetry listener task every time a `CLOCK_TICK`
    /// event arrives from the audio thread.  The extended parameters
    /// (`beat_position`, `is_new_beat`, `is_new_bar`) are stored in the
    /// sequencer state and can be queried by algorithmic sequencer logic
    /// (see [`latest_beat_position`](Self::latest_beat_position),
    /// [`is_new_beat`](Self::is_new_beat),
    /// [`is_new_bar`](Self::is_new_bar)).
    ///
    /// Returns a batch of [`ParameterCommand`] values to push when a step
    /// boundary is crossed, or an empty `Vec` if no step change occurred.
    pub fn tick_ext(
        &mut self,
        sample_pos: u64,
        sample_rate: f32,
        tempo: f32,
        beat_position: f32,
        is_new_beat: bool,
        is_new_bar: bool,
    ) -> Vec<ParameterCommand> {
        self.latest_beat_position = beat_position;
        self.latest_new_beat = is_new_beat;
        self.latest_new_bar = is_new_bar;
        if !self.running {
            return Vec::new();
        }

        let (len, play_mode, step_dur) = {
            let pat = match self.patterns.get(&self.active_pattern) {
                Some(p) if !p.steps.is_empty() => p,
                _ => return Vec::new(),
            };
            let step = &pat.steps[self.current_step];
            (
                pat.steps.len(),
                pat.play_mode,
                step.duration_samples(tempo, sample_rate),
            )
        };

        let elapsed = sample_pos.saturating_sub(self.step_start_sample);

        if elapsed >= step_dur {
            self.current_step = self.advance_step(len, play_mode);
            self.step_start_sample = sample_pos;

            if let Some(pat) = self.patterns.get(&self.active_pattern) {
                if self.current_step < pat.steps.len() {
                    let new_step = &pat.steps[self.current_step];
                    return new_step
                        .parameters
                        .iter()
                        .map(|p| ParameterCommand {
                            node_id: p.node_id,
                            param: p.param_name.clone(),
                            value: p.value,
                        })
                        .collect();
                }
            }
        }

        Vec::new()
    }

    /// Pick the next step index and update direction for PingPong mode.
    fn advance_step(&mut self, len: usize, play_mode: StepPlayMode) -> usize {
        if len == 0 {
            return 0;
        }
        match play_mode {
            StepPlayMode::OneShot => (self.current_step + 1).min(len.saturating_sub(1)),
            StepPlayMode::Loop => (self.current_step + 1) % len,
            StepPlayMode::PingPong => {
                let next = self.current_step as isize + self.direction as isize;
                if next < 0 {
                    self.direction = 1;
                    1
                } else if next >= len as isize {
                    self.direction = -1;
                    len.saturating_sub(2)
                } else {
                    next as usize
                }
            }
            StepPlayMode::Random => {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                rng.gen_range(0..len)
            }
            StepPlayMode::Brownian => {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let offset: isize = rng.gen_range(-1..=1);
                (self.current_step as isize + offset).clamp(0, len.saturating_sub(1) as isize)
                    as usize
            }
        }
    }
}

impl Default for SnapshotSequencer {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Sequencer command channel & handle
// =============================================================================

/// Commands sent to a running sequencer from another thread.
#[derive(Debug, Clone, PartialEq)]
pub enum SequencerCommand {
    /// Start playback.
    Start,
    /// Stop playback.
    Stop,
    /// Reset to the given sample position.
    Reset {
        /// Target sample position for the reset.
        sample_pos: u64,
    },
    /// Switch to a named pattern.
    SetPattern(String),
}

/// Handle for controlling a [`SnapshotSequencer`] that lives inside a
/// tokio task.
///
/// Clone the handle to share control of the sequencer across multiple
/// threads (e.g. multiple OSC handlers).
#[derive(Debug, Clone)]
pub struct SequencerHandle {
    cmd_tx: std::sync::Arc<crossbeam_channel::Sender<SequencerCommand>>,
}

impl SequencerHandle {
    pub(crate) fn new(cmd_tx: crossbeam_channel::Sender<SequencerCommand>) -> Self {
        Self {
            cmd_tx: std::sync::Arc::new(cmd_tx),
        }
    }

    /// Start the sequencer.
    pub fn start(&self) {
        let _ = self.cmd_tx.try_send(SequencerCommand::Start);
    }

    /// Stop the sequencer.
    pub fn stop(&self) {
        let _ = self.cmd_tx.try_send(SequencerCommand::Stop);
    }

    /// Reset the sequencer to the given sample position.
    pub fn reset(&self, sample_pos: u64) {
        let _ = self.cmd_tx.try_send(SequencerCommand::Reset { sample_pos });
    }

    /// Switch to a different pattern by ID.
    pub fn set_pattern(&self, id: &str) {
        let _ = self
            .cmd_tx
            .try_send(SequencerCommand::SetPattern(id.to_string()));
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequencer::{ParameterTarget, SequenceStep};
    use rill_core::NodeId;

    fn make_step(value: f32, dur: f64) -> SequenceStep {
        SequenceStep::single(NodeId(1), "param", value, dur)
    }

    fn simple_pattern() -> Pattern {
        Pattern::new(
            "p1",
            vec![
                make_step(0.0, 1.0),
                make_step(0.5, 1.0),
                make_step(1.0, 1.0),
                make_step(0.5, 1.0),
            ],
        )
    }

    #[test]
    fn test_sequencer_loop() {
        let mut seq = SnapshotSequencer::with_lib(vec![], vec![simple_pattern()]);
        seq.start();

        let sr = 48000.0;
        let tempo = 120.0;

        let cmds = seq.tick(24000, sr, tempo);
        assert!(!cmds.is_empty(), "should advance to step 1");
        assert_eq!(seq.current_step, 1);

        let cmds = seq.tick(48000, sr, tempo);
        assert!(!cmds.is_empty());
        assert_eq!(seq.current_step, 2);

        let cmds = seq.tick(72000, sr, tempo);
        assert!(!cmds.is_empty());
        assert_eq!(seq.current_step, 3);

        let cmds = seq.tick(96000, sr, tempo);
        assert!(!cmds.is_empty());
        assert_eq!(seq.current_step, 0);
    }

    #[test]
    fn test_sequencer_not_running() {
        let mut seq = SnapshotSequencer::with_lib(vec![], vec![simple_pattern()]);
        let cmds = seq.tick(24000, 48000.0, 120.0);
        assert!(cmds.is_empty());
        assert_eq!(seq.current_step, 0);
    }

    #[test]
    fn test_sequencer_stop() {
        let mut seq = SnapshotSequencer::with_lib(vec![], vec![simple_pattern()]);
        seq.start();
        seq.tick(24000, 48000.0, 120.0);
        assert_eq!(seq.current_step, 1);

        seq.stop();
        seq.tick(48000, 48000.0, 120.0);
        assert_eq!(seq.current_step, 1, "should not advance after stop");
    }

    #[test]
    fn test_sequencer_pingpong() {
        let mut seq = SnapshotSequencer::with_lib(
            vec![],
            vec![Pattern::new(
                "p1",
                vec![
                    make_step(0.0, 1.0),
                    make_step(0.5, 1.0),
                    make_step(1.0, 1.0),
                ],
            )
            .with_mode(StepPlayMode::PingPong)],
        );
        seq.start();

        seq.tick(24000, 48000.0, 120.0);
        assert_eq!(seq.current_step, 1);
        seq.tick(48000, 48000.0, 120.0);
        assert_eq!(seq.current_step, 2);
        seq.tick(72000, 48000.0, 120.0);
        assert_eq!(seq.current_step, 1);
        seq.tick(96000, 48000.0, 120.0);
        assert_eq!(seq.current_step, 0);
    }

    #[test]
    fn test_sequencer_oneshot() {
        let mut seq = SnapshotSequencer::with_lib(
            vec![],
            vec![
                Pattern::new("p1", vec![make_step(0.0, 1.0), make_step(0.5, 1.0)])
                    .with_mode(StepPlayMode::OneShot),
            ],
        );
        seq.start();

        seq.tick(24000, 48000.0, 120.0);
        assert_eq!(seq.current_step, 1);
        seq.tick(48000, 48000.0, 120.0);
        assert_eq!(seq.current_step, 1);
    }

    #[test]
    fn test_sequencer_set_pattern() {
        let mut seq = SnapshotSequencer::with_lib(
            vec![],
            vec![
                Pattern::new("a", vec![make_step(1.0, 1.0)]),
                Pattern::new("b", vec![make_step(0.0, 1.0), make_step(0.5, 1.0)]),
            ],
        );
        seq.set_active_pattern("a");
        seq.start();

        assert_eq!(seq.active_pattern(), "a");

        seq.set_active_pattern("b");
        assert_eq!(seq.active_pattern(), "b");
        assert_eq!(seq.current_step, 0);

        let cmds = seq.tick(24000, 48000.0, 120.0);
        assert!(!cmds.is_empty());
        assert_eq!(seq.current_step, 1);

        let cmds = seq.tick(48000, 48000.0, 120.0);
        assert!(!cmds.is_empty());
        assert_eq!(seq.current_step, 0);
    }

    #[test]
    fn test_step_duration_samples() {
        let step = make_step(0.5, 1.0);
        assert_eq!(step.duration_samples(120.0, 48000.0), 24000);
        assert_eq!(step.duration_samples(120.0, 44100.0), 22050);
        assert_eq!(step.duration_samples(60.0, 48000.0), 48000);

        let eighth = make_step(0.5, 0.5);
        assert_eq!(eighth.duration_samples(120.0, 48000.0), 12000);

        let sixteenth = make_step(0.5, 0.25);
        assert_eq!(sixteenth.duration_samples(120.0, 48000.0), 6000);
    }

    #[test]
    fn test_parameter_target_creation() {
        let pt = ParameterTarget::new(NodeId(1), "gain", 0.5);
        assert_eq!(pt.node_id, NodeId(1));
        assert_eq!(pt.param_name, "gain");
        assert_eq!(pt.value, 0.5);
    }

    #[test]
    fn test_sequencer_handle_send() {
        let (tx, rx) = crossbeam_channel::unbounded::<SequencerCommand>();
        let handle = SequencerHandle::new(tx);

        handle.start();
        assert_eq!(rx.try_recv(), Ok(SequencerCommand::Start));

        handle.stop();
        assert_eq!(rx.try_recv(), Ok(SequencerCommand::Stop));

        handle.set_pattern("foo");
        assert_eq!(
            rx.try_recv(),
            Ok(SequencerCommand::SetPattern("foo".into()))
        );

        handle.reset(12345);
        assert_eq!(
            rx.try_recv(),
            Ok(SequencerCommand::Reset { sample_pos: 12345 })
        );
    }

    #[test]
    fn test_tick_ext_stores_beat_info() {
        let mut seq = SnapshotSequencer::new();
        let pat = Pattern::new("p1", vec![SequenceStep::single(NodeId(1), "p", 0.5, 1.0)]);
        seq.add_pattern(pat);
        seq.set_active_pattern("p1");
        seq.start();

        let _ = seq.tick_ext(0, 48000.0, 120.0, 0.0, true, true);
        assert!((seq.latest_beat_position() - 0.0).abs() < 1e-6);
        assert!(seq.is_new_beat());
        assert!(seq.is_new_bar());

        let _ = seq.tick(24000, 48000.0, 120.0);
        assert!((seq.latest_beat_position() - 0.0).abs() < 1e-6);
        assert!(!seq.is_new_beat());
        assert!(!seq.is_new_bar());

        let _ = seq.tick_ext(48000, 48000.0, 120.0, 2.5, false, false);
        assert!((seq.latest_beat_position() - 2.5).abs() < 1e-6);
        assert!(!seq.is_new_beat());
        assert!(!seq.is_new_bar());
    }
}
