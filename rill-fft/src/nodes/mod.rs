// rill-fft/src/nodes/mod.rs
//! Graph node wrappers for rill-fft types.
//!
//! These types implement `Node<T, BUF_SIZE>` + role traits
//! (`Processor`, `Source`, `Sink`) for use in the signal graph.

pub mod convolver_node;
