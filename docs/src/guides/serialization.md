# Graph Serialization

Rill graphs can be serialised to JSON (human-readable) or CBOR (compact binary)
and restored via a [`NodeFactory`]. This enables preset storage, network transfer,
and offline editing of graph topologies.

## Feature gate

```toml
# Cargo.toml
rill-graph = { features = ["serialization"] }
```

Enables `rill_graph::serialization` module, which depends on:
- `serde` / `serde_json` / `serde_cbor`

## Data model

### `GraphDef`

The top-level container:

```rust
pub struct GraphDef {
    pub format_version: String,       // "rill/1"
    pub sample_rate: f32,
    pub block_size: usize,
    pub nodes: Vec<NodeDef>,
    pub connections: Vec<ConnectionDef>,
}
```

### `NodeDef`

```rust
pub struct NodeDef {
    pub id: u32,                          // NodeId (important for patchbay bindings)
    pub type_name: String,                // factory lookup key, e.g. "rill/sine_osc"
    pub name: String,                     // human-readable instance name
    pub backend: Option<String>,          // optional backend name (e.g. "ay38910")
    pub parameters: HashMap<String, ParamValue>,
}
```

- `backend` — specifies a named backend from `BackendFactory` for this
  I/O node.  The orchestrator uses `BackendFactory` to create backends externally;
  the builder no longer holds a factory.  Processor nodes leave this empty.

### `ParamValue`

```rust
pub enum ParamValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    String(String),
    Choice(String),
    Bytes(Vec<u8>),  // raw data for IoControl::write_data()
}
```

`Bytes` carries raw byte arrays for backend-specific control protocols
(e.g. AY-3-8910 register writes, MIDI SySex).  The `"io_write"` parameter
on `LofiInput` forwards `Bytes` payloads to `IoControl::write_data()`.

### `ConnectionDef`

```rust
pub struct ConnectionDef {
    pub kind: SignalKind,
    pub from_node: u32,       // NodeId
    pub from_port: usize,
    pub to_node: u32,         // NodeId
    pub to_port: usize,
}

pub enum SignalKind {
    Signal,
    Control,
    Clock,
    Feedback,
}
```

Connections are reconstructed from the port routing state of each node
(`Port::downstream` for audio, `Port::feedback_downstream` for feedback).

Control and clock connections currently store only the metadata (they are
round-tripped via the document), but the port-level routing for these signal
kinds is not yet tracked by the engine — this is reserved for future use.

## Export (Graph → JSON/CBOR)

```rust
use rill_graph::serialization::{to_json, to_cbor};

let graph: Graph<f32, 64> = /* … */;

let json = to_json(&graph)?;             // pretty-printed JSON
let cbor = to_cbor(&graph)?;             // compact binary (Vec<u8>)
```

Under the hood [`GraphDef::from_graph`] iterates every node, reads
`NodeMetadata` + `get_parameter`, and walks output port routing tables.

## Import (JSON/CBOR → GraphBuilder)

```rust
use rill_graph::serialization::from_json;

let json = std::fs::read_to_string("preset.json")?;

let mut builder: GraphBuilder<f32, 64> = GraphBuilder::new();
let def = from_json(&json)?;
def.populate(&mut builder)?;
let graph = builder.build()?;
```

### Validation

On import [`GraphDef::populate`] performs:

1. **Duplicate NodeId check** — every `NodeDef.id` must be unique.
2. **Block size match** — the document's `block_size` must equal the builder's `B`.
3. **Type resolution** — every `type_name` must be registered in the builder's `NodeFactory`.

## Type names and the factory

Each node type that participates in serialisation must be registered in a
[`NodeFactory`] via the [`node_ctor!`] macro or [`register_fn`]:

```rust
use rill_core::traits::{Node, NodeId, Params, NodeVariant};
use rill_graph::{node_ctor, NodeFactory};

let mut factory = NodeFactory::<f32, 64>::new();
node_ctor!(factory, "rill/sine_osc", |id: NodeId, params: &Params| {
    let freq = params.get_f32("frequency", 440.0);
    let mut osc = SineOsc::<f32, 64>::new().with_frequency(freq);
    osc.set_id(id);
    osc.init(params.sample_rate);
    NodeVariant::Source(Box::new(osc))
});
```

The `type_name` in the factory should match the `type_name`
exposed in [`NodeMetadata`] so that export and import are consistent:

- `node_ctor!` / `register_fn` key → used as the factory lookup key on import.
- `NodeMetadata::type_name` (with `name` fallback) → written into the document
  on export.

## Node IDs

`NodeId` is **preserved** through serialisation. This is critical when the
graph integrates with `rill-patchbay`: control bindings reference specific
`PortId`s, which in turn depend on exact `NodeId`s.

When importing, the document's `id` field is passed directly to
[`GraphBuilder::add_node_with_id`]. If two nodes share the same ID,
[`SerializationError::DuplicateNodeId`] is returned.

## Formats

| Format | When to use |
|---|---|
| **JSON** | Debugging, manual editing, version-controlled presets |
| **CBOR** | Network transfer, embedded preset storage, low-bandwidth links |

