# IPC via Shared Memory for rill-analyzer — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add inter-process debugging via shared memory — `rill-analyzer attach <pid>` and `rill-analyzer launch <target>` connect to running rill processes.

**Architecture:** `ShmemRegion` (unsafe mmap in rill-telemetry) manages `/dev/shm/rill-debug-<pid>`. `CollectorThread` gains a shmem mode alongside mpsc. Two lock-free ring buffers carry serialized `AnalyzerCommand`/`AnalyzerResponse` via serde_cbor. `rill-adrift` creates shmem at startup under `debug` feature.

**Tech Stack:** mmap, serde_cbor (existing dep), AtomicU32 for lock-free ring buffers, SIGUSR1 for async notification.

---

## File Map

| File | Action | Purpose |
|---|---|---|
| `rill-telemetry/Cargo.toml` | Modify | Add `serde_cbor` dependency |
| `rill-telemetry/src/debug/ipc.rs` | Create | `ShmemRegion`, `ControlHeader`, ring buffer read/write |
| `rill-telemetry/src/debug/protocol.rs` | Modify | Add `Serialize, Deserialize` to `AnalyzerCommand`, `AnalyzerResponse`, related types |
| `rill-telemetry/src/debug/collector_thread.rs` | Modify | Add `ipc: Option<ShmemRegion>` param, shmem mode |
| `rill-telemetry/src/debug/mod.rs` | Modify | Add `pub mod ipc;` |
| `rill-telemetry/src/lib.rs` | Modify | Re-export ShmemRegion in prelude |
| `rill-analyzer/src/main.rs` | Modify | Add `Attach`, `Launch` subcommands |
| `rill-adrift/src/lib.rs` | Modify | Create ShmemRegion at startup under `debug` feature |
| `rill-adrift/Cargo.toml` | Modify | Add rill-telemetry `debug` feature passthrough |

---

## Phase 1: Shmem Primitives (rill-telemetry)

### Task 1.1: Add serde_cbor dependency

**Files:**
- Modify: `rill-telemetry/Cargo.toml`

- [ ] **Step 1: Check if serde_cbor is available**

```bash
grep serde_cbor rill/Cargo.toml
```

Expected: it may be a workspace dep or only in rill-graph. If not in workspace, add to `rill-telemetry/Cargo.toml`:

```toml
[dependencies]
# ... existing ...
serde_cbor = "0.11"
```

If it's already a workspace dep via `rill-graph`, add it directly:
```toml
serde_cbor = { workspace = true }
```

But check the workspace Cargo.toml first — it may not be listed as a workspace dep. In that case, add as direct dependency.

- [ ] **Step 2: Verify**

```bash
cargo check -p rill-telemetry --features debug
```

- [ ] **Step 3: Commit**

```bash
git add rill-telemetry/Cargo.toml && git commit -m 'feat(rill-telemetry): add serde_cbor for IPC serialization'
```

---

### Task 1.2: Create ShmemRegion and ring buffer primitives

**Files:**
- Create: `rill-telemetry/src/debug/ipc.rs`
- Modify: `rill-telemetry/src/debug/mod.rs`

- [ ] **Step 1: Create `ipc.rs`**

