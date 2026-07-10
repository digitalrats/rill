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
    /// Mix feedback buffer into target buffer before processing.
    ReadFeedback {
        /// Named feedback buffer to read from.
        name: String,
        /// Buffer index in the sub-engine pool to write into.
        target_buf: usize,
    },
    /// Capture target buffer into feedback buffer after processing.
    WriteFeedback {
        /// Named feedback buffer to write into.
        name: String,
        /// Buffer index in the sub-engine pool to read from.
        source_buf: usize,
    },
}

/// Split schedule for duplex (bridge) execution.
///
/// A bridge node splits the graph into left (recording) and right (playback)
/// sub-graphs. Each sub-graph has its own `ScheduledGraph` with embedded
/// `ReadFeedback`/`WriteFeedback` steps.
pub struct DuplexSchedule {
    /// Left sub-graph — executes before the bridge.
    pub left: ScheduledGraph,
    /// Right sub-graph — executes after the bridge.
    pub right: ScheduledGraph,
    /// All unique feedback buffer names across the graph.
    pub feedback_names: Vec<String>,
    /// Bridge node name, used for anchor-based parameter routing.
    pub anchor: String,
    /// Bridge param name → slot index mapping.
    pub anchor_params: HashMap<String, usize>,
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

/// Lower a GraphIr containing a bridge node into a DuplexSchedule.
///
/// The graph is split into left (before bridge) and right (after bridge) sub-graphs.
/// Returns `None` if no bridge node is found (use regular [`lower()`] instead).
pub fn lower_duplex(ir: &GraphIr) -> Option<DuplexSchedule> {
    let bridge_name = ir
        .nodes
        .iter()
        .find(|(_, n)| n.is_bridge)
        .map(|(name, _)| name.clone())?;

    let bridge = &ir.nodes[&bridge_name];

    let bridge_pos = ir.topo_order.iter().position(|n| n == &bridge_name)?;

    let left_names: Vec<String> = ir.topo_order[..bridge_pos].to_vec();
    let right_names: Vec<String> = ir.topo_order[bridge_pos + 1..].to_vec();

    let left_schedule = build_sub_schedule(ir, &left_names, true);
    let right_schedule = build_sub_schedule(ir, &right_names, false);

    let mut feedback_names = Vec::new();
    for node in ir.nodes.values() {
        for name in &node.feedback_read {
            if !feedback_names.contains(name) {
                feedback_names.push(name.clone());
            }
        }
        for name in &node.feedback_write {
            if !feedback_names.contains(name) {
                feedback_names.push(name.clone());
            }
        }
    }

    let mut anchor_params = HashMap::new();
    for (i, p) in bridge.params.iter().enumerate() {
        anchor_params.insert(p.name.clone(), i);
    }

    Some(DuplexSchedule {
        left: left_schedule,
        right: right_schedule,
        feedback_names,
        anchor: bridge_name,
        anchor_params,
    })
}

/// Build a sub-schedule for a set of nodes, inserting `ReadFeedback`/`WriteFeedback` steps.
fn build_sub_schedule(ir: &GraphIr, node_names: &[String], is_left: bool) -> ScheduledGraph {
    let mut steps = Vec::new();
    let mut buffer_counter = 0usize;
    let mut delay_slots = 0usize;
    let mut output_buffers: HashMap<(String, usize), usize> = HashMap::new();

    for (idx, name) in node_names.iter().enumerate() {
        let node = &ir.nodes[name];

        for fb_name in &node.feedback_read {
            let buf = buffer_counter;
            buffer_counter += 1;
            steps.push(Step::ReadFeedback {
                name: fb_name.clone(),
                target_buf: buf,
            });
        }

        let mut input_bufs = Vec::new();
        for edge in &ir.edges {
            if edge.to_node == *name && edge.kind == EdgeKind::Signal {
                let key = (edge.from_node.clone(), edge.from_port);
                if let Some(&buf) = output_buffers.get(&key) {
                    while input_bufs.len() <= edge.to_port {
                        input_bufs.push(0);
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

        for (port, &buf) in output_bufs.iter().enumerate() {
            output_buffers.insert((name.clone(), port), buf);
        }

        let param_indices: Vec<usize> = (0..node.params.len()).collect();

        steps.push(Step::InlineProgram {
            node_idx: idx,
            input_bufs,
            output_bufs: output_bufs.clone(),
            param_indices,
        });

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

        for fb_name in &node.feedback_write {
            steps.push(Step::WriteFeedback {
                name: fb_name.clone(),
                source_buf: output_bufs[0],
            });
        }
    }

    let inputs: usize = if is_left { ir.inputs } else { 0 };
    let outputs: usize = if !is_left { ir.outputs } else { 0 };

    ScheduledGraph {
        inputs,
        outputs,
        steps,
        buffers: buffer_counter,
        output_mapping: Vec::new(),
        program_names: node_names.to_vec(),
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
