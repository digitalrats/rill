//! DOT graph visualization for patchbay (automation, mappings, sequencer).
//!
//! Generates `digraph` DOT format showing:
//! - Automation sources (LFO, Envelope, old SequencerAutomaton)
//! - Parameter-lock snapshot sequencer with step table
//! - Servo mappings (automaton → parameter)
//! - MIDI/OSC event mappings
//! - Connections to signal graph node parameters
//!
//! # Feature gate
//!
//! Requires the `serde` feature (for [`PatchbayDef`](crate::serialization::PatchbayDef)
//! and [`SequencerDef`](crate::sequencer::SequencerDef) access).

use std::fmt::Write;

#[cfg(feature = "serde")]
use crate::serialization::PatchbayDef;
#[cfg(feature = "serde")]
use crate::serialization::SequencerDef;

/// Configuration for patchbay DOT generation.
#[derive(Default)]
pub struct DotConfig {
    /// Show parameter values in node labels.
    pub show_values: bool,
    /// Include auto-generated internal details.
    pub verbose: bool,
}

/// Generate DOT from a PatchbayDef and optional SequencerDef.
#[cfg(feature = "serde")]
pub fn patchbay_to_dot(
    patchbay: &PatchbayDef,
    sequencer: Option<&SequencerDef>,
    _config: &DotConfig,
) -> String {
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
    for srv in &patchbay.servos {
        let auto_id = &srv.automaton_id;
        let target = format!("node_{}:{}", srv.target_node, srv.target_param);
        let label = format!("{:?} [{:.3}, {:.3}]", srv.mapping, srv.min, srv.max);
        writeln!(
            dot,
            "    auto_{auto_id} -> param_{target} [label=\"{label}\", style=dashed, color=\"#44a\"];"
        ).ok();
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

    // ── SequencerDef overlay ─────────────────────────────────
    if let Some(seq) = sequencer {
        writeln!(dot, "    subgraph cluster_sequencer {{").ok();
        writeln!(dot, "        label=\"Snapshot Sequencer\";").ok();
        writeln!(dot, "        style=filled;").ok();
        writeln!(dot, "        fillcolor=\"#fff0f0\";").ok();
        writeln!(dot, "        fontcolor=\"#f44\";").ok();

        for pat in &seq.patterns {
            // Build an HTML-like table for the pattern
            let pat_id = format!("seq_{}", pat.id);
            writeln!(dot, "        {pat_id} [label=<").ok();
            writeln!(
                dot,
                "            <table border=\"0\" cellborder=\"1\" cellspacing=\"0\">"
            )
            .ok();
            writeln!(
                dot,
                "            <tr><td colspan=\"2\"><b>{}</b></td></tr>",
                pat.id
            )
            .ok();
            writeln!(dot, "            <tr><td>Step</td><td>P-Locks</td></tr>").ok();
            for (si, step) in pat.steps.iter().enumerate() {
                let locks: Vec<String> = step
                    .parameters
                    .iter()
                    .map(|p| format!("{}:{:.2}", p.param_name, p.value))
                    .collect();
                let lock_str = locks.join(" ");
                writeln!(dot, "            <tr><td>{si}</td><td>{lock_str}</td></tr>").ok();
            }
            writeln!(dot, "            </table>").ok();
            writeln!(dot, "        >, shape=plaintext];").ok();
        }

        writeln!(dot, "    }}").ok();
    }

    writeln!(dot, "}}").ok();
    dot
}

/// Generate DOT for sequencer alone.
#[cfg(feature = "serde")]
pub fn sequencer_to_dot(seq: &SequencerDef) -> String {
    let cfg = DotConfig::default();
    let empty = PatchbayDef::new();
    patchbay_to_dot(&empty, Some(seq), &cfg)
}
