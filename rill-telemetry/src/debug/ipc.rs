//! Shared memory IPC via /dev/shm/rill-debug-<pid>.
//!
//! Uses mmap to map a 64KB region containing atomic control flags and
//! two lock-free SPSC ring buffers for command/response serialization.
//! serde_cbor is used to serialize AnalyzerCommand/AnalyzerResponse.

#![allow(missing_docs)]

use std::ffi::{c_int, c_void};
use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

use super::protocol::{AnalyzerCommand, AnalyzerResponse};

#[allow(non_camel_case_types)]
type size_t = usize;
#[allow(non_camel_case_types)]
type off_t = i64;
#[allow(non_camel_case_types)]
type pid_t = i32;

extern "C" {
    fn mmap(
        addr: *mut c_void,
        len: size_t,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        offset: off_t,
    ) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: size_t) -> c_int;
    fn kill(pid: pid_t, sig: c_int) -> c_int;
    fn getpid() -> pid_t;
}

const PROT_READ: c_int = 0x1;
const PROT_WRITE: c_int = 0x2;
const MAP_SHARED: c_int = 0x01;
const MAP_FAILED: *mut c_void = !0 as *mut c_void;
const SIGUSR1: c_int = 10;

const SHMEM_SIZE: usize = 65536;

const CMD_BUFFER_OFFSET: usize = 64;
const CMD_BUFFER_SIZE: usize = 32704;
const RESP_BUFFER_OFFSET: usize = 32768;
const RESP_BUFFER_SIZE: usize = 32768;

const MAX_FRAME_PAYLOAD: usize = 4096;

const MAGIC: u32 = 0x52494C4C;
const VERSION: u32 = 1;

pub const FLAG_PAUSED: u32 = 0x01;
pub const FLAG_ATTACHED: u32 = 0x02;
pub const FLAG_SHUTDOWN: u32 = 0x04;

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
    pub fn open(pid: u64) -> io::Result<Self> {
        let path = shmem_path(pid);
        let file = OpenOptions::new().read(true).write(true).open(&path)?;

        let meta = file.metadata()?;
        if meta.len() < SHMEM_SIZE as u64 {
            file.set_len(SHMEM_SIZE as u64)?;
        }

        let ptr = unsafe {
            mmap(
                ptr::null_mut(),
                SHMEM_SIZE,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr == MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        let header = unsafe { &*(ptr as *const ControlHeader) };
        if header.magic != MAGIC {
            unsafe {
                munmap(ptr, SHMEM_SIZE);
            }
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid magic"));
        }
        if header.version != VERSION {
            unsafe {
                munmap(ptr, SHMEM_SIZE);
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "version mismatch",
            ));
        }

        Ok(Self {
            ptr: ptr as *mut u8,
            size: SHMEM_SIZE,
            path,
        })
    }

    pub fn create() -> io::Result<Self> {
        let pid = unsafe { getpid() } as u64;
        let path = shmem_path(pid);

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        file.set_len(SHMEM_SIZE as u64)?;

        let ptr = unsafe {
            mmap(
                ptr::null_mut(),
                SHMEM_SIZE,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr == MAP_FAILED {
            let _ = fs::remove_file(&path);
            return Err(io::Error::last_os_error());
        }

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

    pub fn open_from_env(var: &str) -> io::Result<Self> {
        let path = std::env::var(var).map_err(|_| {
            io::Error::new(io::ErrorKind::NotFound, format!("env var {} not set", var))
        })?;
        let pid: u64 = path
            .rsplit('-')
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid shmem path"))?;
        let expected = shmem_path(pid);
        if path != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "shmem path mismatch",
            ));
        }
        Self::open(pid)
    }

    fn header(&self) -> &ControlHeader {
        unsafe { &*(self.ptr as *const ControlHeader) }
    }

    pub fn process_pid(&self) -> u64 {
        self.header().process_pid
    }

    pub fn debugger_pid(&self) -> u64 {
        self.header().debugger_pid
    }

    pub fn set_debugger_pid(&self, pid: u64) {
        unsafe {
            let header = &mut *(self.ptr as *mut ControlHeader);
            header.debugger_pid = pid;
        }
    }

    pub fn has_flag(&self, flag: u32) -> bool {
        self.header().flags.load(Ordering::Acquire) & flag != 0
    }

    pub fn set_flag(&self, flag: u32) {
        self.header().flags.fetch_or(flag, Ordering::Release);
    }

    pub fn clear_flag(&self, flag: u32) {
        self.header().flags.fetch_and(!flag, Ordering::Release);
    }

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

    pub fn notify_process(&self) {
        let pid = self.header().process_pid as pid_t;
        unsafe {
            kill(pid, SIGUSR1);
        }
    }

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
        let frame_len = 2 + payload.len();
        if frame_len > capacity || payload.len() > MAX_FRAME_PAYLOAD {
            return false;
        }

        let mut write_pos = get_write(header) as usize;
        let read_pos = get_read(header) as usize;

        let available = if write_pos >= read_pos {
            capacity - (write_pos - read_pos)
        } else {
            read_pos - write_pos
        };
        if available < frame_len + 2 {
            return false;
        }

        if write_pos + frame_len > capacity {
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
            ptr::copy_nonoverlapping(payload.as_ptr(), base.add(write_pos + 2), payload.len());
        }
        set_write(header, (write_pos + frame_len) as u32 % capacity as u32);
        true
    }

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
                ptr::copy_nonoverlapping(base.add(read_pos + 2), payload.as_mut_ptr(), len);
            }
            set_read(header, (read_pos + 2 + len) as u32 % capacity as u32);
            return Some(payload);
        }

        if len > MAX_FRAME_PAYLOAD {
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
            munmap(self.ptr as *mut c_void, self.size);
        }
        let _ = fs::remove_file(&self.path);
    }
}

fn shmem_path(pid: u64) -> String {
    format!("/dev/shm/rill-debug-{}", pid)
}

fn serialize_frame<T: Serialize>(value: &T) -> io::Result<Vec<u8>> {
    serde_cbor::to_vec(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn deserialize_frame<'a, T: Deserialize<'a>>(data: &'a [u8]) -> io::Result<T> {
    serde_cbor::from_slice(data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
