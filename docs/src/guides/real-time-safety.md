# Real-Time Safety

The signal graph runs wherever the `IoBackend` process callback fires. All
backends are callback-driven — they invoke the rill process callback(s) — but
differ in which thread runs them.

## Two backend models

| Model | Backends | RT guarantee |
|---|---|---|
| **Hardware callback** | PipeWire, JACK, PortAudio | Hard RT — the audio system calls the process callback on its own real-time thread. No syscalls, no allocation, no locks. |
| **Own audio thread** | ALSA | Soft RT — the backend runs its own audio thread that waits on the device FDs with `snd_pcm_wait` (event-driven, **never** `thread::sleep()`) and fires the same process callbacks per period. |

## Rules for the RT path (applies to both models)

Any code reached from the process callback — `generate()`, `process()`,
`consume()`, `propagate()`, and everything they call — **must** obey:

| Rule | Rationale |
|------|-----------|
| **No heap allocation in RT path** | `Vec::new()`, `Box::new()`, `format!()` inside `propagate`/`generate`/`process`/`consume` will cause xruns. All buffers must be stack-allocated or pre-allocated at graph construction. |
| **No locks in RT path** | `Mutex::lock()`, `RwLock::write()` (even parking_lot) may spin. Communication with the control thread uses only `rill_core::queues::MpscQueue` (lock-free SPSC). |
| **No `thread::sleep()` in RT path** | `thread::sleep()` is a syscall — it blocks the calling thread, introduces timing jitter, and makes deterministic scheduling impossible. Backends that run their own audio thread (ALSA) must wait on the device FDs (`snd_pcm_wait` / `poll`), never on `sleep`. |
| **No file I/O, no socket I/O in RT path** | Any syscall (open, read, write, send, recv) can block unpredictably. |
| **`downstream_nodes` is pre-filled** | `Port::downstream_nodes` is populated once by `GraphBuilder::build()` and iterated at runtime without deduplication or allocation. |
| **Fixed-size stack buffers** | Backend callbacks must use `[f32; MAX_BLOCK_SAMPLES]` (512) instead of `vec![]`. |

## Allowed exceptions

- `MpscQueue::pop()` — lock-free atomic, OK on RT.
- `AtomicU32::fetch_add()` / `AtomicBool::store()` — OK on RT.
- Raw pointer dereference (`*mut`, `*const`) — single-threaded DAG, guaranteed valid.
- `IoRingBuffer::read()` / `write()` — lock-free atomic SPSC, OK on RT (used inside backends only, not in graph nodes).

## Known issues

1. **Backends with their own audio thread** (ALSA) must not use
   `thread::sleep()` — wait on the device FDs (`snd_pcm_wait` / `poll`) instead.
   All current backends (PortAudio, ALSA, PipeWire, JACK) respect this rule.
2. **Testing RT code** — any new RT path code must be verified with
   `cargo test --release` under `pw-loopback` or similar virtual device
   to detect xruns.

## RT-safety unit tests

`rill-fft` includes automated RT-safety integration tests (`tests/rt_safety.rs`)
using a custom `#[global_allocator]` that panics on any heap allocation or
deallocation during `process()` calls. The allocator guard uses `thread_local!`
to isolate test threads, allowing tests to run in parallel. Run with:

```bash
cargo test -p rill-fft --test rt_safety
```

All FFT, convolution, and spectral effect `process()` paths are verified
zero-allocation.
