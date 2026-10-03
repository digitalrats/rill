//! The tape as a passive in-memory delay backend.
//!
//! The tape is an in-memory ring buffer written by a write head and read by
//! any number of read heads, declared declaratively via [`backend::TapeBackendSpec`].
//! The buffer itself lives in `rill_core::buffer` and is shared only through the
//! [`SharedWriter`](rill_core::buffer::SharedWriter) /
//! [`SharedReader`](rill_core::buffer::SharedReader) capability wrappers.

pub mod backend;
#[cfg(feature = "lang")]
pub mod lang;
pub mod read_head;
pub mod write_head;
