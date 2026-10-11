//! Graph compilation: [`GraphSpec`] → [`CompiledStream`].
//!
//! The graph module turns a data-level graph description into compiled
//! programs. A plain graph compiles to a single [`CompiledStream::Single`]; a
//! graph with passive backends (tape heads) partitions into a recording and a
//! playback subgraph connected by a plain-data [`TapeSpec`] that the runtime
//! converts into a `rill-sampler` tape backend.
//!
//! Gated behind the `graph` feature: the graph front-end (`GraphBuilder`,
//! `GraphSpec`, reconstruction into rill-lang AST, and the tape-duplex
//! partition) is an optional convenience layer over the ordinary compilation
//! pipeline.

/// Mutable graph builder front-end: [`GraphBuilder`] accumulates nodes and
/// edges, then converts to a [`GraphSpec`] for reconstruction into rill-lang
/// AST (`compile_spec`). Also the graph serialization (`GraphDef`, JSON/CBOR).
pub mod builder;
pub mod compile;
pub mod partition;
pub mod reconstruct;
#[cfg(feature = "serialization")]
pub mod serialization;
pub mod spec;

pub use builder::{BuildError, GraphBuilder, GraphResource};
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
