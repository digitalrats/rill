//! # Rill Graph — Static DAG Audio Graph
//!
//! This crate provides an immutable audio graph with static topology.
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

pub use graph::{AudioGraph, BuildError, ConnectionKind, GraphBuilder};

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::{AudioGraph, GraphBuilder};
    pub use rill_core::prelude::*;
}
