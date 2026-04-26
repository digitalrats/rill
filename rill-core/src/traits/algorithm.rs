//! Algorithm trait — the unified per-port and DSP processing primitive.
//!
//! All processing in the Rill graph is defined by `Algorithm` implementations.
//! Every port's `run_action()` delegates to its `Algorithm::process()`.
//! Low-level DSP primitives (filters, generators) also implement `Algorithm`.

use crate::math::AudioNum;
use crate::time::ClockTick;
use crate::traits::ProcessResult;

// ============================================================================
// ActionContext
// ============================================================================

/// Context provided to an `Algorithm` during processing.
pub struct ActionContext<'a> {
    /// Current clock tick
    pub tick: &'a ClockTick,
}

impl<'a> ActionContext<'a> {
    pub fn new(tick: &'a ClockTick) -> Self {
        Self { tick }
    }
}

// ============================================================================
// Algorithm Metadata
// ============================================================================

/// Category of an algorithm (for introspection / UI)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlgorithmCategory {
    /// Signal generator (oscillator, noise, etc.)
    Generator,
    /// Audio filter (biquad, SVF, etc.)
    Filter,
    /// Audio effect (delay, distortion, etc.)
    Effect,
    /// Signal analyzer (meter, scope, etc.)
    Analyzer,
    /// Utility / helper (smoother, mapper, etc.)
    Utility,
}

impl AlgorithmCategory {
    /// Human-readable name
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Generator => "generator",
            Self::Filter => "filter",
            Self::Effect => "effect",
            Self::Analyzer => "analyzer",
            Self::Utility => "utility",
        }
    }
}

/// Descriptive metadata for an `Algorithm` implementation.
#[derive(Debug, Clone)]
pub struct AlgorithmMetadata {
    /// Short name (e.g. "Biquad", "OnePole", "ParamSmoother")
    pub name: &'static str,
    /// Functional category
    pub category: AlgorithmCategory,
    /// One-line description
    pub description: &'static str,
    /// Author name
    pub author: &'static str,
    /// Version string
    pub version: &'static str,
}

impl AlgorithmMetadata {
    /// Minimal default metadata (Utility category, no name).
    pub const fn empty() -> Self {
        Self {
            name: "",
            category: AlgorithmCategory::Utility,
            description: "",
            author: "",
            version: "",
        }
    }
}

// ============================================================================
// Algorithm Trait
// ============================================================================

/// Unified processing primitive for ports and DSP blocks.
///
/// Every port in the graph owns an optional `Box<dyn Algorithm>`. When present,
/// the port's `run_action()` calls `Algorithm::process()` to fill its buffer.
///
/// Low-level DSP components (filters, generators, effects) also implement this
/// trait directly, making them usable both inside the graph and standalone.
///
/// # Required methods
/// - `process()` — the main per-block processing entry point.
/// - `reset()` — restore initial state.
///
/// # Optional methods
/// - `init()` — configure sample rate.
/// - `apply_command()` — receive a real-time parameter value from the control
///   path (called between samples by the graph driver).
/// - `metadata()` — return descriptive info (defaults to empty).
pub trait Algorithm<T: AudioNum>: Send + Sync {
    /// Process one block of audio.
    ///
    /// # Arguments
    /// * `input`  — Audio data from upstream (empty when the port is
    ///   unconnected, or `None` for source ports / control output ports).
    /// * `output` — Buffer to fill with processed data.
    /// * `ctx`    — Processing context (clock tick, block position, etc.).
    fn process(
        &mut self,
        input: Option<&[T]>,
        output: &mut [T],
        ctx: &ActionContext,
    ) -> ProcessResult<()>;

    /// Receive a real-time command value from the control path.
    ///
    /// Called by the graph driver between `process()` calls when a
    /// `SetParameter` targets this port. The algorithm should store the
    /// value and apply it (possibly smoothed) on the next `process()`.
    ///
    /// Default: no-op.
    fn apply_command(&mut self, _value: T) {}

    /// Initialise the algorithm with a sample rate.
    ///
    /// Called once when the node is added to the graph. Available for
    /// coefficient pre-computation.
    ///
    /// Default: no-op.
    fn init(&mut self, _sample_rate: f32) {}

    /// Reset the algorithm to its initial state.
    ///
    /// Called when the owning node is reset, or when feedback delay lines
    /// need clearing.
    fn reset(&mut self);

    /// Descriptive metadata (defaults to empty).
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata::empty()
    }
}
