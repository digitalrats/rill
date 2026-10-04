//! # rill-core memory subsystem
//!
//! Centralized, RT-safe memory management for the rill ecosystem.
//!
//! - [`ChunkedVec`] — grow-safe chunked storage (the shared primitive).
//! - [`Arena`] / [`ArenaValue`] — the genericized value arena (fixed-capacity,
//!   reference-counted, copy-on-write, embedded free list).
//! - [`BufferPool`] — size-classed payload buffer pool.
//! - [`MemoryRegion`] / [`RegionHandle`] / [`BufferAllocator`] — chunked,
//!   reference-counted slabs for large static buffer memory.
//!
//! # Safety policy
//!
//! This module is exempt from the crate-level `#![deny(unsafe_code)]` under the
//! policy in the SP-4 design spec (§2.1): `unsafe` is only introduced with a
//! proven performance benefit, every `unsafe` block carries a `// SAFETY:`
//! invariant, and the unsafe paths are Miri-checked.

#![allow(unsafe_code)]

pub mod chunked;

pub use chunked::ChunkedVec;
