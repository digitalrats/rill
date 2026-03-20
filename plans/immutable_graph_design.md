# Immutable Graph Design using AudioNode Trait

## Overview

This document outlines the design for an immutable audio graph that replaces `DynProcessor` with `AudioNode` as the unified node trait. The graph is immutable after construction (topology cannot be changed), enabling performance optimizations such as precomputed topological order and buffer reuse.

## Design Goals

1. **Immutable topology** – Once built, nodes and connections cannot be added or removed.
2. **Unified node trait** – Use `AudioNode` as the base trait, extended with a `process_block` method that works for all node categories (Source, Processor, Sink).
3. **Performance** – Precompute topological order, routing tables, and reuse buffers to avoid allocations during real-time processing.
4. **Compatibility** – Existing node types (Source, Processor, Sink) must work with the new graph via blanket implementations.

## ActivePort-Based Connection Model

The existing `ActivePort` trait (defined in `kama-core/src/traits/port.rs`) provides a uniform interface for ports that can `pull` and `push` blocks of audio data. Each port is associated with a `PipeBuffer` that acts as a single‑producer, single‑consumer lock‑free ring buffer.

In the immutable graph, connections are represented as `PipeBuffer` instances that are shared between an output port and an input port. During graph construction, the builder creates a `PipeBuffer` for each connection and attaches it to both ports. Once built, the connections are fixed; no new buffers can be added or removed.

### Advantages of ActivePort Model

- **Decoupling**: Nodes do not need to know about the graph’s topology; they only interact with their own ports.
- **Real‑time safety**: `PipeBuffer` is lock‑free and suitable for real‑time audio threads.
- **Flexibility**: The same port abstraction works for audio, control, clock, and feedback signals.
- **Compatibility**: Existing nodes already have `Port` instances that implement `ActivePort`.

### Connection Lifecycle

1. **Construction**: `GraphBuilder::connect` creates a new `PipeBuffer` and calls `port.connect(buffer)` on both the source and destination ports.
2. **Validation**: The builder ensures that port types match and that each input port is connected at most once (output ports may fan out).
3. **Immutable snapshot**: After `build()`, the graph holds references to all buffers but does not allow modification of the connection set.

## Extended AudioNode Trait

Add a `process_block` method to `AudioNode` that processes a single block using the node’s ports. The default implementation will delegate to the appropriate subtrait (`Source`, `Processor`, `Sink`) based on the node’s category.

### ProcessContext (Optional)

While nodes can directly pull/push via their ports, a `ProcessContext` can be provided for convenience, especially for nodes that implement the `generate`/`process`/`consume` methods. The context aggregates all input and output buffers that have been pre‑fetched from the ports.

```rust
/// Convenience structure that gathers all input/output buffers for a node.
pub struct ProcessContext<'a, T: AudioNum, const BUF_SIZE: usize> {
    /// Current clock tick
    pub clock: &'a ClockTick,
    /// Audio input buffers (slice of references to [T; BUF_SIZE])
    pub audio_inputs: &'a [&'a [T; BUF_SIZE]],
    /// Control input values (slice of T)
    pub control_inputs: &'a [T],
    /// Clock input ticks
    pub clock_inputs: &'a [ClockTick],
    /// Feedback input buffers (slice of references to [T; BUF_SIZE])
    pub feedback_inputs: &'a [&'a [T; BUF_SIZE]],
    /// Audio output buffers (slice of mutable references to [T; BUF_SIZE])
    pub audio_outputs: &'a mut [&'a mut [T; BUF_SIZE]],
    /// Control output values (slice of mutable T)
    pub control_outputs: &'a mut [T],
    /// Clock output ticks
    pub clock_outputs: &'a mut [ClockTick],
    /// Feedback output buffers (slice of mutable references to [T; BUF_SIZE])
    pub feedback_outputs: &'a mut [&'a mut [T; BUF_SIZE]],
}
```

### AudioNode Extension

