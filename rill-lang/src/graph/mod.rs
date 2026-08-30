//! Graph compilation: [`GraphSpec`] → [`CompiledStream`].
//!
//! The graph module turns a data-level graph description into compiled
//! programs. A plain graph compiles to a single [`CompiledStream::Single`]; a
//! graph with passive backends (tape heads) partitions into a recording and a
//! playback subgraph connected by a plain-data [`TapeSpec`] that the runtime
//! converts into a `rill-sampler` tape backend.

pub mod compile;
pub mod partition;
pub mod reconstruct;
pub mod spec;

pub use compile::{compile, CompiledStream};
pub use partition::{partition, SubGraph};
pub use reconstruct::compile_spec;
pub use spec::*;

/// Plain-data tape backend configuration produced by a duplex compile.
///
/// Converted to `rill_sampler::tape::backend::TapeBackendSpec` by the runtime
/// (Task 6); rill-lang stays free of a rill-sampler dependency.
#[derive(Debug, Clone)]
pub struct TapeSpec {
    /// Tape backend name, e.g. `"tape_0"`.
    pub name: String,
    /// Tape loop capacity in samples.
    pub capacity: usize,
    /// Write-head feedback gain.
    pub write_feedback: f64,
    /// Per-read-head delay in seconds.
    pub read_delays: Vec<f64>,
}
