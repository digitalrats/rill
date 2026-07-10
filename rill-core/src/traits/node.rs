use std::fmt;

pub struct NodeId(pub u32);

impl NodeId {
    /// Create a new node ID
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the inner value
    pub const fn inner(&self) -> u32 {
        self.0
    }
}
