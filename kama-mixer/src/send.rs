//! Aux sends for effects

/// Send type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SendType {
    /// Pre-fader (signal taken before channel volume)
    PreFader,
    /// Post-fader (signal taken after channel volume)
    PostFader,
}

/// Configuration for an aux send
#[derive(Debug, Clone)]
pub struct SendConfig {
    /// Target bus index
    pub bus_index: usize,
    /// Send level (0.0 - 1.0)
    pub level: f32,
    /// Send type
    pub send_type: SendType,
}

impl Default for SendConfig {
    fn default() -> Self {
        Self {
            bus_index: 0,
            level: 0.0,
            send_type: SendType::PostFader,
        }
    }
}