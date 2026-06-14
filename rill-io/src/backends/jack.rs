//! JACK backend — signal data flows through `IoRingBuffer` pairs
//! bridged by `DeinterleavedView`. The process callback receives a
//! `ClockTick` carrying the view — graph nodes read/write through
//! `tick.view` uniformly.
//!
//! `run()` — blocking: creates JACK client, activates, enters poll
//! loop. Process callback runs on JACK RT thread.
//! `stop()` sets `running = false` and deactivates the client.

use std::cell::UnsafeCell;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use jack::{AudioIn, AudioOut, Client, ClientOptions, Control, Port, ProcessHandler, ProcessScope};

use crate::buffer::IoRingBuffer;
use crate::buffer_view::DeinterleavedView;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use rill_core::io::IoBackend;
use rill_core::time::{ClockTick, SystemClock};
use rill_core::traits::buffer_view::BufferView;

/// Maximum interleaved buffer size: 4096 frames × 2 channels = 8192 floats.
/// Stack-allocated to avoid heap allocation in the RT path.
const MAX_INTERLEAVED: usize = 8192;

/// Callback slot — stores the process callback via raw pointer for `Send`-safe
/// single-threaded access from the JACK RT callback.
#[derive(Copy, Clone)]
struct CbSlot(usize);

impl CbSlot {
    fn new() -> Self {
        Self(Box::into_raw(Box::new(None::<Box<dyn FnMut(&ClockTick)>>)) as usize)
    }
    unsafe fn set(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        (*(self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>)) = Some(cb);
    }
    unsafe fn call(&self, tick: &ClockTick) {
        if let Some(ref mut cb) = *(self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>) {
            cb(tick);
        }
    }
    unsafe fn drop_box(&self) {
        drop(Box::from_raw(
            self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>,
        ));
    }
}

/// JACK signal backend.
pub struct JackBackend {
    config: AudioConfig,
    input_ring: Arc<IoRingBuffer>,
    output_ring: Arc<IoRingBuffer>,
    view: Arc<dyn BufferView>,
    process_cb: CbSlot,
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    sample_pos: Arc<AtomicU64>,
    /// Stores the active JACK client handle.
    /// Set once in `run()` (I/O thread), taken once in `stop()` (control thread).
    active_client: UnsafeCell<Option<jack::AsyncClient<(), JackProcessHandler>>>,
    /// Optional shared system clock for JACK transport sync.
    /// When set, the process callback queries JACK transport state and
    /// updates BPM atomically.
    sys_clock: Option<Arc<SystemClock>>,
}

impl fmt::Debug for JackBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JackBackend")
            .field("config", &self.config)
            .field("running", &self.running)
            .finish()
    }
}

impl JackBackend {
    /// Create a new JACK backend.
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            return Err(IoError::Unsupported(
                "JACK is only available on Linux and macOS".into(),
            ));
        }

        let input_channels = config.input_channels as usize;
        let output_channels = config.output_channels as usize;
        let block_size = config.buffer_size as usize;
        let ring_cap = (block_size * output_channels.max(1) * 32).next_power_of_two();
        let in_ring_cap = (block_size * input_channels.max(1) * 32).next_power_of_two();

        let input_ring = Arc::new(IoRingBuffer::new(in_ring_cap));
        let output_ring = Arc::new(IoRingBuffer::new(ring_cap));
        let view: Arc<dyn BufferView> = Arc::new(DeinterleavedView::new(
            input_ring.clone(),
            output_ring.clone(),
            input_channels,
            output_channels,
            block_size,
        ));

        Ok(Self {
            config,
            input_ring,
            output_ring,
            view,
            process_cb: CbSlot::new(),
            xruns: Arc::new(AtomicU32::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            sample_pos: Arc::new(AtomicU64::new(0)),
            active_client: UnsafeCell::new(None),
            sys_clock: None,
        })
    }

    /// Attach a shared [`SystemClock`] for JACK transport sync.
    ///
    /// When set, the JACK process callback queries `client.transport().query()`
    /// on every block and updates the clock's BPM atomically if the transport
    /// is rolling and BBT data is valid.
    ///
    /// Call this **before** `run()` so the process handler receives the clock.
    pub fn set_system_clock(&mut self, clock: Arc<SystemClock>) {
        self.sys_clock = Some(clock);
    }
}

// ─── ProcessHandler ──────────────────────────────────────────────────────

struct JackProcessHandler {
    process_cb: CbSlot,
    out_ports: Vec<Port<AudioOut>>,
    in_ports: Vec<Port<AudioIn>>,
    in_ch: usize,
    out_ch: usize,
    input_ring: Arc<IoRingBuffer>,
    output_ring: Arc<IoRingBuffer>,
    view: Arc<dyn BufferView>,
    sample_pos: Arc<AtomicU64>,
    sample_rate: f32,
    block_size: usize,
    sys_clock: Option<Arc<SystemClock>>,
}

