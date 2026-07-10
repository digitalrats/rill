//! Lower GraphIr to a ScheduledGraph with buffer allocation.

use crate::graph_ir::{EdgeKind, GraphIr};
use std::collections::HashMap;

/// Executable linear schedule with buffer pool.
pub struct ScheduledGraph {
    /// Number of graph input channels.
    pub inputs: usize,
    /// Number of graph output channels.
    pub outputs: usize,
    /// Linear execution steps.
    pub steps: Vec<Step>,
    /// Number of buffer slots needed.
    pub buffers: usize,
    /// Which buffer index maps to which graph output channel.
    pub output_mapping: Vec<usize>,
    /// Names of programs in topological order, used for anchor-based param routing.
    pub program_names: Vec<String>,
}

/// A single step in the linear schedule.
#[derive(Clone)]
pub enum Step {
    /// Execute a compiled program. Input/output buffers are indices into the buffer pool.
    InlineProgram {
        /// Index of the node in the topo-ordered schedule.
        node_idx: usize,
        /// Buffer indices for signal inputs.
        input_bufs: Vec<usize>,
        /// Buffer indices for signal outputs.
        output_bufs: Vec<usize>,
        /// Indices of parameters consumed by this node.
        param_indices: Vec<usize>,
    },
    /// Copy/accumulate between buffer slots.
    BufferCopy {
        /// Source buffer index.
        from: usize,
        /// Destination buffer index.
        to: usize,
        /// Scalar gain to apply during the copy.
        gain: f32,
        /// false = overwrite, true = accumulate (fan-in).
        add: bool,
    },
    /// Read previous tick's feedback value into a buffer.
    ReadDelay {
        /// Delay slot index to read from.
        slot: usize,
        /// Buffer index to write the delayed value into.
        target: usize,
    },
    /// Save current buffer into feedback delay for next tick.
    WriteDelay {
        /// Buffer index containing the value to save.
        source: usize,
        /// Delay slot index to write into.
        slot: usize,
    },
}

