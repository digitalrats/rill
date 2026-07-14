# Two-Thread Architecture

Rill separates processing into two independent threads communicating through lock-free SPSC queues:

## Signal Thread (hard or soft RT)

Runs the process callback — `generate()` → `process()` → `consume()` → `Port::propagate()`.
No heap allocs, no locks, no syscalls. The graph is a single-threaded static DAG — nodes are
never added or removed after construction, and topology never changes.

Inside the I/O callback tick:
1. `actor.drain()` — applies queued `CommandEnum::SetParameter` commands from the actor mailbox
2. Builds a `RenderContext` with sample clock, transport state, and hardware clock correction
3. `Source::generate()` / `Processor::process()` / `Sink::consume()` via `process_block(&ctx)`
4. `Port::propagate()` — recursive DAG traversal through direct port pointers
5. Sends `CommandEnum::ClockTick` to the parent Patchbay actor

All `rill-core::buffer` types (`DelayLine`, `TapeLoop`, `PipeBuffer`, `RingBuffer`, `FanOutBuffer`, `FanInBuffer`)
are used exclusively inside this path. No atomics, no locks — the graph is a single-threaded static DAG.

## Control Thread (tokio green threads)

Runs `Patchbay` with automatons (LFO, envelopes, sequencers). Communicates with the signal
thread via the graph actor mailbox — messages are `CommandEnum` variants, sent via
`ActorRef<CommandEnum>` and drained inline inside the callback tick. No separate queue types are needed.

**Servo** — the primary automaton-to-parameter bridge:
1. Receives `CommandEnum::ClockTick` from the graph
2. Advances time and calls `automaton.step()`
3. Applies `ControlStrategy` and `ConflictStrategy`
4. Sends `CommandEnum::SetParameter` to the graph's `ActorRef<CommandEnum>`
5. The `SetParameter` lands in the graph's actor mailbox; next I/O callback tick, `actor.drain()` applies it

### Communication channels

```
I/O callback tick:                     Actor mailbox (CommandEnum):
  actor.drain()  ◄──────────  SetParameter (servo → graph)
  generate() / process() / consume()
  port.propagate()                    Control path:
  ── ClockTick ──→ Servo ──→ automaton.step()
                             ── SetParameter ──→ graph_ref (next tick drain)
```

## Rule of thumb

If data crosses threads, send `CommandEnum` variants through `ActorRef<CommandEnum>`.
Everything else is single-threaded within the signal graph running inside the I/O callback.
