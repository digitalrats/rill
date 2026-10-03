//! # Rill Graph — Static DAG Signal Graph
//!
//! This crate provides an immutable signal graph with static topology.
//! Build once with `GraphBuilder`. The graph is a pure topology description
//! — `ast_from_def()` produces a rill-lang AST, which `rill-lang` compiles
//! into a single [`rill_lang::program_engine::ProgramEngine`] for execution.
//!
//! ## Key Features
//!
//! - **Static DAG topology** — connections are fixed after build
//! - **Kahn's algorithm** — automatic topological sort with cycle detection
//! - **Auto FanOut/FanIn** — connections classified by topology (user never chooses)

#![warn(missing_docs)]
#![deny(unsafe_code)]

mod graph;

/// Graph serialization (JSON / CBOR). Feature-gated behind `serialization`.
#[cfg(feature = "serialization")]
pub mod serialization;

pub use graph::{BuildError, GraphBuilder, GraphResource};

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::GraphBuilder;
    pub use rill_core::prelude::*;
}