```rust
pub trait AudioNode<T: AudioNum, const BUF_SIZE: usize>: Send + Sync {
    // Existing methods...
    // ...

    /// Process a single block using the node's ports.
    /// The default implementation fetches buffers from ports, builds a ProcessContext,
    /// and calls the appropriate subtrait method.
    fn process_block(&mut self, clock: &ClockTick) -> ProcessResult<()> {
        match self.metadata().category {
            NodeCategory::Source => {
                let mut outputs = self.gather_output_buffers();
                self.as_source_mut().unwrap().generate(clock, &[], &[], &mut outputs)
            }
            NodeCategory::Processor => {
                let inputs = self.gather_input_buffers();
                let mut outputs = self.gather_output_buffers();
                let controls = self.gather_control_inputs();
                let clocks = self.gather_clock_inputs();
                let feedbacks = self.gather_feedback_inputs();
                let mut control_outs = self.gather_control_outputs();
                let mut clock_outs = self.gather_clock_outputs();
                let mut feedback_outs = self.gather_feedback_outputs();
                self.as_processor_mut().unwrap().process(
                    clock, &inputs, &controls, &clocks, &feedbacks,
                    &mut outputs, &mut control_outs, &mut clock_outs, &mut feedback_outs,
                )
            }
            NodeCategory::Sink => {
                let inputs = self.gather_input_buffers();
                let controls = self.gather_control_inputs();
                let clocks = self.gather_clock_inputs();
                let feedbacks = self.gather_feedback_inputs();
                let mut control_outs = self.gather_control_outputs();
                let mut clock_outs = self.gather_clock_outputs();
                self.as_sink_mut().unwrap().consume(
                    clock, &inputs, &controls, &clocks, &feedbacks,
                    &mut control_outs, &mut clock_outs,
                )
            }
            _ => unreachable!(),
        }
    }

    /// Helper methods to gather buffers from ports (to be implemented by the graph).
    fn gather_input_buffers(&self) -> Vec<&[T; BUF_SIZE]>;
    fn gather_output_buffers(&mut self) -> Vec<&mut [T; BUF_SIZE]>;
    // ... similar for control, clock, feedback.
}
```

Blanket implementations can be provided for `Source`, `Processor`, `Sink` that bypass the gathering step and directly use the subtrait’s methods, improving performance.

## GraphBuilder and AudioGraph

### GraphBuilder (Mutable Construction)

```rust
pub struct GraphBuilder<T: AudioNum, const BUF_SIZE: usize> {
    nodes: Vec<NodeEntry<T, BUF_SIZE>>,
    connections: Vec<Connection>,
    next_id: NodeId,
}

impl<T: AudioNum, const BUF_SIZE: usize> GraphBuilder<T, BUF_SIZE> {
    pub fn new() -> Self { ... }
    
    /// Add a node to the graph, returning its NodeId.
    pub fn add_node(&mut self, node: Box<dyn AudioNode<T, BUF_SIZE>>) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.nodes.push(NodeEntry { node, id });
        id
    }
    
    /// Connect two ports. Creates a PipeBuffer and attaches it to both ports.
    pub fn connect(&mut self, from: PortId, to: PortId) -> Result<(), ConnectionError> {
        // Validate ports exist and are compatible.
        let buffer = PipeBuffer::new(BUF_SIZE);
        self.get_port_mut(from)?.connect(buffer.clone());
        self.get_port_mut(to)?.connect(buffer);
        self.connections.push(Connection { from, to });
        Ok(())
    }
    
    /// Build the immutable AudioGraph.
    pub fn build(self) -> Result<AudioGraph<T, BUF_SIZE>, BuildError> {
        // Validate connections, detect cycles.
        let topological_order = compute_topological_order(&self.nodes, &self.connections)?;
        // Precompute forward/backward reachable orders for each node.
        let (forward_orders, backward_orders) = compute_reachable_orders(&self.nodes, &self.connections);
        Ok(AudioGraph {
            nodes: self.nodes,
            connections: self.connections,
            topological_order,
            clock_source: Box::new(SystemClock::default()),
            current_tick: ClockTick::default(),
            forward_orders,
            backward_orders,
        })
    }
}
```

### AudioGraph (Immutable Processing)

