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
│  set_default_backend(name, params)                       │
│  set_clock_tx(tx: ActorRef<ClockTick>)                   │
│  build() → Graph                                         │
└──────────────────────┬───────────────────────────────────┘
                       │ consume
                       ▼
┌──────────────────────────────────────────────────────────┐
│                       Graph                              │
│  ┌────────┐   ┌────────────┐   ┌────────┐               │
│  │ Source │──►│ Processor  │──►│  Sink  │  ...          │
│  └────────┘   └────────────┘   └────────┘               │
│                                                          │
│  nodes: Vec<NodeVariant>        topo_order: Vec<usize>   │
│  active_node_idx: Option<usize>                          │
│  cmd_queue: Arc<MpscQueue<SetParameter>>   (control→audio)│
│  clock_tx: ActorRef<ClockTick>              (audio→control)│
└──────────────────────┬───────────────────────────────────┘
                       │ Graph::run()
                       ▼
┌──────────────────────────────────────────────────────────┐
│                Tick closure (per block)                   │
│  1. Drain cmd_queue → set_parameter on target nodes      │
│  2. process_block() on source node                       │
│  3. Port::propagate() — recursive DAG traversal          │
│     • Copy data to downstream input ports                │
│     • process_block() on downstream nodes                │
│     • Recurse through output ports                       │
│  4. clock_tx.send(ClockTick) → control thread            │
└──────────────────────────────────────────────────────────┘
```

## Processing model

The processing callback (called once per audio block):

1. Drain `MpscQueue<SetParameter>` (control→audio commands)
2. Call `Source::generate()` — fills output buffers
3. Call `Port::propagate()` — recursive DAG traversal:
   - Copy data to downstream input ports (zero-copy for 1:1 via `upstream_buffer`)
   - Call the downstream node's `process_block` (`generate`/`process`/`consume`)
   - Recurse through output ports' `downstream_input_ptrs`
4. Send `ClockTick` to control thread via `clock_tx`

## Backend ownership

Each I/O node owns its backend via `Box<dyn IoBackend<T>>`.
Backends are created in `GraphBuilder::build()` and passed only to nodes
implementing `IoNode` (detected via `as_io_node_mut()`).
Per-node backends are specified through `NodeDef.backend: Option<String>`
in serialized graphs or via `GraphBuilder::set_node_backend(idx, name)`.

The active (driver) node owns the audio I/O backend (e.g. PortAudio)
and implements `ActiveNode::run()` to set up the process callback and block
on the audio thread.  The active node is detected via `as_active_node_mut()`.

```
build():
  for each node with a backend name:
    backend = BackendFactory::create(name, params)
    if let Some(io_node) = node.as_io_node_mut() {
        io_node.resolve_backend(backend)
    }

  find active node via as_active_node_mut() → store active_node_idx

run():
  let tick: Box<dyn FnMut(u64, f32)> = Box::new(move |sample_pos, sample_rate| {
      // drain command queue
      while let Some(cmd) = cmd_queue.pop() {
          nodes[cmd.node].set_parameter(&cmd.parameter, cmd.value);
      }
      // process source node
      nodes[source_idx].process_block(&mut ctx);
      // propagate through DAG
      port.propagate(...);
      // send clock tick to control-side actors
      clock_tx.send(ClockTick::new(sample_pos, BUF_SIZE as u32, sample_rate));
  });
  nodes[active_idx].as_active_node_mut().unwrap().run(tick, running)
```

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
            backend: None,  // uses default backend from RuntimeConfig
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

The command queue is drained from the process callback in `Graph::run()`.

## Key components

| Component | Purpose |
|-----------|---------|
| `GraphBuilder` | Mutable builder: nodes, connections, backends, clock channel |
| `Graph` | Immutable DAG container, owned by audio thread |
| `GraphDef` | Serializable graph topology (nodes + connections + backends) |
| `NodeDef` | Node in a serialized graph: type, params, optional backend name |
| `Port` | Owns buffer, downstream routes, and feedback state |

## Integration

- `rill-core` — `Node`, `Source`/`Processor`/`Sink` traits, `ClockTick`
- `rill-core-actor` — `ActorRef<SetParameter>`, `ActorCell` (mailbox infrastructure)
- `rill-io` — `Input`/`Output` nodes implementing `IoNode` + `ActiveNode`
- `rill-patchbay` — automation via parameter commands through the actor mailbox
