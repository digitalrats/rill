# Graph Serialization

Rill graphs can be serialised to JSON (human-readable) or CBOR (compact binary)
and restored via the `NodeRegistry`. This enables preset storage, network transfer,
and offline editing of graph topologies.

## Feature gate

```toml
# Cargo.toml
rill-graph = { features = ["serialization"] }
```

Enables `rill_graph::serialization` module, which depends on:
- `serde` / `serde_json` / `serde_cbor`

## Data model

### `GraphDocument`

The top-level container:

```rust
pub struct GraphDocument {
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
    pub parameters: HashMap<String, ParamValue>,
}
```

- `type_name` comes from [`NodeMetadata::type_name`](crate::NodeMetadata).
  When `type_name` is `None`, the `name` field is used as fallback.
- `parameters` are read from each node via [`Node::get_parameter`],
  iterating over [`NodeMetadata::parameters`].

### `ConnectionDef`

```rust
pub struct ConnectionDef {
    pub kind: SignalKind,
    pub from_node: usize,     // index in nodes array
    pub from_port: usize,
    pub to_node: usize,
    pub to_port: usize,
}

pub enum SignalKind {
    Audio,
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

Under the hood [`GraphDocument::from_graph`] iterates every node, reads
`NodeMetadata` + `get_parameter`, and walks output port routing tables.

## Import (JSON/CBOR → GraphBuilder)

```rust
use rill_graph::serialization::from_json;
use rill_graph::registry::NodeRegistry;

let registry = NodeRegistry::<f32, 64>::new();
registry.register(MySineCtor);
// …

let json = std::fs::read_to_string("preset.json")?;
let mut builder: GraphBuilder<f32, 64> = from_json(&json, &registry)?;
let graph = builder.build(clock_source)?;
```

### Validation

On import [`GraphDocument::into_builder`] performs:

1. **Duplicate NodeId check** — every `NodeDef.id` must be unique.
2. **Block size match** — the document's `block_size` must equal `B`.
3. **Type resolution** — every `type_name` must be registered in the `NodeRegistry`.

## Type names and the registry

Each node type that participates in serialisation must register a constructor:

```rust
use rill_graph::registry::{NodeConstructor, NodeRegistry};
use rill_core::traits::{Node, NodeId, NodeParams, NodeVariant};

struct SineCtor;
impl<T: Transcendental, const B: usize> NodeConstructor<T, B> for SineCtor {
    fn type_name(&self) -> &'static str { "rill/sine_osc" }
    fn construct(&self, id: NodeId, params: &NodeParams) -> NodeVariant<T, B> {
        let freq = params.get_f32("frequency", 440.0);
        let mut osc = SineOsc::<T, B>::new().with_frequency(freq);
        osc.set_id(id);
        osc.init(params.sample_rate);
        NodeVariant::Source(Box::new(osc))
    }
}

let mut registry = NodeRegistry::<f32, 64>::new();
registry.register(SineCtor);
```

The `type_name` returned by the constructor should match the `type_name`
exposed in [`NodeMetadata`] so that export and import are consistent:

- `NodeConstructor::type_name()` → used as the factory lookup key on import.
- `NodeMetadata::type_name` (with `name` fallback) → written into the document
  on export.

To set an explicit type name in metadata:

```rust
fn metadata(&self) -> NodeMetadata {
    NodeMetadata {
        type_name: Some("rill/sine_osc".into()),
        ..NodeMetadata::new("Sine", NodeCategory::Source)
    }
}
```

### Convenience: closure registration

```rust
registry.register_fn("rill/sine_osc", |id, params| {
    let mut osc = SineOsc::new().with_frequency(params.get_f32("freq", 440.0));
    osc.set_id(id);
    osc.init(params.sample_rate);
    NodeVariant::Source(Box::new(osc))
});
```

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

Both encode the identical [`GraphDocument`] structure — switching formats
is a single function call.

## Example round-trip

```rust
use rill_graph::prelude::*;

// Build
let mut builder = GraphBuilder::<f32, 64>::new();
builder.add_node(&registry, "rill/sine", &NodeParams::new(44100.0))?;
builder.add_node(&registry, "rill/delay", &NodeParams::new(44100.0))?;
builder.connect_signal(0, 0, 1, 0);
let graph = builder.build(clock)?;

// Export
let json = rill_graph::serialization::to_json(&graph)?;

// Import
let mut restored = rill_graph::serialization::from_json(&json, &registry)?;
let graph2 = restored.build(clock)?;
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

The umbrella crate `rill-adrift` provides a **centralised registry** that
pre-registers every built-in node type from all rill crates:

