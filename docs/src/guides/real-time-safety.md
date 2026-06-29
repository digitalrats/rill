# Real-Time Safety

The signal graph runs wherever the `IoBackend` process callback fires. The
constraints depend on the backend model.

## Two backend models

| Model | Backends | RT guarantee |
|---|---|---|
| **Callback-driven** | PipeWire, JACK, PortAudio | Hard RT — callback fires on the audio device's real-time thread. No syscalls, no allocation, no locks. |
| **Poll-driven** | ALSA | Soft RT — the backend's own thread loops polling the audio device. Must not use `thread::sleep()` to pace iterations. Use `poll()` / `epoll()` on audio FDs instead. |

## Rules for the RT path (applies to both models)

Any code reached from the process callback — `generate()`, `process()`,
`consume()`, `propagate()`, and everything they call — **must** obey:

| Rule | Rationale |
|------|-----------|
| **No heap allocation in RT path** | `Vec::new()`, `Box::new()`, `format!()` inside `propagate`/`generate`/`process`/`consume` will cause xruns. All buffers must be stack-allocated or pre-allocated at graph construction. |
| **No locks in RT path** | `Mutex::lock()`, `RwLock::write()` (even parking_lot) may spin. Communication with the control thread uses only `rill_core::queues::MpscQueue` (lock-free SPSC). |
| **No `thread::sleep()` in RT path** | `thread::sleep()` is a syscall — it blocks the calling thread, introduces timing jitter, and makes deterministic scheduling impossible. Even in poll-driven backends (ALSA, CPAL) the processing loop must wait on audio FDs (`poll`/`epoll`), not on `sleep`. |
| **No file I/O, no socket I/O in RT path** | Any syscall (open, read, write, send, recv) can block unpredictably. |
| **`downstream_nodes` is pre-filled** | `Port::downstream_nodes` is populated once by `GraphBuilder::build()` and iterated at runtime without deduplication or allocation. |
| **Fixed-size stack buffers** | Backend callbacks must use `[f32; MAX_BLOCK_SAMPLES]` (512) instead of `vec![]`. |

## Allowed exceptions

- `MpscQueue::pop()` — lock-free atomic, OK on RT.
- `AtomicU32::fetch_add()` / `AtomicBool::store()` — OK on RT.
- Raw pointer dereference (`*mut`, `*const`) — single-threaded DAG, guaranteed valid.
- `IoRingBuffer::read()` / `write()` — lock-free atomic SPSC, OK on RT (used inside backends only, not in graph nodes).

## Known issues

1. **Poll-driven backends** must not use `thread::sleep()` in the poll loop.
   Use `poll()`/`epoll()` on audio FDs instead. All current backends
   (PortAudio, ALSA, PipeWire, JACK) respect this rule.
2. **Testing RT code** — any new RT path code must be verified with
   `cargo test --release` under `pw-loopback` or similar virtual device
   to detect xruns.