```rust
//! Shared memory IPC via /dev/shm/rill-debug-<pid>.
//!
//! Uses mmap to map a 64KB region containing atomic control flags and
//! two lock-free SPSC ring buffers for command/response serialization.
//! serde_cbor is used to serialize AnalyzerCommand/AnalyzerResponse.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

use super::protocol::{AnalyzerCommand, AnalyzerResponse};

/// Size of the shared memory region: 64KB.
const SHMEM_SIZE: usize = 65536;

/// Offset where CmdRingBuffer starts.
const CMD_BUFFER_OFFSET: usize = 64;
/// Size of CmdRingBuffer in bytes.
const CMD_BUFFER_SIZE: usize = 32704;
/// Offset where RespRingBuffer starts.
const RESP_BUFFER_OFFSET: usize = 32768;
/// Size of RespRingBuffer in bytes.
const RESP_BUFFER_SIZE: usize = 32768;

/// Maximum payload size for a single frame (4KB).
const MAX_FRAME_PAYLOAD: usize = 4096;

/// Magic bytes: "RILL"
const MAGIC: u32 = 0x52494C4C;
const VERSION: u32 = 1;

/// Flags for the ControlHeader.
pub const FLAG_PAUSED: u32 = 0x01;
pub const FLAG_ATTACHED: u32 = 0x02;
pub const FLAG_SHUTDOWN: u32 = 0x04;

/// Control structure at the start of the shared memory region.
/// All fields use repr(C) for stable layout across processes.
#[repr(C)]
struct ControlHeader {
    magic: u32,
    version: u32,
    process_pid: u64,
    debugger_pid: u64,
    flags: AtomicU32,
    cmd_capacity: u32,
    resp_capacity: u32,
    cmd_write_pos: AtomicU32,
    cmd_read_pos: AtomicU32,
    resp_write_pos: AtomicU32,
    resp_read_pos: AtomicU32,
    _reserved: [u8; 12],
}

/// Owns a mmap'd shared memory region for IPC.
/// On drop, unmaps and unlinks the backing file.
pub struct ShmemRegion {
    ptr: *mut u8,
    size: usize,
    path: String,
}

// Safety: ShmemRegion owns an exclusive mmap region. Access is single-writer
// for each ring buffer direction (SPSC). No concurrent mutable access.
unsafe impl Send for ShmemRegion {}
unsafe impl Sync for ShmemRegion {}

impl ShmemRegion {
    /// Open an existing shmem region for the given process ID (attach mode).
    pub fn open(pid: u64) -> io::Result<Self> {
        let path = shmem_path(pid);
        let file = OpenOptions::new().read(true).write(true).open(&path)?;

        // Extend file to SHMEM_SIZE if needed
        let meta = file.metadata()?;
        if meta.len() < SHMEM_SIZE as u64 {
            file.set_len(SHMEM_SIZE as u64)?;
        }

        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                SHMEM_SIZE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        // Verify magic and version
        let header = unsafe { &*(ptr as *const ControlHeader) };
        if header.magic != MAGIC {
            unsafe { libc::munmap(ptr, SHMEM_SIZE) };
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid magic"));
        }
        if header.version != VERSION {
            unsafe { libc::munmap(ptr, SHMEM_SIZE) };
            return Err(io::Error::new(io::ErrorKind::InvalidData, "version mismatch"));
        }

        Ok(Self {
            ptr: ptr as *mut u8,
            size: SHMEM_SIZE,
            path,
        })
    }

    /// Create a new shmem region for this process (listen mode).
    pub fn create() -> io::Result<Self> {
        let pid = unsafe { libc::getpid() } as u64;
        let path = shmem_path(pid);

        // Create file
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        file.set_len(SHMEM_SIZE as u64)?;

        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                SHMEM_SIZE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            let _ = fs::remove_file(&path);
            return Err(io::Error::last_os_error());
        }

        // Initialize ControlHeader
        let header = unsafe { &mut *(ptr as *mut ControlHeader) };
        header.magic = MAGIC;
        header.version = VERSION;
        header.process_pid = pid;
        header.debugger_pid = 0;
        header.flags = AtomicU32::new(0);
        header.cmd_capacity = CMD_BUFFER_SIZE as u32;
        header.resp_capacity = RESP_BUFFER_SIZE as u32;
        header.cmd_write_pos = AtomicU32::new(0);
        header.cmd_read_pos = AtomicU32::new(0);
        header.resp_write_pos = AtomicU32::new(0);
        header.resp_read_pos = AtomicU32::new(0);

        Ok(Self {
            ptr: ptr as *mut u8,
            size: SHMEM_SIZE,
            path,
        })
    }

    /// Open from environment variable (used by child process in launch mode).
    pub fn open_from_env(var: &str) -> io::Result<Self> {
        let path = std::env::var(var).map_err(|_| {
            io::Error::new(io::ErrorKind::NotFound, format!("env var {} not set", var))
        })?;
        let pid: u64 = path
            .rsplit('-')
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid shmem path"))?;
        // Verify path matches
        let expected = shmem_path(pid);
        if path != expected {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "shmem path mismatch"));
        }
        Self::open(pid)
    }

    /// Get a reference to the ControlHeader.
    fn header(&self) -> &ControlHeader {
        unsafe { &*(self.ptr as *const ControlHeader) }
    }

    /// Returns the PID of the owning rill process.
    pub fn process_pid(&self) -> u64 {
        self.header().process_pid
    }

    /// Returns the PID of the connected debugger (0 if none).
    pub fn debugger_pid(&self) -> u64 {
        self.header().debugger_pid
    }

    /// Set the debugger PID in the header.
    pub fn set_debugger_pid(&self, pid: u64) {
        unsafe {
            let header = &mut *(self.ptr as *mut ControlHeader);
            header.debugger_pid = pid;
        }
    }

    /// Check if a flag is set.
    pub fn has_flag(&self, flag: u32) -> bool {
        self.header().flags.load(Ordering::Acquire) & flag != 0
    }

    /// Set a flag.
    pub fn set_flag(&self, flag: u32) {
        self.header().flags.fetch_or(flag, Ordering::Release);
    }

    /// Clear a flag.
    pub fn clear_flag(&self, flag: u32) {
        self.header().flags.fetch_and(!flag, Ordering::Release);
    }

    // ── Command ring buffer (debugger → process) ──

    /// Write a command into the ring buffer.
    /// Returns true on success, false if buffer is full.
    pub fn write_command(&self, cmd: &AnalyzerCommand) -> bool {
        let payload = match serialize_frame(cmd) {
            Ok(p) => p,
            Err(_) => return false,
        };
        self.write_ring_buffer(
            CMD_BUFFER_OFFSET,
            CMD_BUFFER_SIZE,
            |h| h.cmd_write_pos.load(Ordering::Acquire),
            |h| h.cmd_read_pos.load(Ordering::Acquire),
            |h, v| h.cmd_write_pos.store(v, Ordering::Release),
            |h, v| h.cmd_read_pos.store(v, Ordering::Release),
            &payload,
        )
    }

    /// Read a command from the ring buffer.
    pub fn read_command(&self) -> Option<AnalyzerCommand> {
        let payload = self.read_ring_buffer(
            CMD_BUFFER_OFFSET,
            CMD_BUFFER_SIZE,
            |h| h.cmd_write_pos.load(Ordering::Acquire),
            |h| h.cmd_read_pos.load(Ordering::Acquire),
            |h, v| h.cmd_write_pos.store(v, Ordering::Release),
            |h, v| h.cmd_read_pos.store(v, Ordering::Release),
        )?;
        deserialize_frame(&payload).ok()
    }

    // ── Response ring buffer (process → debugger) ──

    /// Write a response into the ring buffer.
    pub fn write_response(&self, resp: &AnalyzerResponse) -> bool {
        let payload = match serialize_frame(resp) {
            Ok(p) => p,
            Err(_) => return false,
        };
        self.write_ring_buffer(
            RESP_BUFFER_OFFSET,
            RESP_BUFFER_SIZE,
            |h| h.resp_write_pos.load(Ordering::Acquire),
            |h| h.resp_read_pos.load(Ordering::Acquire),
            |h, v| h.resp_write_pos.store(v, Ordering::Release),
            |h, v| h.resp_read_pos.store(v, Ordering::Release),
            &payload,
        )
    }

    /// Read a response from the ring buffer.
    pub fn read_response(&self) -> Option<AnalyzerResponse> {
        let payload = self.read_ring_buffer(
            RESP_BUFFER_OFFSET,
            RESP_BUFFER_SIZE,
            |h| h.resp_write_pos.load(Ordering::Acquire),
            |h| h.resp_read_pos.load(Ordering::Acquire),
            |h, v| h.resp_write_pos.store(v, Ordering::Release),
            |h, v| h.resp_read_pos.store(v, Ordering::Release),
        )?;
        deserialize_frame(&payload).ok()
    }

    /// Send SIGUSR1 to the rill process (caller must be the debugger).
    pub fn notify_process(&self) {
        let pid = self.header().process_pid as libc::pid_t;
        unsafe { libc::kill(pid, libc::SIGUSR1) };
    }

    // ── Internal ring buffer operations ──

    /// Write `payload` into a ring buffer at `offset` with `capacity` bytes.
    fn write_ring_buffer(
        &self,
        offset: usize,
        capacity: usize,
        get_write: impl Fn(&ControlHeader) -> u32,
        get_read: impl Fn(&ControlHeader) -> u32,
        set_write: impl Fn(&ControlHeader, u32),
        _set_read: impl Fn(&ControlHeader, u32),
        payload: &[u8],
    ) -> bool {
        let header = self.header();
        let frame_len = 2 + payload.len(); // u16 len + data
        if frame_len > capacity as usize || payload.len() > MAX_FRAME_PAYLOAD {
            return false;
        }

        let mut write_pos = get_write(header) as usize;
        let read_pos = get_read(header) as usize;

        // Check space (with wrap-around)
        let available = if write_pos >= read_pos {
            capacity - (write_pos - read_pos)
        } else {
            read_pos - write_pos
        };
        if available < frame_len + 2 {
            return false; // buffer full
        }

        // Handle wrap-around
        if write_pos + frame_len > capacity {
            // Write zero-length marker at tail
            let base = unsafe { self.ptr.add(offset) };
            unsafe {
                ptr::write(base.add(write_pos) as *mut u16, 0u16);
            }
            set_write(header, 0);
            write_pos = 0;
        }

        let base = unsafe { self.ptr.add(offset) };
        let len = payload.len() as u16;
        unsafe {
            ptr::write(base.add(write_pos) as *mut u16, len);
            ptr::copy_nonoverlapping(
                payload.as_ptr(),
                base.add(write_pos + 2),
                payload.len(),
            );
        }
        set_write(header, (write_pos + frame_len) as u32 % capacity as u32);
        true
    }

    /// Read the next frame from a ring buffer. Returns None if empty.
    fn read_ring_buffer(
        &self,
        offset: usize,
        capacity: usize,
        get_write: impl Fn(&ControlHeader) -> u32,
        get_read: impl Fn(&ControlHeader) -> u32,
        _set_write: impl Fn(&ControlHeader, u32),
        set_read: impl Fn(&ControlHeader, u32),
    ) -> Option<Vec<u8>> {
        let header = self.header();
        let write_pos = get_write(header) as usize;
        let mut read_pos = get_read(header) as usize;

        if read_pos == write_pos {
            return None;
        }

        let base = unsafe { self.ptr.add(offset) };
        let len = unsafe { ptr::read(base.add(read_pos) as *const u16) } as usize;

        if len == 0 {
            // Wrap-around marker: reset to start
            set_read(header, 0);
            read_pos = 0;
            if read_pos == write_pos {
                return None;
            }
            let len = unsafe { ptr::read(base.add(read_pos) as *const u16) } as usize;
            if len == 0 || len > MAX_FRAME_PAYLOAD {
                return None;
            }
            let mut payload = vec![0u8; len];
            unsafe {
                ptr::copy_nonoverlapping(
                    base.add(read_pos + 2),
                    payload.as_mut_ptr(),
                    len,
                );
            }
            set_read(header, (read_pos + 2 + len) as u32 % capacity as u32);
            return Some(payload);
        }

        if len == 0 || len > MAX_FRAME_PAYLOAD {
            return None;
        }

        let mut payload = vec![0u8; len];
        unsafe {
            ptr::copy_nonoverlapping(base.add(read_pos + 2), payload.as_mut_ptr(), len);
        }
        set_read(header, (read_pos + 2 + len) as u32 % capacity as u32);
        Some(payload)
    }
}

impl Drop for ShmemRegion {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, self.size);
        }
        let _ = fs::remove_file(&self.path);
    }
}

/// Build the shmem file path: /dev/shm/rill-debug-<pid>
fn shmem_path(pid: u64) -> String {
    format!("/dev/shm/rill-debug-{}", pid)
}

/// Serialize a value to CBOR bytes.
fn serialize_frame<T: Serialize>(value: &T) -> io::Result<Vec<u8>> {
    serde_cbor::to_vec(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Deserialize a value from CBOR bytes.
fn deserialize_frame<'a, T: Deserialize<'a>>(data: &'a [u8]) -> io::Result<T> {
    serde_cbor::from_slice(data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
```