```rust
pub struct AudioGraph<T: AudioNum, const BUF_SIZE: usize> {
    /// Nodes in index order.
    nodes: Vec<NodeEntry<T, BUF_SIZE>>,
    /// Connections (for introspection only; not used during processing).
    connections: Vec<Connection>,
    /// Topological order of node indices (full graph).
    topological_order: Vec<usize>,
    /// Clock source and current tick.
    clock_source: Box<dyn ClockSource>,
    current_tick: ClockTick,
    /// Precomputed processing order for each possible active node (forward direction).
    forward_orders: HashMap<NodeId, Vec<usize>>,
    /// Precomputed processing order for each possible active node (backward direction).
    backward_orders: HashMap<NodeId, Vec<usize>>,
}

impl<T: AudioNum, const BUF_SIZE: usize> AudioGraph<T, BUF_SIZE> {
    /// Process one block of audio starting from the given active node.
    pub fn process_block(&mut self, active_node: NodeId) -> ProcessResult<()> {
        self.current_tick = self.clock_source.next_tick(BUF_SIZE);
        // Determine direction based on node category (Source → forward, Sink → backward).
        let order = self.get_processing_order(active_node);
        for &idx in &order {
            let node = &mut self.nodes[idx].node;
            node.process_block(&self.current_tick)?;
        }
        // Advance feedback buffers (swap previous/current).
        self.advance_feedback_buffers();
        Ok(())
    }

    /// Retrieve the precomputed processing order for the given active node.
    fn get_processing_order(&self, active_node: NodeId) -> &[usize] {
        // Determine node category (requires access to node metadata).
        // For simplicity, assume forward direction; real implementation would inspect node.
        self.forward_orders.get(&active_node)
            .map(Vec::as_slice)
            .unwrap_or(&self.topological_order)
    }
}
```

### NodeEntry

```rust
struct NodeEntry<T: AudioNum, const BUF_SIZE: usize> {
    /// The node itself.
    node: Box<dyn AudioNode<T, BUF_SIZE>>,
    /// Node ID (cached for quick access).
    id: NodeId,
}
```

