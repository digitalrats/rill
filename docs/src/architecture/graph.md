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
1. `actor.drain()` — applies queued `SetParameter` commands
2. Creates `RenderContext` from the tick
3. `source.process_block(&ctx, &tick)` — fills output ports
4. `Port::propagate()` — recursive DAG traversal

`send_clock_tick(&tick)` dispatches a single `ClockTick` per I/O cycle
to the rack actor (gated by `tick.is_final`).

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

`Graph` implements `ActorCell<Msg = SetParameter>` so that control-side
code can send parameter changes through `ActorRef<SetParameter>`:

```rust
impl ActorCell for Graph {
    type Msg = SetParameter;
    fn receive(&mut self, msg: SetParameter) {
        let idx = msg.port.node_id().inner() as usize;
        self.nodes[idx].set_parameter(&msg.parameter, msg.value);
    }
}
```

The command queue is drained from the process callback via
`ProcessingState::process_block()`.

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
- `rill-core-actor` — `ActorRef<SetParameter>`, `ActorCell` (mailbox infrastructure)
- `rill-io` — `Input`/`Output` nodes, `IoBackend` trait
- `rill-patchbay` — automation via parameter commands through the actor mailbox
