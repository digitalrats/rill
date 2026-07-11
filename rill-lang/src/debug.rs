//! Debug infrastructure types for the rill-lang execution engine.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use rill_core::queues::spsc::SpscQueue;

/// A single frame of signal data captured at a probe point.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProbeFrame {
    pub value_bits: u64,
    pub block_index: u64,
}

/// A fixed-size, Copy-compatible string buffer.
#[derive(Debug, Clone, Copy)]
pub struct CmdStr<const N: usize> {
    bytes: [u8; N],
    len: u8,
}

impl<const N: usize> CmdStr<N> {
    pub fn from_str(s: &str) -> Self {
        let mut bytes = [0u8; N];
        let len = s.len().min(N);
        bytes[..len].copy_from_slice(&s.as_bytes()[..len]);
        Self { bytes, len: len as u8 }
    }
    pub fn as_str(&self) -> &str {
        let len = self.len as usize;
        std::str::from_utf8(&self.bytes[..len]).unwrap_or("")
    }
    pub fn is_empty(&self) -> bool { self.len == 0 }
}

impl<const N: usize> Default for CmdStr<N> {
    fn default() -> Self { Self { bytes: [0u8; N], len: 0 } }
}

/// A single frame of command data captured from the actor mailbox.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandFrame {
    pub block_index: u64,
    pub command_kind: CmdStr<32>,
    pub node_name: CmdStr<64>,
    pub param_name: CmdStr<64>,
    pub value_repr: CmdStr<128>,
}

/// Per-probe runtime slot stored in the engine.
pub struct ProbeSlot {
    pub enabled: AtomicBool,
    pub break_flag: AtomicBool,
    pub paused_flag: AtomicBool,
    pub last_value: AtomicU64,
    pub queue: Arc<SpscQueue<ProbeFrame, 64>>,
}

impl ProbeSlot {
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            break_flag: AtomicBool::new(false),
            paused_flag: AtomicBool::new(false),
            last_value: AtomicU64::new(0),
            queue: Arc::new(SpscQueue::new()),
        }
    }
    #[inline]
    pub fn is_active(&self) -> bool { self.enabled.load(Ordering::Acquire) }
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
    pub global_pause: Arc<AtomicBool>,
    pub global_resume: Arc<AtomicBool>,
    pub block_index: Arc<AtomicU64>,
}

impl DebugControl {
    pub fn new() -> Self {
        Self {
            global_pause: Arc::new(AtomicBool::new(false)),
            global_resume: Arc::new(AtomicBool::new(false)),
            block_index: Arc::new(AtomicU64::new(0)),
        }
    }
    pub fn cont(&self) {
        self.global_pause.store(false, Ordering::Release);
        self.global_resume.store(true, Ordering::Release);
    }
    pub fn pause(&self) {
        self.global_pause.store(true, Ordering::Release);
        self.global_resume.store(false, Ordering::Release);
    }
}

impl Default for DebugControl {
    fn default() -> Self { Self::new() }
}