- [ ] **Step 2: Wire module**

In `rill-telemetry/src/debug/mod.rs`, add:
```rust
#[cfg(feature = "debug")]
pub mod ipc;
```

The module is already inside `#[cfg(feature = "debug")]` context since `mod.rs` is guarded in lib.rs.

- [ ] **Step 3: Verify**

```bash
cargo check -p rill-telemetry --features debug
```

- [ ] **Step 4: Commit**

```bash
git add rill-telemetry/src/debug/ipc.rs rill-telemetry/src/debug/mod.rs
git commit -m 'feat(rill-telemetry): add ShmemRegion with lock-free ring buffers'
```

---

### Task 1.3: Add Serialize/Deserialize to protocol types

**Files:**
- Modify: `rill-telemetry/src/debug/protocol.rs`

- [ ] **Step 1: Add serde derives**

Update the derives on key types:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnalyzerCommand { ... }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnalyzerResponse { ... }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo { ... }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeInfo { ... }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandLogEntry { ... }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputMode { ... }
```

Add `use serde::{Deserialize, Serialize};` if not already present.

- [ ] **Step 2: Verify**

```bash
cargo check -p rill-telemetry --features debug
```

- [ ] **Step 3: Commit**

```bash
git add rill-telemetry/src/debug/protocol.rs
git commit -m 'feat(rill-telemetry): add Serialize/Deserialize to protocol types'
```

---

## Phase 2: IPC Mode in CollectorThread

### Task 2.1: Add shmem mode to CollectorThread

**Files:**
- Modify: `rill-telemetry/src/debug/collector_thread.rs`

- [ ] **Step 1: Add imports**

At the top of the file, add:

```rust
use super::ipc::ShmemRegion;
```

- [ ] **Step 2: Update `spawn()` signature**

Add `shmem: Option<ShmemRegion>` parameter to `spawn()`:

```rust
    pub fn spawn(
        config: AnalyzerConfig,
        probe_states: Arc<DashMap<ProbeId, ProbeState>>,
        probe_queues: Vec<Arc<SpscQueue<ProbeFrame, 64>>>,
        command_queue: Arc<SpscQueue<CommandFrame, 256>>,
        probe_slots: Vec<Arc<ProbeSlot>>,
        debug_control: DebugControl,
        resp_tx: mpsc::Sender<AnalyzerResponse>,
        shmem: Option<ShmemRegion>,
    ) -> (Self, mpsc::Sender<AnalyzerCommand>) {
```

- [ ] **Step 3: Modify the thread loop**

After the `AnalyzerCommand::Quit` handling (which sends a final response and breaks), add the shmem-conditional logic. The loop currently does:
1. Process mpsc commands via `try_recv`
2. Drain signal queues
3. Drain command queue

Add after step 1 (command processing) and before step 2:

```rust
                // Check shmem flags
                if let Some(ref shmem) = shmem {
                    if shmem.has_flag(super::ipc::FLAG_SHUTDOWN) {
                        break;
                    }
                    if shmem.has_flag(super::ipc::FLAG_PAUSED) {
                        debug_control.pause();
                    } else {
                        // Only cont if previously paused (avoid spurious resume)
                    }
                }
```

And after step 3 (command draining), add shmem command reading:

```rust
                // Read commands from shmem (if in IPC mode)
                if let Some(ref shmem) = shmem {
                    while let Some(cmd) = shmem.read_command() {
                        if matches!(cmd, AnalyzerCommand::Quit) {
                            break;
                        }
                        let resp = state_manager.handle_command(cmd);
                        // Write response to shmem instead of mpsc
                        shmem.write_response(&resp);
                    }
                }
```

- [ ] **Step 4: Verify**

```bash
cargo check -p rill-telemetry --features debug
```

- [ ] **Step 5: Commit**

```bash
git add rill-telemetry/src/debug/collector_thread.rs
git commit -m 'feat(rill-telemetry): add shmem mode to CollectorThread'
```

---

## Phase 3: CLI — Attach + Launch

### Task 3.1: Add Attach subcommand

**Files:**
- Modify: `rill-analyzer/src/main.rs`

- [ ] **Step 1: Add `Attach` and `Launch` variants to Commands**

```rust
#[derive(Subcommand)]
enum Commands {
    Run {
        graph: PathBuf,
        #[arg(long)] no_repl: bool,
        #[arg(long)] json: bool,
        #[arg(long)] log: Option<PathBuf>,
        #[arg(long)] script: Option<PathBuf>,
    },

    /// Connect to a running rill process via shared memory.
    Attach {
        /// PID of the rill process.
        pid: u64,
        #[arg(long)] json: bool,
    },

    /// Launch a target and connect the debugger.
    ///
    /// If TARGET ends with .json, compiles it as a serialized graph and runs via drift.
    /// If TARGET ends with .rll, compiles the rill-lang DSL and runs via drift.
    /// Otherwise, TARGET is executed directly as a binary.
    /// Use "--" before TARGET to run an arbitrary command.
    Launch {
        /// Graph file (.json), rill-lang source (.rll), or binary.
        target: String,
        /// Additional arguments for the launched process.
        #[arg(last = true)]
        args: Vec<String>,
        #[arg(long)] json: bool,
    },
}
```

- [ ] **Step 2: Implement `Attach` in main()**

```rust
        Commands::Attach { pid, json } => {
            if json {
                println!("{} --json mode not yet implemented", "[rill-analyzer]".yellow());
            }

            let shmem = rill_telemetry::debug::ipc::ShmemRegion::open(pid)
                .unwrap_or_else(|e| {
                    eprintln!("ERROR: cannot attach to PID {}: {}", pid, e);
                    std::process::exit(1);
                });

            // Verify the process exists
            if shmem.process_pid() != pid {
                eprintln!("ERROR: shmem process_pid {} != requested {}", shmem.process_pid(), pid);
                std::process::exit(1);
            }

            // Register as debugger
            let my_pid = std::process::id() as u64;
            shmem.set_debugger_pid(my_pid);
            shmem.set_flag(rill_telemetry::debug::ipc::FLAG_ATTACHED);

            println!(
                "{} attached to process {} (shmem @ /dev/shm/rill-debug-{})",
                "[rill-analyzer]".green(), pid, pid
            );

            // REPL loop: read stdin, write commands to shmem, read responses
            repl_loop_shmem(shmem);
        }
```

- [ ] **Step 3: Implement `Launch` in main()**

```rust
        Commands::Launch { target, args, json } => {
            if json {
                println!("{} --json mode not yet implemented", "[rill-analyzer]".yellow());
            }

            let shmem = rill_telemetry::debug::ipc::ShmemRegion::create()
                .unwrap_or_else(|e| {
                    eprintln!("ERROR: cannot create shmem: {}", e);
                    std::process::exit(1);
                });

            let my_pid = std::process::id() as u64;
            let shmem_env = format!("RILL_DEBUG_SHMEM=/dev/shm/rill-debug-{}", my_pid);

            // Resolve target
            let child_pid = if target.ends_with(".json") || target.ends_with(".rll") {
                // For .rll: compile first, write temp .json, then launch drift
                // For .json: launch drift --graph directly
                if target.ends_with(".rll") {
                    println!(
                        "{} compiling .rll source...",
                        "[rill-analyzer]".green()
                    );
                    // Compile .rll → temp .json
                    let src = std::fs::read_to_string(&target).unwrap_or_else(|e| {
                        eprintln!("ERROR: cannot read {}: {}", target, e);
                        std::process::exit(1);
                    });
                    let registry = rill_lang::builtin::Registry::<f64>::new();
                    let engine = rill_lang::compile_graph(&src, &registry, 44100.0)
                        .unwrap_or_else(|e| {
                            eprintln!("ERROR: compilation failed: {:?}", e);
                            std::process::exit(1);
                        });
                    // Serialize schedule to temp file
                    let tmp = std::env::temp_dir().join(format!("rill-debug-{}.json", my_pid));
                    // TODO: serialize ScheduledGraph to JSON, write to tmp
                    let graph_arg = format!("--graph={}", target);
                    launch_child("drift", &[&graph_arg], &shmem_env)
                } else {
                    let graph_arg = format!("--graph={}", target);
                    launch_child("drift", &[&graph_arg], &shmem_env)
                }
            } else {
                // Arbitrary binary
                let args_strs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                launch_child(&target, &args_strs, &shmem_env)
            };

            println!(
                "{} launched process {} (shmem @ /dev/shm/rill-debug-{})",
                "[rill-analyzer]".green(), child_pid, my_pid
            );
            println!("{} waiting for debug attachment...", "[rill-analyzer]".yellow());

            // Wait for FLAG_ATTACHED
            while !shmem.has_flag(rill_telemetry::debug::ipc::FLAG_ATTACHED) {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }

            repl_loop_shmem(shmem);
        }
```

- [ ] **Step 4: Add helper functions**

```rust
/// Fork + exec a child process. Returns the child PID.
fn launch_child(binary: &str, args: &[&str], shmem_env: &str) -> u32 {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(binary);
    cmd.args(args);
    cmd.env("RILL_DEBUG_SHMEM", shmem_env);
    // Don't inherit stdin — child will have its own
    cmd.stdin(std::process::Stdio::null());
    let child = cmd.spawn().unwrap_or_else(|e| {
        eprintln!("ERROR: cannot launch '{}': {}", binary, e);
        std::process::exit(1);
    });
    child.id()
}

/// REPL loop using shmem for command/response transport.
fn repl_loop_shmem(shmem: rill_telemetry::debug::ipc::ShmemRegion) {
    use std::io::{self, Write};
    use colored::Colorize;

    println!("{} type 'help' for commands", "[rill-analyzer]".green());

    loop {
        print!("{} ", "(rla)".blue().bold());
        io::stdout().flush().ok();

        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "q" || line == "quit" {
            shmem.set_flag(rill_telemetry::debug::ipc::FLAG_SHUTDOWN);
            break;
        }
        if line == "h" || line == "help" {
            println!("  break <probe>  continue  step  info nodes  info probes  print <probe>  quit");
            continue;
        }

        // Parse command (simple for MVP)
        let cmd = if line.starts_with("break") {
            let probe = line.split_whitespace().nth(1).unwrap_or("0");
            let probe_id: u32 = probe.parse().unwrap_or(0);
            rill_telemetry::debug::protocol::AnalyzerCommand::SetBreakpoint { probe_id }
        } else if line == "continue" || line == "c" {
            shmem.clear_flag(rill_telemetry::debug::ipc::FLAG_PAUSED);
            rill_telemetry::debug::protocol::AnalyzerCommand::Continue
        } else if line == "step" || line == "s" {
            rill_telemetry::debug::protocol::AnalyzerCommand::Step { count: 1 }
        } else if line.starts_with("print") || line.starts_with("p") {
            let probe = line.split_whitespace().nth(1).unwrap_or("0");
            let probe_id: u32 = probe.parse().unwrap_or(0);
            rill_telemetry::debug::protocol::AnalyzerCommand::GetProbeValue { probe_id }
        } else if line == "info nodes" {
            rill_telemetry::debug::protocol::AnalyzerCommand::ListNodes
        } else if line == "info probes" {
            rill_telemetry::debug::protocol::AnalyzerCommand::ListProbes
        } else {
            rill_telemetry::debug::protocol::AnalyzerCommand::Pause
        };

        shmem.write_command(&cmd);
        shmem.notify_process();

        // Poll responses
        std::thread::sleep(std::time::Duration::from_millis(10));
        while let Some(resp) = shmem.read_response() {
            match resp {
                rill_telemetry::debug::protocol::AnalyzerResponse::Ok { message } => {
                    println!("  {}", message);
                }
                rill_telemetry::debug::protocol::AnalyzerResponse::ProbeValue { name, value, .. } => {
                    println!("  {} = {:.6}", name, value);
                }
                rill_telemetry::debug::protocol::AnalyzerResponse::NodeList { nodes } => {
                    for (i, n) in nodes.iter().enumerate() {
                        println!("  #{:<4} {:<16} in:{} out:{}", i, n.name, n.num_inputs, n.num_outputs);
                    }
                }
                rill_telemetry::debug::protocol::AnalyzerResponse::ProbeList { probes } => {
                    for p in &probes {
                        let status = if p.breakpoint { "BREAK" } else if p.enabled { "ON" } else { "OFF" };
                        println!("  [{}] {} value={:?}", status, p.name, p.last_value);
                    }
                }
                rill_telemetry::debug::protocol::AnalyzerResponse::Paused { probe_name, reason } => {
                    println!("{} {} {}", "BREAK:".red().bold(), probe_name.unwrap_or_default(), reason);
                }
                rill_telemetry::debug::protocol::AnalyzerResponse::Error { message } => {
                    println!("{} {}", "ERROR:".red(), message);
                }
                _ => {}
            }
        }
    }
}
```

- [ ] **Step 5: Verify**

```bash
cargo check -p rill-analyzer
```

Fix any compilation errors. The `repl_loop_shmem` function has prototype-level command parsing — this is sufficient for MVP.

- [ ] **Step 6: Commit**

```bash
git add rill-analyzer/src/main.rs
git commit -m 'feat(rill-analyzer): add Attach and Launch subcommands with shmem transport'
```

---

## Phase 4: Host Initialization

### Task 4.1: Create shmem in rill-adrift at startup

**Files:**
- Modify: `rill-adrift/Cargo.toml`
- Modify: `rill-adrift/src/lib.rs`

- [ ] **Step 1: Add debug feature passthrough in Cargo.toml**

In `rill-adrift/Cargo.toml`, add to `[features]`:

```toml
[features]
# ... existing features ...
debug = ["rill-telemetry/debug"]
```

Check that `rill-telemetry` is already in the dependencies (it should be, behind `telemetry` feature). If it needs to be always-on for debug, add it unconditionally:

```toml
[dependencies]
# ... existing ...
rill-telemetry = { workspace = true, optional = true }  # if not already present
```

If `rill-telemetry` is already behind `telemetry` feature, add `debug` as a separate feature that also enables it:

```toml
debug = ["rill-telemetry", "rill-telemetry/debug"]
```

- [ ] **Step 2: Add shmem initialization in lib.rs**

In `rill-adrift/src/lib.rs`, after the existing re-exports, add:

```rust
#[cfg(feature = "debug")]
pub mod debug_init {
    use rill_telemetry::debug::ipc::ShmemRegion;

    /// Create the shared memory region for IPC debugging.
    /// Called at host application startup. Returns None if creation fails.
    pub fn init_shmem() -> Option<ShmemRegion> {
        match ShmemRegion::create() {
            Ok(shmem) => {
                eprintln!("[rill-debug] shmem created: /dev/shm/rill-debug-{}", std::process::id());
                Some(shmem)
            }
            Err(e) => {
                eprintln!("[rill-debug] failed to create shmem: {}", e);
                None
            }
        }
    }

    /// Open shmem from environment variable (for child processes launched by rill-analyzer).
    pub fn init_shmem_from_env() -> Option<ShmemRegion> {
        let shmem = ShmemRegion::open_from_env("RILL_DEBUG_SHMEM").ok()?;
        shmem.set_debugger_pid(0); // Will be set by debugger
        shmem.set_flag(rill_telemetry::debug::ipc::FLAG_ATTACHED);
        eprintln!(
            "[rill-debug] shmem opened from env: /dev/shm/rill-debug-{}",
            shmem.process_pid()
        );
        Some(shmem)
    }
}
```

- [ ] **Step 3: Verify**

```bash
cargo check -p rill-adrift --features debug
cargo check -p rill-adrift
```

Without `debug` feature, the module should not be compiled.

- [ ] **Step 4: Commit**

```bash
git add rill-adrift/Cargo.toml rill-adrift/src/lib.rs
git commit -m 'feat(rill-adrift): add shmem initialization under debug feature'
```

---

## Phase 5: Integration & Polish

### Task 5.1: Workspace check and tests

- [ ] **Step 1: Full workspace compilation**

```bash
cargo check --workspace
```

- [ ] **Step 2: Clippy on modified crates**

```bash
cargo clippy -p rill-telemetry --features debug -p rill-analyzer
```

Fix any new warnings.

- [ ] **Step 3: Run tests**

```bash
cargo test -p rill-telemetry --features debug
cargo test -p rill-analyzer
```

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m 'chore: workspace check and clippy clean for IPC implementation'
```

---

## Plan Summary

| Phase | Tasks | Crate | Key Deliverable |
|---|---|---|---|
| 1 | 1.1–1.3 | `rill-telemetry` | `ShmemRegion` with lock-free ring buffers, serde on protocol types |
| 2 | 2.1 | `rill-telemetry` | `CollectorThread` shmem mode |
| 3 | 3.1 | `rill-analyzer` | `Attach`, `Launch` CLI subcommands, shmem REPL loop |
| 4 | 4.1 | `rill-adrift` | `debug_init` module, shmem creation at startup |
| 5 | 5.1 | All | Workspace check, clippy, tests |

**Total tasks: 6**

**Not in MVP:** `.rll` compilation in launch mode (skeleton present, needs full pipeline), signal-based wakeup for CollectorThread (uses sleep polling), cleanup of stale `/dev/shm` files.
