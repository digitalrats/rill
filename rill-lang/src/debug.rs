//! Debug infrastructure types for the rill-lang execution engine.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use rill_core::queues::spsc::SpscQueue;

/// A single frame of signal data captured at a probe point.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProbeFrame {
    /// Raw bits of the captured signal value.
    pub value_bits: u64,
    /// Block index when this frame was captured.
    pub block_index: u64,
}

/// A fixed-size, Copy-compatible string buffer.
#[derive(Debug, Clone, Copy)]
pub struct CmdStr<const N: usize> {
    bytes: [u8; N],
    len: u8,
}

impl<const N: usize> CmdStr<N> {
    /// Create a new CmdStr from a string slice, truncating to N bytes.
    pub fn new(s: &str) -> Self {
        let mut bytes = [0u8; N];
        let len = s.len().min(N);
        bytes[..len].copy_from_slice(&s.as_bytes()[..len]);
        Self { bytes, len: len as u8 }
    }
    /// Return the contained string slice.
    pub fn as_str(&self) -> &str {
        let len = self.len as usize;
        std::str::from_utf8(&self.bytes[..len]).unwrap_or("")
    }
    /// Returns true if the string is empty.
    pub fn is_empty(&self) -> bool { self.len == 0 }
}

impl<const N: usize> Default for CmdStr<N> {
    fn default() -> Self { Self { bytes: [0u8; N], len: 0 } }
}

/// A single frame of command data captured from the actor mailbox.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandFrame {
    /// Block index when the command was received.
    pub block_index: u64,
    /// Short label identifying the command kind.
    pub command_kind: CmdStr<32>,
    /// Name of the target graph node.
    pub node_name: CmdStr<64>,
    /// Name of the parameter being set.
    pub param_name: CmdStr<64>,
    /// String representation of the parameter value.
    pub value_repr: CmdStr<128>,
}

/// Per-probe runtime slot stored in the engine.
pub struct ProbeSlot {
    /// Whether this probe is actively capturing data.
    pub enabled: AtomicBool,
    /// When set, the engine pauses on value capture.
    pub break_flag: AtomicBool,
    /// Indicates the engine is currently paused at this breakpoint.
    pub paused_flag: AtomicBool,
    /// Most recently captured value, for poll-based inspection.
    pub last_value: AtomicU64,
    /// Ring buffer of captured probe frames.
    pub queue: Arc<SpscQueue<ProbeFrame, 64>>,
}

impl ProbeSlot {
    /// Create a new disabled probe slot.
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            break_flag: AtomicBool::new(false),
            paused_flag: AtomicBool::new(false),
            last_value: AtomicU64::new(0),
            queue: Arc::new(SpscQueue::new()),
        }
    }
    /// Returns true if the probe is enabled and capturing.
    #[inline]
    pub fn is_active(&self) -> bool { self.enabled.load(Ordering::Acquire) }
    /// Returns true if the probe is enabled and has a breakpoint set.
    #[inline]
    pub fn is_breakpoint(&self) -> bool {
        self.enabled.load(Ordering::Acquire) && self.break_flag.load(Ordering::Acquire)
    }
}

impl Default for ProbeSlot {
    fn default() -> Self { Self::new() }
}

/// Debug control atomics shared between engine and collector/debugger threads.
#[derive(Clone)]
pub struct DebugControl {
    /// When true, the engine spins waiting for resume.
    pub global_pause: Arc<AtomicBool>,
    /// When true, the engine resumes execution.
    pub global_resume: Arc<AtomicBool>,
    /// Monotonic counter incremented each processing block.
    pub block_index: Arc<AtomicU64>,
}

impl DebugControl {
    /// Create a new DebugControl with all flags in their default state.
    pub fn new() -> Self {
        Self {
            global_pause: Arc::new(AtomicBool::new(false)),
            global_resume: Arc::new(AtomicBool::new(false)),
            block_index: Arc::new(AtomicU64::new(0)),
        }
    }
    /// Release a paused engine and signal it to continue.
    pub fn cont(&self) {
        self.global_pause.store(false, Ordering::Release);
        self.global_resume.store(true, Ordering::Release);
    }
    /// Signal the engine to pause at the next inter-tick boundary.
    pub fn pause(&self) {
        self.global_pause.store(true, Ordering::Release);
        self.global_resume.store(false, Ordering::Release);
    }
}

impl Default for DebugControl {
    fn default() -> Self { Self::new() }
}
