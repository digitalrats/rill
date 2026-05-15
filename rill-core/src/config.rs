//! # Configuration
//!
//! Types for configuring system components.

    /// Operating mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Real-time mode (maximum performance)
    Realtime,
    /// Low latency mode (for live-coding)
    LowLatency,
    /// Eco mode (less CPU)
    Eco,
    /// Debug mode (checks, logs)
    Debug,
}

    /// Thread priority
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadPriority {
    /// Low (background)
    Low,
    /// Normal
    Normal,
    /// High
    High,
    /// Maximum (for RT thread)
    Realtime,
    /// Custom
    Custom(i32),
}

/// I/O configuration
#[derive(Debug, Clone)]
pub struct IoConfig {
    /// Sample rate
    pub sample_rate: u32,
    /// Buffer size
    pub buffer_size: usize,
    /// Number of channels
    pub channels: u16,
/// Operating mode
    pub mode: Mode,
/// Thread priority
    pub thread_priority: ThreadPriority,
    /// Device name (optional)
    pub device_name: Option<String>,
}

impl Default for IoConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            buffer_size: 256,
            channels: 2,
            mode: Mode::Realtime,
            thread_priority: ThreadPriority::Realtime,
            device_name: None,
        }
    }
}

impl IoConfig {
    /// Create a new configuration
    pub fn new(sample_rate: u32, buffer_size: usize) -> Self {
        Self {
            sample_rate,
            buffer_size,
            ..Default::default()
        }
    }
    
    /// Set the number of channels
    pub fn with_channels(mut self, channels: u16) -> Self {
        self.channels = channels;
        self
    }
    
    /// Set the mode
    pub fn with_mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }
    
    /// Set the priority
    pub fn with_priority(mut self, priority: ThreadPriority) -> Self {
        self.thread_priority = priority;
        self
    }
    
    /// Set the device name
    pub fn with_device(mut self, name: impl Into<String>) -> Self {
        self.device_name = Some(name.into());
        self
    }
    
    /// Get latency in seconds
    pub fn latency_seconds(&self) -> f64 {
        self.buffer_size as f64 / self.sample_rate as f64
    }
    
    /// Get latency in milliseconds
    pub fn latency_ms(&self) -> f64 {
        self.latency_seconds() * 1000.0
    }
}

/// Queue configuration
#[derive(Debug, Clone)]
pub struct QueueConfig {
    /// Queue size
    pub size: usize,
    /// Overflow mode
    pub overflow_policy: queue::OverflowPolicy,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            size: 1024,
            overflow_policy: queue::OverflowPolicy::OverwriteOldest,
        }
    }
}