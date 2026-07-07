//! ScheduledGraph — pre-compiled execution plan with buffer allocation.

use crate::graph_ir::GraphIr;
use crate::graph_lower::SchedulingOrder;
use std::collections::HashMap;

/// A step in the scheduled execution plan.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Execute an inlined program node.
    InlineProgram {
        /// Index into the programs vector in RillGraphEngine.
        node_idx: usize,
        /// Input buffer indices (may alias previous outputs — zero-copy).
        input_bufs: Vec<usize>,
        /// Output buffer indices.
        output_bufs: Vec<usize>,
        /// Parameter indices within the program to set before execution.
        param_indices: Vec<usize>,
    },
    /// Copy (or accumulate) between buffer slots.
    /// Used for fan-out (overwrite) and fan-in (add).
    BufferCopy {
        /// Source buffer index.
        from: usize,
        /// Destination buffer index.
        to: usize,
        /// Linear gain applied during copy (1.0 = identity).
        gain: f32,
        /// false = overwrite, true = accumulate (+=)
        add: bool,
    },
    /// Read a delay buffer (previous tick's feedback) into a target buffer.
    ReadDelay {
        /// Delay slot index.
        slot: usize,
        /// Target buffer index.
        target: usize,
    },
    /// Save a source buffer into a delay slot for the next tick.
    WriteDelay {
        /// Source buffer index.
        source: usize,
        /// Delay slot index.
        slot: usize,
    },
}

/// Pre-compiled execution plan with buffer allocation.
#[derive(Debug, Clone)]
pub struct ScheduledGraph {
    /// Number of external input channels.
    pub inputs: usize,
    /// Number of external output channels.
    pub outputs: usize,
    /// Ordered execution steps.
    pub steps: Vec<Step>,
    /// Total number of buffer slots needed.
    pub buffers: usize,
    /// Total number of delay (feedback) buffer slots needed.
    pub delay_slots: usize,
    /// Which buffer indices map to output channels.
    pub output_mapping: Vec<usize>,
}