```rust
use rill_adrift::registration::{register_all, registry};

// Option A: create and populate your own registry
let mut my_reg = rill_graph::NodeRegistry::<f32, 64>::new();
register_all(&mut my_reg);
my_reg.register_fn("app/custom_node", |id, params| { /* … */ });

// Option B: use the lazily-initialized global singleton
let reg = registry::<64>();
// Equivalent to calling register_all once, then reusing forever.
```

### Global `registry::<B>()`

`rill_adrift::registration::registry::<B>()` returns a `&'static NodeRegistry`
initialised **once** on first call. Supported block sizes: 64, 128, 256, 512.

```rust
use rill_adrift::registration;

let reg_64  = registration::registry::<64>();
let reg_256 = registration::registry::<256>();  // separate singleton per size
```

This is especially useful for quick prototyping and applications that only
use built-in node types. Drift, for example, relies entirely on the global
registry:

```rust
// drift/src/server/mod.rs
let registry = rill_adrift::registration::registry::<BUF_SIZE>();
match doc.into_builder::<f32, BUF_SIZE>(registry) { … }
```

### Convenience deserialisation helpers

When using the global registry you can skip the registry parameter entirely:

```rust
#[cfg(feature = "serialization")]
use rill_adrift::registration::{load_graph_json, load_graph_document};

// Load from JSON string → GraphBuilder
let builder = load_graph_json::<256>(r#"{"nodes":[…], "connections":[…]}"#)?;

// Load from deserialized GraphDocument
let doc: GraphDocument = serde_json::from_str(&json)?;
let builder = load_graph_document::<256>(doc)?;
```

Both functions use `registry::<B>()` internally.

---

## Custom nodes at the application level

Applications can define their own graph nodes and register them alongside
the built-in rill types. There are three levels of integration.

### Level 1: Register a closure (no custom type)

For simple one-off processing, use `register_fn`:

```rust
registry.register_fn("app/gain", |id: NodeId, params: &NodeParams| {
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

### Level 2: Implement `NodeConstructor` trait

For reusable constructors with parameter validation:

```rust
use rill_graph::registry::{NodeConstructor, NodeRegistry};
use rill_core::traits::{Node, NodeId, NodeParams, NodeVariant};

struct TremoloCtor;

impl NodeConstructor<f32, 64> for TremoloCtor {
    fn type_name(&self) -> &'static str { "app/tremolo" }

    fn construct(&self, id: NodeId, params: &NodeParams) -> NodeVariant<f32, 64> {
        let rate = params.get_f32("rate", 5.0).clamp(0.1, 50.0);
        let depth = params.get_f32("depth", 0.5).clamp(0.0, 1.0);
        let mut n = Tremolo::<f32, 64>::new(rate, depth);
        n.set_id(id);
        n.init(params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    }
}

registry.register(TremoloCtor);
```

### Level 3: Full custom graph node

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
        inputs.push(Port::audio_input(PortId::new(0, PortType::Signal)));

        let mut outputs = Vec::with_capacity(1);
        outputs.push(Port::audio_output(PortId::new(0, PortType::Signal)));

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
        let buf = out.audio_buffer_mut().as_mut_array();

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

### Wiring custom nodes into the registry

```rust
use rill_graph::NodeRegistry;

let mut registry = NodeRegistry::<f32, 64>::new();

// Built-in rill nodes
rill_adrift::registration::register_all(&mut registry);

// Custom app nodes
registry.register_fn("app/gain", |id, params| {
    /* … */
});
registry.register(TremoloCtor);

// Serialize/deserialize with the combined registry
let json = r#"{"nodes":[{"id":0,"type_name":"app/tremolo","name":"MyTrem","parameters":{"rate":4.0}}]}"#;
let mut builder = rill_graph::serialization::from_json(&json, &registry)?;
```

### Referencing custom nodes from GraphDocument

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

### Adding custom nodes to the global registry

To make custom nodes available in the `registry::<B>()` singleton without
passing a custom registry around, call `register_all` and then extend the
global registry before first use:

```rust
// Force initialisation, then extend
let reg = rill_adrift::registration::registry::<64>();
// registry is immutable after first init — this only works if called
// BEFORE any other code accesses registry::<64>().
//
// For complete control, build your own NodeRegistry instead.
```

For production applications the recommended pattern is to build a dedicated
registry at startup rather than mutating the global singleton:

```rust
fn build_app_registry() -> NodeRegistry<f32, 64> {
    let mut reg = NodeRegistry::new();
    rill_adrift::registration::register_all(&mut reg);
    reg.register_fn("app/tremolo", |id, params| { /* … */ });
    reg
}

fn main() {
    let registry = build_app_registry();
    // pass &registry to serialization, graph building, etc.
}
```