Both encode the identical [`GraphDef`] structure — switching formats
is a single function call.

## Example round-trip

```rust
use rill_graph::prelude::*;

// Build
let mut builder = GraphBuilder::<f32, 64>::new();
builder.add_node(&registry, "rill/sine", &Params::new(44100.0))?;
builder.add_node(&registry, "rill/delay", &Params::new(44100.0))?;
builder.connect_signal(0, 0, 1, 0);
let graph = builder.build()?;

// Export
let json = rill_graph::serialization::to_json(&graph)?;

// Import
let def = rill_graph::serialization::from_json(&json)?;
let mut restored_builder = GraphBuilder::new();
def.populate(&mut restored_builder)?;
let graph2 = restored_builder.build()?;
```

## Error types

| Error | Cause |
|---|---|
| `UnknownType(name)` | `type_name` not found in registry |
| `DuplicateNodeId(id)` | Two `NodeDef`s with the same `id` |
| `InvalidFormat(msg)` | Malformed JSON/CBOR or block size mismatch |

## Tests

Serialisation tests live in `rill-graph/src/serialization.rs` under
`mod tests`. Run them with the feature flag:

```bash
cargo test -p rill-graph --features serialization -- serialization
```

Coverage includes:

- JSON and CBOR round-trips
- Parameter preservation
- Feedback connection export
- Type name explicit vs fallback
- Node ID preservation
- Complex multi-node topologies
- Error handling (unknown type, duplicate ID, block size mismatch, malformed input)

---

## Automatic node registration (rill-adrift)

The umbrella crate `rill-adrift` provides [`register_all_nodes`] which
pre-registers every built-in node type from all rill crates:

```rust
use rill_adrift::registration::register_all_nodes;

let mut factory = rill_graph::NodeFactory::<f32, 256>::new();
register_all_nodes(&mut factory);
factory.register_fn("app/custom_node", |id, params| { /* … */ });
```

To build a factory shared across multiple graphs, wrap it in an `Arc`:

```rust
use std::sync::Arc;
let shared_factory = Arc::new(factory);
let mut builder = rill_graph::GraphBuilder::new(shared_factory);
```

[`register_all_nodes`]: https://docs.rs/rill-adrift/latest/rill_adrift/registration/fn.register_all_nodes.html

### Convenience deserialisation helper

`rill-adrift` re-exports [`load_graph_json`] for quick graph loading:

```rust
use rill_adrift::registration::load_graph_json;

let def = load_graph_json(r#"{"nodes":[…], "connections":[…]}"#)?;
// Then populate into a builder:
// def.populate(&mut builder)?;
```

[`load_graph_json`]: https://docs.rs/rill-adrift/latest/rill_adrift/registration/fn.load_graph_json.html

---

## Custom nodes at the application level

Applications can define their own graph nodes and register them alongside
the built-in rill types. There are three levels of integration.

### Level 1: Register a closure

For simple one-off processing, use `register_fn` on [`NodeFactory`]:

```rust
use rill_core::traits::{Node, NodeId, Params, NodeVariant};
use rill_graph::NodeFactory;

let mut factory = NodeFactory::<f32, 64>::new();
factory.register_fn("app/gain", |id: NodeId, params: &Params| {
    let mut n = GainNode::<f32, 64>::new(params.get_f32("gain", 1.0));
    n.set_id(id);
    n.init(params.sample_rate);
    NodeVariant::Processor(Box::new(n))
});
```

The type name `"app/gain"` can then be used in JSON documents:

```json
{"id": 0, "type_name": "app/gain", "name": "Volume", "parameters": {"gain": 0.8}}
```

[`NodeFactory`]: https://docs.rs/rill-graph/latest/rill_graph/factory/struct.NodeFactory.html
[`register_fn`]: https://docs.rs/rill-graph/latest/rill_graph/factory/struct.NodeFactory.html#method.register_fn

### Level 2: Full custom graph node

Implementing the `Node`, `Source`, `Processor`, or `Sink` trait:

