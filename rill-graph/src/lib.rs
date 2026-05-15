//! # Rill Graph — Static DAG Signal Graph
//!
//! This crate provides an immutable signal graph with static topology.
//! Build once with `GraphBuilder`. The graph is a pure topology description
//! — processing is driven by port-level methods (`pre_process`,
//! `snapshot_feedback`, `propagate`) called from external code.
//!
//! ## Key Features
//!
//! - **Static DAG topology** — connections are fixed after build
//! - **Kahn's algorithm** — automatic topological sort with cycle detection
//! - **Auto FanOut/FanIn** — connections classified by topology (user never chooses)
//! - **Port-owned routing** — downstream connections and feedback state live on ports
//! - **Copy-based buffer routing** — separate input/output buffer pools (zero-copy planned)
//! - **Safe Rust** — no `unsafe` code

#![warn(missing_docs)]
#![deny(unsafe_code)]

mod graph;

/// Backend factory for constructing I/O backends by name.
pub mod backend_factory;

/// Node factory and registry for constructing nodes by type name.
pub mod factory;

/// ActiveNode trait — nodes that drive graph processing.
pub mod active;

/// Graph serialization (JSON / CBOR). Feature-gated behind `serialization`.
#[cfg(feature = "serialization")]
pub mod serialization;

/// DOT graph visualization (Graphviz). Feature-gated behind `dot`.
#[cfg(feature = "dot")]
pub mod dot;

pub use factory::{NodeConstructor, NodeFactory, RegistryError};
pub use graph::{BuildError, Graph, GraphBuilder, GraphResource};

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::{Graph, GraphBuilder, NodeConstructor, NodeFactory, RegistryError};
    pub use rill_core::prelude::*;
}
