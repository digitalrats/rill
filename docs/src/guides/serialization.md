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
- `parameters` are read from each node via [`AudioNode::get_parameter`],
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

## Export (AudioGraph → JSON/CBOR)

```rust
use rill_graph::serialization::{to_json, to_cbor};

let graph: AudioGraph<f32, 64> = /* … */;

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
use rill_core::traits::{AudioNode, NodeId, NodeParams, NodeVariant};

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
builder.connect_audio(0, 0, 1, 0);
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