```rust
use rill_core::math::Transcendental;
use rill_core::traits::{
    Node, Processor, NodeId, NodeMetadata, NodeCategory,
    NodeState, ParamValue, ParameterId, Port, PortId, PortDirection, PortType,
    ProcessResult,
};
use rill_core::time::ClockTick;

/// A simple sine-wave tremolo: multiplies the input by a low-frequency
/// oscillation. Demonstrates a complete custom graph node with metadata,
/// parameters, and ports.
pub struct Tremolo<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    state: NodeState<T, BUF_SIZE>,
    rate: f32,
    depth: f32,
    phase: f32,
    inputs: Vec<Port<T, BUF_SIZE>>,
    outputs: Vec<Port<T, BUF_SIZE>>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Tremolo<T, BUF_SIZE> {
    pub fn new(rate: f32, depth: f32) -> Self {
        let mut inputs = Vec::with_capacity(1);
        inputs.push(Port::input(NodeId(0), 0, "signal_in"));

        let mut outputs = Vec::with_capacity(1);
        outputs.push(Port::output(NodeId(0), 0, "signal_out"));

        Self {
            id: NodeId(0),
            state: NodeState::default(),
            rate, depth, phase: 0.0,
            inputs, outputs,
        }
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE>
    for Tremolo<T, BUF_SIZE>
{
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata::new("Tremolo", NodeCategory::Processor)
            .with_type_name("app/tremolo")
            .with_parameter("rate", 0.1, 50.0, 5.0)
            .with_parameter("depth", 0.0, 1.0, 0.5)
    }

    fn init(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;
    }

    fn reset(&mut self) { self.phase = 0.0; }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "rate" => Some(ParamValue::Float(self.rate)),
            "depth" => Some(ParamValue::Float(self.depth)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        match id.as_str() {
            "rate"  => self.rate  = value.as_f32().unwrap_or(self.rate),
            "depth" => self.depth = value.as_f32().unwrap_or(self.depth),
            _ => return Err(ProcessError::Parameter(ParameterError::NotFound)),
        }
        Ok(())
    }

    fn id(&self) -> NodeId { self.id }
    fn set_id(&mut self, id: NodeId) { self.id = id; }

    fn input_port(&self, idx: usize) -> Option<&Port<T, BUF_SIZE>> { self.inputs.get(idx) }
    fn input_port_mut(&mut self, idx: usize) -> Option<&mut Port<T, BUF_SIZE>> { self.inputs.get_mut(idx) }
    fn output_port(&self, idx: usize) -> Option<&Port<T, BUF_SIZE>> { self.outputs.get(idx) }
    fn output_port_mut(&mut self, idx: usize) -> Option<&mut Port<T, BUF_SIZE>> { self.outputs.get_mut(idx) }
    fn state(&self) -> &NodeState<T, BUF_SIZE> { &self.state }
    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> { &mut self.state }
    fn num_signal_inputs(&self) -> usize { 1 }
    fn num_signal_outputs(&self) -> usize { 1 }
}

impl<T: Transcendental, const BUF_SIZE: usize> Processor<T, BUF_SIZE>
    for Tremolo<T, BUF_SIZE>
{
    fn process(
        &mut self,
        clock: &ClockTick,
        inputs: &[&[T; BUF_SIZE]],
        _control: &[T],
        _clocks: &[ClockTick],
        _feedback: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        let dt = clock.delta_seconds();
        let out = self.output_port_mut(0).unwrap();
        let buf = out.buffer.as_mut_array();

        for i in 0..BUF_SIZE {
            self.phase += self.rate * dt / BUF_SIZE as f32;
            if self.phase > 1.0 { self.phase -= 1.0; }

            let lfo = (self.phase * std::f32::consts::TAU).sin();
            let gain = 1.0 - self.depth * 0.5 * (lfo + 1.0);

            buf[i] = inputs[0][i] * T::from_f32(gain);
        }
        Ok(())
    }
}
```

### Wiring custom nodes into the factory

```rust
use rill_graph::NodeFactory;

let mut factory = NodeFactory::<f32, 64>::new();

// Built-in rill nodes
rill_adrift::registration::register_all_nodes(&mut factory);

// Custom app nodes
factory.register_fn("app/gain", |id, params| {
    /* … */
});
factory.register_fn("app/tremolo", |id, params| {
    let rate = params.get_f32("rate", 5.0).clamp(0.1, 50.0);
    let depth = params.get_f32("depth", 0.5).clamp(0.0, 1.0);
    let mut n = Tremolo::<f32, 64>::new(rate, depth);
    n.set_id(id);
    n.init(params.sample_rate);
    NodeVariant::Processor(Box::new(n))
});

// Serialize/deserialize
use rill_graph::serialization::{to_json, from_json};
let mut builder = GraphBuilder::new(factory);
// … populate builder, build, export/import
```

### Referencing custom nodes from GraphDef

Once registered, custom type names work identically to built-in types in
all serialisation formats:

```json
{
  "format_version": "rill/1",
  "sample_rate": 48000.0,
  "block_size": 64,
  "nodes": [
    {
      "id": 0,
      "type_name": "app/tremolo",
      "name": "MyTremolo",
      "parameters": { "rate": 4.0, "depth": 0.7 }
    },
    {
      "id": 1,
      "type_name": "rill/sine",
      "name": "Carrier",
      "parameters": { "freq": 440.0, "amp": 0.5 }
    }
  ],
  "connections": [
    { "kind": "Signal", "from_node": 0, "from_port": 0, "to_node": 1, "to_port": 0 }
  ]
}
```

### Building a custom factory

The recommended pattern for production applications is to build a dedicated
factory at startup:

```rust
use rill_graph::NodeFactory;

fn build_app_factory() -> NodeFactory<f32, 64> {
    let mut factory = NodeFactory::new();
    rill_adrift::registration::register_all_nodes(&mut factory);
    factory.register_fn("app/tremolo", |id, params| { /* … */ });
    factory
}

fn main() {
    let factory = build_app_factory();
    let mut builder = GraphBuilder::new(factory);
    // … add nodes, build
}
```
