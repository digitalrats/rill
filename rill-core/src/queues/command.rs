use std::fmt;

/// Base trait for all commands sent via actor mailboxes.
pub trait Command: Send + 'static + fmt::Debug {}
