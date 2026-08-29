//! The tape as a passive in-memory delay backend.
//!
//! The tape is an in-memory ring buffer written by a write head and read by
//! any number of read heads, declared declaratively via [`backend::TapeBackendSpec`].

pub mod backend;
pub mod read_head;
pub mod tape_loop;
pub mod write_head;
