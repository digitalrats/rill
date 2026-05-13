//! DOT graph visualization for patchbay (automation, mappings).
//!
//! # Feature gate
//!
//! Requires the `serde` feature (for [`RackDef`] access).

use std::fmt::Write;

#[cfg(feature = "serde")]
use crate::serialization::{ModuleDef, RackDef};

/// Configuration for patchbay DOT generation.
#[derive(Default)]
pub struct DotConfig {
    /// Show parameter values in node labels.
    pub show_values: bool,
    /// Include auto-generated internal details.
    pub verbose: bool,
}

/// Generate DOT from a RackDef.
#[cfg(feature = "serde")]
pub fn rack_to_dot(patchbay: &RackDef, _config: &DotConfig) -> String {
    let mut dot = String::new();
    writeln!(dot, "// Patchbay control graph").ok();
    writeln!(dot, "digraph patchbay {{").ok();
    writeln!(dot, "    rankdir=LR;").ok();
    writeln!(dot, "    node [style=filled, fontname=\"Sans\"];").ok();
    writeln!(dot, "    edge [fontname=\"Sans\", fontsize=10];").ok();
    writeln!(dot, "    ranksep=2;").ok();

    // ── Automata cluster ──────────────────────────────────────────
    writeln!(dot, "    subgraph cluster_automata {{").ok();
    writeln!(dot, "        label=\"Automation Sources\";").ok();
    writeln!(dot, "        style=filled;").ok();
    writeln!(dot, "        fillcolor=\"#f0f0ff\";").ok();
    writeln!(dot, "        fontcolor=\"#44f\";").ok();

    for auto in &patchbay.automata {
        let id = auto.id();
        let (label, fillcolor) = match auto {
            crate::serialization::AutomatonDef::Lfo {
                frequency,
                waveform,
                ..
            } => {
                let l = format!("LFO\\n{frequency} Hz\\n{waveform:?}");
                (l, "#ccf")
            }
            crate::serialization::AutomatonDef::Envelope {
                attack,
                decay,
                sustain,
                release,
                ..
            } => {
                let l = format!("Envelope\\n{attack}/{decay}/{sustain}/{release}");
                (l, "#fcf")
            }
            crate::serialization::AutomatonDef::Sequencer { steps, tempo, .. } => {
                let l = format!("SequencerAuto\\n{} steps @ {tempo} BPM", steps.len());
                (l, "#ffc")
            }
            crate::serialization::AutomatonDef::NamedFunction { function_name, .. } => {
                let l = format!("Fn\\n{function_name}");
                (l, "#cfc")
            }
            crate::serialization::AutomatonDef::Custom { type_name, .. } => {
                let l = format!("Custom\\n{type_name}");
                (l, "#fcf")
            }
        };
        let escaped = label.replace('\"', "\\\"");
        writeln!(
            dot,
            "        auto_{id} [label=\"{escaped}\", shape=box, fillcolor=\"{fillcolor}\"];"
        )
        .ok();
    }

    writeln!(dot, "    }}").ok();

    // ── Servo edges (automaton → target) ──────────────────────────
    for m in &patchbay.modules {
        if let ModuleDef::Servo(srv) = m {
            let auto_id = &srv.automaton_id;
            let target = format!("node_{}:{}", srv.target_node, srv.target_param);
            let label = format!("{:?} [{:.3}, {:.3}]", srv.mapping, srv.min, srv.max);
            writeln!(
            dot,
            "    auto_{auto_id} -> param_{target} [label=\"{label}\", style=dashed, color=\"#44a\"];"
        ).ok();
        }
    }

    // ── Mappings ──────────────────────────────────────────────────
    for (i, map) in patchbay.mappings.iter().enumerate() {
        let pat_str = format!("{:?}", map.event_pattern);
        let target = format!("node_{}:{}", map.target_node, map.target_param);
        writeln!(
            dot,
            "    map_{i} [label=\"{pat_str}\", shape=note, fillcolor=\"#eee\"];"
        )
        .ok();
        writeln!(
            dot,
            "    map_{i} -> param_{target} [label=\"\", style=dotted, color=\"#888\"];"
        )
        .ok();
    }

    writeln!(dot, "}}").ok();
    dot
}