### Connection

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub from: PortId,
    pub to: PortId,
    /// Indicates whether this is a feedback connection (output → feedback port).
    /// Feedback connections are excluded from topological ordering and introduce a one‑block delay.
    pub is_feedback: bool,
}
```

## Processing Algorithm

### Initialization (Build Time)

1. **Validate connections** – Ensure ports exist and types match.
2. **Create PipeBuffers** – One per connection, shared between output and input ports.
3. **Detect cycles** – Use Kahn’s algorithm on audio and control connections (clock connections ignored for ordering). Feedback loops are allowed but require a one‑block delay.
4. **Compute topological order** – Store as `Vec<usize>` of node indices.
5. **Initialize clock** – Set up the clock source and initial tick.
6. **Precompute reachable subgraphs** (optional) – For each possible active node (source or sink), compute the set of nodes reachable in the direction of signal flow. This allows efficient processing of independent subgraphs.

### Active‑Node Driven Processing

The graph is processed by a designated **active node**, which is either a `Source` (produces audio) or a `Sink` (consumes audio). The active node generates a `ClockTick` that propagates through the graph, synchronizing all nodes within the same processing block.

The `AudioGraph::process_block` method takes an `active_node: NodeId` parameter:

```rust
pub fn process_block(&mut self, active_node: NodeId) -> ProcessResult<()> {
    self.current_tick = self.clock_source.next_tick(BUF_SIZE);
    // Determine direction: if active node is a Source, process forward; if Sink, process backward.
    let order = self.get_processing_order(active_node);
    for &idx in &order {
        self.process_single_node(idx)?;
    }
    self.advance_feedback_buffers();
    Ok(())
}
```

#### Direction of Processing

- **Forward processing** (active node is a `Source`): Signal flows from the active node downstream to its outputs. The processing order is a topological order restricted to nodes **reachable from the active node** following output connections.
- **Backward processing** (active node is a `Sink`): Signal flows from inputs toward the active node. The processing order is a **reverse topological order** restricted to nodes that can reach the active node via input connections.

The direction can be inferred from the node’s category (available via `NodeCategory`). If the node is both a source and a sink (e.g., a processor with external I/O), the default is forward.

#### Clock Tick Propagation

Each node receives the same `ClockTick` (the tick generated by the active node). Clock outputs are simply copies of the tick and do not affect the processing order. However, clock connections can be used to synchronize side‑chain processing or trigger events.

### Per‑Node Processing

For each node in the processing order:

1. **Gather inputs** – For each input port, retrieve the data from the connected `PipeBuffer`. If no connection exists, a zero buffer is used. Feedback inputs are gathered from the `FeedbackBuffer`s attached to feedback ports (these buffers contain data written in the previous block).

2. **Call `process_block`** – Invoke the node’s `AudioNode::process_block` method, passing the current `ClockTick`, input slices, feedback slices, and mutable output slices. The node is responsible for mixing the regular inputs with feedback signals as needed to implement the desired feedback behavior.

3. **Push outputs** – Write the output buffers into the corresponding `PipeBuffer`s, making them available to downstream nodes. Outputs destined for feedback ports are written to the “current” side of the `FeedbackBuffer` (to be used in the next block).

Because connections are immutable, the pull/push operations are guaranteed to have a consistent producer/consumer relationship.

### Feedback Loop Handling

Feedback connections are a special type of connection where an output port is connected to a **feedback port** of another node (or the same node). They are marked with `is_feedback: true` in the `Connection` struct.

Because feedback loops would create cycles in the signal flow, they are excluded from the topological ordering algorithm. Instead, each feedback connection is equipped with a `FeedbackBuffer` – a double‑buffer that introduces a one‑block delay.

During processing:
- Feedback inputs are read from the “previous” buffer (data written in the previous block).
- Feedback outputs are written to the “current” buffer (to be used in the next block).
- After each block, the two buffers are swapped.

This mechanism allows feedback loops to exist while preserving the strict forward direction of signal propagation driven by the active node’s `ClockTick`.

### External Inputs/Outputs

External audio I/O is modeled as special nodes (`Source` for inputs, `Sink` for outputs). Their ports are connected to the host’s audio buffers before each call to `process_block` (e.g., via `Port::connect` with a temporary buffer). This allows the graph to remain immutable while supporting dynamic external data.

## Performance Optimizations

Despite using `PipeBuffer` indirection, the immutable graph still enables several important optimizations:

### Precomputed Topological Order
- Computed once at build time; no runtime sorting.
- Reachable subgraphs for each possible active node are precomputed, enabling efficient processing of independent subgraphs.

### Buffer Reuse
- `PipeBuffer` instances are allocated once and never re‑allocated.
- The buffer storage is a fixed‑size array that is reused every block.

### Zero Allocations During Processing
- The processing loop does not allocate any memory on the heap.
- `pull` and `push` operate on pre‑allocated buffers.

### Cache‑Friendly Data Layout
- Nodes are stored in a vector indexed by node ID; iteration follows topological order.
- `PipeBuffer` internal storage is aligned for SIMD operations.

### Feedback Buffer Optimizations
- Double‑buffering uses pointer swapping (O(1)) rather than copying.

## Immutability Enforcement

- `AudioGraph` has no public methods to modify topology (no `add_node`, `connect`, `remove_node`). The only mutable operations are `process_block` (which modifies node internal state and buffers) and `set_parameter` (via command queue).
- `GraphBuilder` is the only way to construct a graph; after `build()`, the builder is consumed and cannot be reused.

## Integration with Existing Code

- Existing nodes that implement `Source`, `Processor`, or `Sink` will automatically work with the new graph because they already have ports that implement `ActivePort`. The default `process_block` implementation will call the appropriate subtrait method.
- The graph stores `Box<dyn AudioNode>`; each node’s ports are already connected via `PipeBuffer`s created during construction.
- No changes are required to existing node implementations (unless they rely on `DynProcessor`‑specific APIs).

## Open Questions

1. **Performance of `PipeBuffer` vs direct buffer references** – Is the extra indirection acceptable for real‑time processing? Profiling will be needed.
2. **Handling control and clock signals** – Should they use the same `PipeBuffer` mechanism or be passed as simple values?
3. **Parameter automation** – How to integrate command queues for parameter changes without breaking immutability? (Answer: queues are separate and can be processed before `process_block`.)

## Next Steps

1. **Implement `AudioNode::process_block` default implementation** in `kama-core`.
2. **Extend `GraphBuilder` and `AudioGraph`** in `kama‑graph` to use `ActivePort` and `PipeBuffer` connections.
3. **Update existing graph tests** to verify immutability and correct processing.
4. **Benchmark** the new graph against the old `DynProcessor`‑based implementation.

## Appendix: Example Usage

```rust
let mut builder = GraphBuilder::<f32, 64>::new();
let osc_id = builder.add_node(Box::new(SineOsc::new(440.0)));
let filter_id = builder.add_node(Box::new(BiquadFilter::lowpass(1000.0, 0.707)));
builder.connect(PortId::audio_out(osc_id, 0), PortId::audio_in(filter_id, 0))?;

let mut graph = builder.build()?;

// Real‑time processing loop
loop {
    graph.process_block();
    // output graph's sink nodes to audio interface
}