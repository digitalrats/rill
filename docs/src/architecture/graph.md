# Signal graph (rill-graph)

Static DAG signal graph — topology and port connections only.
Processing is driven by `Port::propagate`.

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│                      GraphBuilder                        │
│  add_source() → idx  add_processor() → idx              │
│  add_sink() → idx    connect_signal(from, to)           │
│  connect_feedback(from, to)                              │
└──────────────────────────────────────────────────────────┘
```

### Backend ownership

Backends are created **externally** by the orchestrator, not inside
graph nodes.  The `GraphBuilder` no longer holds a `BackendFactory`.

1. The orchestrator creates a backend via `BackendFactory::create_output()` /
   `create_input()` / `create_duplex()`, obtaining `IoDriver`, `IoCapture`,
   `IoPlayback` capability objects
2. `ProcessingState` is extracted from the `Graph` via `into_processing_state()`
3. `state.wire_backends(capture, playback)` injects backends into Source / Sink nodes
4. The orchestrator runs the graph through the driver:

```rust,ignore
let OutputBundle { driver, playback } = bf.create_output(name, &params)?;
let mut state = graph.into_processing_state();
state.wire_backends(None, Some(playback));
state.run_with_driver(driver, running)?;
```

`run_with_driver()` internally calls `set_process_callback` + `run` + parks until stopped.

### Processing flow

`ProcessingState::process_block(&tick)`:
1. Adopt `tick.sample_rate` — re-`init` all nodes if it differs from the rate
   they were built with (the graph has no clock of its own; it runs at whatever
   rate the backend's `ClockTick` carries — e.g. JACK at 48 kHz)
2. `actor.drain()` — queues/applies `SetParameter` commands
3. Apply sample-accurate parameter changes due for this 256-sample block
   (writes carrying `sample_pos`; writes without one are applied immediately)
4. Creates `RenderContext` from the tick
5. `source.process_block(&ctx, &tick)` — fills output ports
6. `Port::propagate()` — recursive DAG traversal

`send_clock_tick(&tick)` forwards the `ClockTick` to the rack actor (gated by
`tick.is_final`). Chunking backends (PipeWire, JACK) leave `is_final = true` on
every `block_size` chunk, so control modules receive **one tick per block**;
sample-accurate placement of their resulting parameter writes is handled by
`SetParameter.sample_pos` + `ClockTick.io_quantum`, not by coalescing ticks.

## Serialized graphs (GraphDef)

```rust
let def = GraphDef {
    nodes: vec![
        NodeDef {
            id: 0,
            type_name: "rill/lofi_input",
            backend: Some("ay38910".into()),
            parameters: [("bit_depth", ParamValue::Int(8))].into(),
        },
        NodeDef {
            id: 1,
            type_name: "rill/output",
            backend: None,  // nodes can optionally reference backends by name
            parameters: [("channels", ParamValue::Float(1.0))].into(),
        },
    ],
    connections: vec![
        ConnectionDef { kind: Signal, from_node: 0, from_port: 0, to_node: 1, to_port: 0 },
    ],
};
def.populate(&mut builder)?;
let graph = builder.build()?;
```

## Actor interface

The graph spawns an inline actor (`Actor<CommandEnum>`) whose handler applies
parameter changes to nodes. Control-side code sends `CommandEnum::SetParameter`
through `ActorRef<CommandEnum>` obtained from `graph.handle()`:

```rust
// Simplified graph actor handler (rill-graph/src/graph.rs)
system.spawn("graph", move |msg: CommandEnum| {
    if let CommandEnum::SetParameter(param) = msg {
        if param.sample_pos.is_some() {
            pending.borrow_mut().push(param);      // sample-accurate: defer
        } else {
            let idx = param.port.node_id().inner() as usize;
            nodes[idx].set_parameter(&param.parameter, param.value); // ASAP
        }
    }
});
```

The mailbox is drained from the process callback via
`ProcessingState::process_block()`, which then applies any deferred
sample-accurate writes due for the current block.

## Key components

| Component | Purpose |
|-----------|---------|
| `GraphBuilder` | Mutable builder: nodes and connections |
| `Graph` | Immutable DAG container; `into_processing_state()` extracts runtime state |
| `ProcessingState` | Runtime processor: `process_block()`, `send_clock_tick()` |
| `GraphDef` | Serializable graph topology (nodes + connections) |
| `NodeDef` | Node in a serialized graph: type, params, optional backend name |
| `Port` | Owns buffer, downstream routes, and feedback state |

## Integration

- `rill-core` — `Node`, `Source`/`Processor`/`Sink` traits, `ClockTick`
- `rill-core-actor` — `Actor<CommandEnum>` / `ActorRef<CommandEnum>` (mailbox infrastructure)
- `rill-io` — `Input`/`Output` nodes, `IoBackend` trait
- `rill-patchbay` — automation via parameter commands through the actor mailbox
- `rill-fft` — `ConvolverNode` graph node (IR convolution)
- `rill-lang` — `rill/lang` factory node (DSL‑compiled signal processors)
