//! DOT graph serialization for Graphviz visualization.
//!
//! Generates `digraph` DOT format from a [`Graph`].  Uses
//! R-scheme (Glushkov R-scheme) inspired node shapes:
//!
//! | NodeCategory | DOT shape | Color | Meaning |
//! |---|---|---|---|
//! | `Source` | `trapezium` | green `#8f8` | Generator operator |
//! | `Processor` | `box` | blue `#8bf` | Processor operator |
//! | `Router` | `diamond` | orange `#fa8` | Condition/distributor |
//! | `Sink` | `invtrapezium` | red `#f88` | Terminal operator |
//! | `Analyzer` | `note` | grey `#ccc` | Observer |
//! | `Sequencer` | `doublecircle` | pink `#fcf` | Control operator |
//!
//! ## Feature gate
//!
//! This module is available when the `dot` feature is enabled.

use std::fmt::Write;

use rill_core::traits::processable::NodeVariant;
use rill_core::traits::{Node, NodeCategory};
use rill_core::Transcendental;

use crate::graph::Graph;

/// Configuration for DOT graph generation.
pub struct DotConfig {
    /// Whether to group nodes into subgraph clusters by category.
    pub cluster_by_category: bool,
    /// Whether to show port names on edges.
    pub show_port_names: bool,
    /// Whether to show parameter values in node labels.
    pub show_parameters: bool,
    /// Graph direction: LR = left-to-right, TB = top-to-bottom.
    pub rankdir: &'static str,
    /// Graph title (shown as label).
    pub title: String,
}

impl Default for DotConfig {
    fn default() -> Self {
        Self {
            cluster_by_category: true,
            show_port_names: true,
            show_parameters: false,
            rankdir: "LR",
            title: "Rill Signal Graph".into(),
        }
    }
}

/// Generate a DOT string from a signal graph.
pub fn to_dot<T: Transcendental, const B: usize>(
    graph: &Graph<T, B>,
    config: &DotConfig,
) -> String {
    let mut dot = String::new();

    writeln!(dot, "// {title}", title = config.title).ok();
    writeln!(dot, "digraph rill_graph {{").ok();
    writeln!(dot, "    rankdir={rd};", rd = config.rankdir).ok();
    writeln!(dot, "    splines=true;").ok();
    writeln!(dot, "    node [style=filled, fontname=\"Sans\"];").ok();
    writeln!(dot, "    edge [fontname=\"Sans\", fontsize=10];").ok();

    let nodes = graph.nodes();

    if config.cluster_by_category {
        emit_clusters(&mut dot, nodes, config);
    } else {
        for (idx, variant) in nodes.iter().enumerate() {
            emit_node(&mut dot, idx, variant, config);
        }
    }

    // ── Edges ──────────────────────────────────────────────────────────
    // Walk every node's output ports and create DOT edges from downstream.
    let topo = graph.topo_order();
    for &node_idx in topo {
        let variant = &nodes[node_idx];
        let id = variant.id().inner();

        for p in 0..variant.num_signal_outputs() {
            if let Some(port) = variant.output_port(p) {
                for &(target_node, _) in &port.downstream {
                    let target_entry = &nodes[target_node];
                    let target_id = target_entry.id().inner();

                    let from_label = port_name(&port_name_by_index(p, "out"));

                    write!(dot, "    node_{id} -> node_{target_id}").ok();
                    if config.show_port_names {
                        write!(dot, " [label=\"sig {from_label}\"").ok();
                    } else {
                        write!(dot, " [label=\"\"").ok();
                    }
                    writeln!(dot, " color=\"#448\", fontcolor=\"#448\"];").ok();
                }
            }
        }
    }

    writeln!(dot, "}}").ok();
    dot
}

// ============================================================================
// Node emission
// ============================================================================

fn emit_node<T: Transcendental, const B: usize>(
    dot: &mut String,
    _idx: usize,
    variant: &NodeVariant<T, B>,
    config: &DotConfig,
) {
    let meta = variant.metadata();
    let id = variant.id().inner();
    let cat = meta.category;

    let (shape, fillcolor) = category_style(cat);

    // Build label
    let mut label = String::new();
    write!(label, "{}\\n", meta.name).ok();
    if let Some(ref tn) = meta.type_name {
        write!(label, "<i>{tn}</i>").ok();
    }

    if config.show_parameters {
        for pm in meta.parameters.iter().take(5) {
            if let Ok(pid) = rill_core::ParameterId::new(&pm.name) {
                if let Some(val) = variant.get_parameter(&pid) {
                    let val_str = format!("{:?}", val);
                    write!(label, "\\n{}={}", pm.name, val_str).ok();
                }
            }
        }
    }

    let escaped_label = label.replace('\"', "\\\"");

    writeln!(dot, "    node_{id} [label=\"{escaped_label}\"",).ok();
    writeln!(dot, "          shape={shape}").ok();
    writeln!(dot, "          fillcolor=\"{fillcolor}\"];").ok();
}

fn emit_clusters<T: Transcendental, const B: usize>(
    dot: &mut String,
    nodes: &[NodeVariant<T, B>],
    config: &DotConfig,
) {
    let mut by_cat: std::collections::BTreeMap<String, Vec<(usize, &NodeVariant<T, B>)>> =
        std::collections::BTreeMap::new();

    for (idx, variant) in nodes.iter().enumerate() {
        let cat = variant.metadata().category;
        let cat_name = cat.name();
        // Capitalize
        let mut key = String::new();
        let mut chars = cat_name.chars();
        if let Some(c) = chars.next() {
            key.extend(c.to_uppercase());
            key.push_str(chars.as_str());
        }
        by_cat.entry(key).or_default().push((idx, variant));
    }

    for (cat_name, nodes) in &by_cat {
        let first = nodes
            .first()
            .map(|(_, n)| n.metadata().category)
            .unwrap_or(NodeCategory::Processor);
        let (_, fillcolor) = category_style(first);

        writeln!(dot, "    subgraph cluster_{cat_name} {{").ok();
        writeln!(dot, "        label=\"{cat_name}\";").ok();
        writeln!(dot, "        style=filled;").ok();
        writeln!(dot, "        fillcolor=\"#f8f8f8\";").ok();
        writeln!(dot, "        fontcolor=\"{fillcolor}\";").ok();
        writeln!(dot, "        fontsize=14;").ok();

        for (idx, variant) in nodes {
            emit_node(dot, *idx, variant, config);
        }

        writeln!(dot, "    }}").ok();
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn category_style(cat: NodeCategory) -> (&'static str, &'static str) {
    match cat {
        NodeCategory::Source => ("trapezium", "#8f8"),
        NodeCategory::Processor => ("box", "#8bf"),
        NodeCategory::Utility => ("diamond", "#fa8"),
        NodeCategory::Sink => ("invtrapezium", "#f88"),
        NodeCategory::Analyzer => ("note", "#ccc"),
        NodeCategory::Sequencer => ("doublecircle", "#fcf"),
    }
}

fn port_name(s: &str) -> String {
    if s.is_empty() {
        "?".to_string()
    } else {
        s.to_string()
    }
}

fn port_name_by_index(idx: usize, dir: &str) -> String {
    format!("{dir}_{idx}")
}
