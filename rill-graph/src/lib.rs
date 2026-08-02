//! # Rill Graph — Static DAG Signal Graph
//!
//! This crate provides an immutable signal graph with static topology.
//! Build once with `GraphBuilder`. The graph is a pure topology description
//! — `build_ir()` produces a [`rill_lang::graph_ir::GraphIr`], which
//! `rill-lang`'s [`rill_lang::graph_compiler::compile`] transforms into a
//! [`rill_lang::graph_engine::CompiledGraphEngine`] for execution.
//!
//! ## Key Features
//!
//! - **Static DAG topology** — connections are fixed after build
//! - **Kahn's algorithm** — automatic topological sort with cycle detection
//! - **Auto FanOut/FanIn** — connections classified by topology (user never chooses)
//! - **GraphIr** — intermediate representation for rill-lang compilation

#![warn(missing_docs)]
#![deny(unsafe_code)]

mod graph;

/// Backend factory for constructing I/O backends by name.
pub mod backend_factory;

/// Graph serialization (JSON / CBOR). Feature-gated behind `serialization`.
#[cfg(feature = "serialization")]
pub mod serialization;

pub use graph::{BuildError, GraphBuilder, GraphResource};

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::GraphBuilder;
    pub use rill_core::prelude::*;
}