impl ProcessHandler for JackProcessHandler {
    fn process(&mut self, client: &Client, ps: &ProcessScope) -> Control {
        // JACK Transport sync — update BPM from transport master
        if let Some(ref clock) = self.sys_clock {
            if let Ok(state) = client.transport().query() {
                if state.pos.valid_bbt() {
                    if let Some(bbt) = state.pos.bbt() {
                        clock.set_bpm(bbt.bpm);
                    }
                }
            }
        }

        let nframes = ps.n_frames() as usize;
        let chunk = self.block_size;

        // Capture: write all input to ring buffer (one interleaved write)
        if !self.in_ports.is_empty() && self.in_ch > 0 {
            let total_samps = nframes * self.in_ch;
            let cap = total_samps.min(MAX_INTERLEAVED);
            let mut interleaved = [0.0f32; MAX_INTERLEAVED];
            for i in 0..nframes {
                for ch in 0..self.in_ch {
                    if ch < self.in_ports.len() {
                        let src = self.in_ports[ch].as_slice(ps);
                        if i < src.len() {
                            interleaved[i * self.in_ch + ch] = src[i];
                        }
                    }
                }
            }
            self.input_ring.write(&interleaved[..cap]);
        }

        // Process in chunks of block_size — graph processes BUF_SIZE frames per call
        let mut offset = 0usize;
        while offset < nframes {
            let n = (nframes - offset).min(chunk);
            let pos = self.sample_pos.fetch_add(n as u64, Ordering::Relaxed);
            let tick = ClockTick::new(
                pos,
                n as u32,
                self.sample_rate,
                "jack".into(),
                self.view.clone(),
            );
            unsafe {
                self.process_cb.call(&tick);
            }
            offset += n;
        }

        // Playback: read output ring → output ports (deinterleaved)
        if !self.out_ports.is_empty() && self.out_ch > 0 {
            let total_samps = nframes * self.out_ch;
            let cap = total_samps.min(MAX_INTERLEAVED);
            let mut interleaved = [0.0f32; MAX_INTERLEAVED];
            let n = self.output_ring.read(&mut interleaved[..cap]);
            for ch in 0..self.out_ch {
                let buf = self.out_ports[ch].as_mut_slice(ps);
                for i in 0..nframes {
                    let idx = i * self.out_ch + ch;
                    if idx < n {
                        buf[i] = interleaved[idx];
                    } else {
                        buf[i] = 0.0;
                    }
                }
            }
        }

        Control::Continue
    }
}

// ============================================================================
// IoBackend impl
// ============================================================================

impl IoBackend for JackBackend {
    fn create_view(&self) -> Arc<dyn BufferView> {
        self.view.clone()
    }

    fn set_process_callback(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        unsafe {
            self.process_cb.set(cb);
        }
    }

    fn run(&self, _running: Arc<AtomicBool>) -> Result<(), String> {
        let client_name = self.config.output_device.as_deref().unwrap_or("rill");

        let (client, _status) = Client::new(client_name, ClientOptions::NO_START_SERVER)
            .map_err(|e| format!("JACK client new: {e:?}"))?;

        let out_ch = self.config.output_channels.min(2) as usize;
        let out_ports: Vec<Port<AudioOut>> = if out_ch > 0 {
            let mut ports = Vec::with_capacity(out_ch);
            for i in 0..out_ch {
                let name = if out_ch == 1 {
                    "output".into()
                } else {
                    format!("output_{}", i + 1)
                };
                ports.push(
                    client
                        .register_port(&name, AudioOut)
                        .map_err(|e| format!("JACK output port {name}: {e:?}"))?,
                );
            }
            ports
        } else {
            Vec::new()
        };

        let in_ch = self.config.input_channels.min(2) as usize;
        let in_ports: Vec<Port<AudioIn>> = if in_ch > 0 {
            let mut ports = Vec::with_capacity(in_ch);
            for i in 0..in_ch {
                let name = if in_ch == 1 {
                    "input".into()
                } else {
                    format!("input_{}", i + 1)
                };
                ports.push(
                    client
                        .register_port(&name, AudioIn)
                        .map_err(|e| format!("JACK input port {name}: {e:?}"))?,
                );
            }
            ports
        } else {
            Vec::new()
        };

        // Collect port names for auto-connect
        let out_port_names: Vec<_> = out_ports.iter().filter_map(|p| p.name().ok()).collect();
        let in_port_names: Vec<_> = in_ports.iter().filter_map(|p| p.name().ok()).collect();

        let sample_rate = client.sample_rate() as f32;
        let block_size = self.config.buffer_size as usize;
        let handler = JackProcessHandler {
            process_cb: self.process_cb,
            out_ports,
            in_ports,
            in_ch,
            out_ch,
            input_ring: self.input_ring.clone(),
            output_ring: self.output_ring.clone(),
            view: self.view.clone(),
            sample_pos: self.sample_pos.clone(),
            sample_rate,
            block_size,
            sys_clock: self.sys_clock.clone(),
        };

        let active_client = client
            .activate_async((), handler)
            .map_err(|e| format!("JACK activate: {e:?}"))?;

        let jack_client = active_client.as_client();
        for (i, name) in out_port_names.iter().enumerate() {
            let target = format!("system:playback_{}", i + 1);
            if let Err(e) = jack_client.connect_ports_by_name(name, &target) {
                log::info!("JACK connect {name} → {target}: {e}");
            }
        }
        for (i, name) in in_port_names.iter().enumerate() {
            let src = format!("system:capture_{}", i + 1);
            if let Err(e) = jack_client.connect_ports_by_name(&src, name) {
                log::info!("JACK connect {src} → {name}: {e}");
            }
        }

        self.running.store(true, Ordering::Release);
        unsafe {
            *self.active_client.get() = Some(active_client);
        }

        // Block until stop() is called
        while self.running.load(Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        Ok(())
    }

    fn stop(&self) -> Result<(), String> {
        self.running.store(false, Ordering::Release);
        unsafe {
            if let Some(client) = (*self.active_client.get()).take() {
                drop(client);
            }
        }
        Ok(())
    }
}

impl Drop for JackBackend {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        unsafe {
            if let Some(client) = (*self.active_client.get()).take() {
                drop(client);
            }
            self.process_cb.drop_box();
        }
    }
}