/// Build a ScheduledGraph from GraphIr and SchedulingOrder.
pub fn build_scheduled_graph(ir: &GraphIr, order: &SchedulingOrder) -> ScheduledGraph {
    let name_to_idx: HashMap<&str, usize> = ir
        .nodes
        .keys()
        .enumerate()
        .map(|(i, name)| (name.as_str(), i))
        .collect();

    let mut buf_count: usize = 0;
    let mut edge_buf: HashMap<(String, String, usize), usize> = HashMap::new();

    for (to_name, inputs) in &order.node_inputs {
        for (from_name, _from_port, to_port) in inputs {
            let key = (from_name.clone(), to_name.clone(), *to_port);
            if !edge_buf.contains_key(&key) {
                edge_buf.insert(key.clone(), buf_count);
                buf_count += 1;
            }
        }
    }

    let mut steps: Vec<Step> = Vec::new();
    let mut node_buf_inputs: HashMap<String, Vec<usize>> = HashMap::new();
    let mut node_buf_outputs: HashMap<String, Vec<usize>> = HashMap::new();

    for (_producer, consumer, slot) in &order.feedback_pairs {
        if name_to_idx.contains_key(consumer.as_str()) {
            let target_buf = buf_count;
            buf_count += 1;
            steps.push(Step::ReadDelay {
                slot: *slot,
                target: target_buf,
            });
            node_buf_inputs
                .entry(consumer.clone())
                .or_default()
                .push(target_buf);
        }
    }

    for node_name in &order.topo_order {
        let node_idx = name_to_idx[node_name.as_str()];
        let node = &ir.nodes[node_name];

        let mut input_bufs: Vec<usize> = Vec::new();
        if let Some(inputs) = order.node_inputs.get(node_name) {
            for (from_name, _from_port, to_port) in inputs {
                let key = (from_name.clone(), node_name.clone(), *to_port);
                if let Some(&buf) = edge_buf.get(&key) {
                    if !input_bufs.contains(&buf) {
                        input_bufs.push(buf);
                    }
                }
            }
        }

        if let Some(extra) = node_buf_inputs.remove(node_name) {
            for b in extra {
                if !input_bufs.contains(&b) {
                    input_bufs.push(b);
                }
            }
        }

        let output_bufs: Vec<usize> = (0..node.arity.1)
            .map(|_| {
                let b = buf_count;
                buf_count += 1;
                b
            })
            .collect();

        let param_count = node.ir.params.len();
        steps.push(Step::InlineProgram {
            node_idx,
            input_bufs,
            output_bufs: output_bufs.clone(),
            param_indices: (0..param_count).collect(),
        });

        node_buf_outputs.insert(node_name.clone(), output_bufs);
    }

    for (producer, _consumer, slot) in &order.feedback_pairs {
        if let Some(outputs) = node_buf_outputs.get(producer) {
            if let Some(&source) = outputs.first() {
                steps.push(Step::WriteDelay {
                    source,
                    slot: *slot,
                });
            }
        }
    }

    let output_mapping = if let Some(last) = order.topo_order.last() {
        node_buf_outputs.get(last).cloned().unwrap_or_default()
    } else {
        vec![]
    };

    ScheduledGraph {
        inputs: ir.inputs,
        outputs: ir.outputs,
        steps,
        buffers: buf_count,
        delay_slots: order.feedback_pairs.len(),
        output_mapping,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_ir::{EdgeKind, GraphEdge, GraphNode};
    use crate::graph_lower::{lower_graph, SchedulingOrder};
    use indexmap::IndexMap;
    use std::collections::HashMap;

    fn empty_node() -> GraphNode {
        GraphNode {
            arity: (1, 1),
            ir: crate::ir::Ir {
                instrs: vec![],
                num_regs: 0,
                output_reg: 0,
                num_inputs: 1,
                state: Default::default(),
                builtins: vec![],
                params: vec![],
            },
            params: vec![],
            keep: false,
            force_inline: false,
        }
    }

    #[test]
    fn simple_chain_produces_two_steps() {
        let mut nodes = IndexMap::new();
        nodes.insert("a".into(), empty_node());
        nodes.insert("b".into(), empty_node());

        let ir = GraphIr {
            inputs: 0,
            outputs: 1,
            nodes,
            edges: vec![GraphEdge {
                from_node: "a".into(),
                from_port: 0,
                to_node: "b".into(),
                to_port: 0,
                kind: EdgeKind::Signal,
            }],
        };
        let order = lower_graph(&ir).unwrap();
        let sched = build_scheduled_graph(&ir, &order);

        assert_eq!(sched.steps.len(), 2, "one step per node in simple chain");
        assert!(sched.buffers > 0);
    }

    #[test]
    fn feedback_creates_delay_steps() {
        let mut nodes = IndexMap::new();
        nodes.insert("a".into(), empty_node());
        nodes.insert("b".into(), empty_node());

        let ir = GraphIr {
            inputs: 0,
            outputs: 1,
            nodes,
            edges: vec![
                GraphEdge {
                    from_node: "a".into(),
                    from_port: 0,
                    to_node: "b".into(),
                    to_port: 0,
                    kind: EdgeKind::Signal,
                },
                GraphEdge {
                    from_node: "b".into(),
                    from_port: 0,
                    to_node: "a".into(),
                    to_port: 0,
                    kind: EdgeKind::Feedback,
                },
            ],
        };
        let order = lower_graph(&ir).unwrap();
        let sched = build_scheduled_graph(&ir, &order);

        let has_read = sched
            .steps
            .iter()
            .any(|s| matches!(s, Step::ReadDelay { .. }));
        let has_write = sched
            .steps
            .iter()
            .any(|s| matches!(s, Step::WriteDelay { .. }));
        assert!(has_read, "feedback creates ReadDelay step");
        assert!(has_write, "feedback creates WriteDelay step");
    }

    #[test]
    fn empty_graph_produces_empty_schedule() {
        let ir = GraphIr {
            inputs: 0,
            outputs: 1,
            nodes: IndexMap::new(),
            edges: vec![],
        };
        let order = SchedulingOrder {
            topo_order: vec![],
            feedback_pairs: vec![],
            node_inputs: HashMap::new(),
            node_outputs: HashMap::new(),
        };
        let sched = build_scheduled_graph(&ir, &order);
        assert!(sched.steps.is_empty());
        assert_eq!(sched.buffers, 0);
    }
}