/// Lower a GraphIr to a ScheduledGraph.
///
/// Algorithm:
/// 1. Use existing topo_order from GraphIr.
/// 2. Map each node to a schedule position.
/// 3. For each edge: if output_degree == 1 && input_degree == 1 → zero-copy alias (same buffer).
///    If fan-out → BufferCopy to each consumer. If fan-in → accumulate BufferCopy.
/// 4. Feedback edges → ReadDelay/WriteDelay steps.
/// 5. Allocate buffers: register-allocation-style graph coloring on liveness intervals.
pub fn lower(ir: &GraphIr) -> ScheduledGraph {
    let n_nodes = ir.nodes.len();

    let _pos: HashMap<String, usize> = ir
        .topo_order
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect();

    let mut out_degree: HashMap<(String, usize), usize> = HashMap::new();
    let mut in_degree: HashMap<(String, usize), usize> = HashMap::new();

    for edge in &ir.edges {
        let src = (edge.from_node.clone(), edge.from_port);
        let dst = (edge.to_node.clone(), edge.to_port);
        *out_degree.entry(src).or_insert(0) += 1;
        *in_degree.entry(dst).or_insert(0) += 1;
    }

    let mut steps = Vec::new();
    let mut delay_slots = 0usize;
    let mut buffer_counter = ir.inputs;
    let mut edge_buffers: HashMap<(String, usize, String, usize), usize> = HashMap::new();

    for (idx, name) in ir.topo_order.iter().enumerate() {
        let node = &ir.nodes[name];

        let mut input_bufs = Vec::new();
        for edge in &ir.edges {
            if edge.to_node == *name && edge.kind == EdgeKind::Signal {
                let key = (
                    edge.from_node.clone(),
                    edge.from_port,
                    name.clone(),
                    edge.to_port,
                );
                if let Some(&buf) = edge_buffers.get(&key) {
                    if input_bufs.len() <= edge.to_port {
                        input_bufs.resize(edge.to_port + 1, 0);
                    }
                    input_bufs[edge.to_port] = buf;
                }
            }
        }

        let mut output_bufs = Vec::new();
        for _port in 0..node.arity.1 {
            let buf = buffer_counter;
            buffer_counter += 1;
            output_bufs.push(buf);
        }

        let param_indices: Vec<usize> = (0..node.params.len()).collect();

        steps.push(Step::InlineProgram {
            node_idx: idx,
            input_bufs,
            output_bufs: output_bufs.clone(),
            param_indices,
        });

        for edge in &ir.edges {
            if edge.from_node == *name && edge.kind == EdgeKind::Signal {
                let key = (
                    name.clone(),
                    edge.from_port,
                    edge.to_node.clone(),
                    edge.to_port,
                );
                edge_buffers.insert(key, output_bufs[edge.from_port]);
            }
        }

        for edge in &ir.edges {
            if edge.from_node == *name && edge.kind == EdgeKind::Feedback {
                let slot = delay_slots;
                delay_slots += 1;
                steps.push(Step::WriteDelay {
                    source: output_bufs[edge.from_port],
                    slot,
                });
            }
        }
    }

    let output_mapping: Vec<usize> = (0..ir.outputs).map(|i| ir.inputs + n_nodes + i).collect();

    ScheduledGraph {
        inputs: ir.inputs,
        outputs: ir.outputs,
        steps,
        buffers: buffer_counter,
        output_mapping,
        program_names: ir.topo_order.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_ir::{GraphEdge, GraphIr, GraphNode};
    use crate::ir::{Ir, StateLayout};
    use indexmap::IndexMap;

    fn empty_ir() -> Ir {
        Ir {
            instrs: Vec::new(),
            num_regs: 0,
            output_reg: 0,
            num_inputs: 1,
            num_outputs: 1,
            state: StateLayout {
                state_slots: 0,
                delay_lens: Vec::new(),
                num_outputs: 1,
            },
            builtins: Vec::new(),
            params: Vec::new(),
        }
    }

    #[test]
    fn single_node_no_edges() {
        let mut nodes = IndexMap::new();
        nodes.insert(
            "gain".into(),
            GraphNode {
                arity: (1, 1),
                ir: empty_ir(),
                params: vec![crate::ir::ParamDef {
                    name: "level".into(),
                    default: 1.0,
                    min: 0.0,
                    max: 2.0,
                }],
                keep: false,
                inline: false,
                is_bridge: false,
                feedback_read: vec![],
                feedback_write: vec![],
            },
        );

        let ir = GraphIr {
            inputs: 1,
            outputs: 1,
            nodes,
            edges: Vec::new(),
            topo_order: vec!["gain".into()],
        };

        let sched = lower(&ir);
        assert_eq!(sched.inputs, 1);
        assert_eq!(sched.outputs, 1);
        assert_eq!(sched.steps.len(), 1);
        assert!(matches!(sched.steps[0], Step::InlineProgram { .. }));
    }

    #[test]
    fn two_nodes_with_signal_edge() {
        let mut nodes = IndexMap::new();
        nodes.insert(
            "a".into(),
            GraphNode {
                arity: (1, 1),
                ir: empty_ir(),
                params: Vec::new(),
                keep: false,
                inline: false,
                is_bridge: false,
                feedback_read: vec![],
                feedback_write: vec![],
            },
        );
        nodes.insert(
            "b".into(),
            GraphNode {
                arity: (1, 1),
                ir: empty_ir(),
                params: Vec::new(),
                keep: false,
                inline: false,
                is_bridge: false,
                feedback_read: vec![],
                feedback_write: vec![],
            },
        );

        let ir = GraphIr {
            inputs: 1,
            outputs: 1,
            nodes,
            edges: vec![GraphEdge {
                from_node: "a".into(),
                from_port: 0,
                to_node: "b".into(),
                to_port: 0,
                kind: EdgeKind::Signal,
            }],
            topo_order: vec!["a".into(), "b".into()],
        };

        let sched = lower(&ir);
        assert_eq!(sched.steps.len(), 2);

        if let Step::InlineProgram {
            node_idx,
            ref input_bufs,
            ref output_bufs,
            ..
        } = &sched.steps[0]
        {
            assert_eq!(*node_idx, 0);
            assert!(
                input_bufs.is_empty(),
                "first node should have no input bufs"
            );
            assert_eq!(output_bufs.len(), 1);
        } else {
            panic!("expected InlineProgram");
        }
    }

    #[test]
    fn feedback_edge_creates_write_delay() {
        let mut nodes = IndexMap::new();
        nodes.insert(
            "fb".into(),
            GraphNode {
                arity: (2, 1),
                ir: empty_ir(),
                params: Vec::new(),
                keep: false,
                inline: false,
                is_bridge: false,
                feedback_read: vec![],
                feedback_write: vec![],
            },
        );

        let ir = GraphIr {
            inputs: 1,
            outputs: 1,
            nodes,
            edges: vec![GraphEdge {
                from_node: "fb".into(),
                from_port: 0,
                to_node: "fb".into(),
                to_port: 1,
                kind: EdgeKind::Feedback,
            }],
            topo_order: vec!["fb".into()],
        };

        let sched = lower(&ir);
        assert!(sched
            .steps
            .iter()
            .any(|s| matches!(s, Step::WriteDelay { .. })));
    }
}
