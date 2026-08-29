//! Compile a [`GraphSpec`] into a [`CompiledStream`]: one program (plain graph)
//! or two programs plus a tape backend (duplex).
//!
//! A plain graph (no passive backends) compiles into a single program. A graph
//! with passive tape-head backends partitions into a recording and a playback
//! subgraph; each sub-spec is reconstructed and compiled separately, and a
//! plain-data [`TapeSpec`] is derived from the graph's tape resource and the
//! passive write/read head attachments.

use rill_core::math::Transcendental;

use crate::builtin::Registry;
use crate::error::CompileError;
use crate::graph::partition::{partition, SubGraph};
use crate::graph::reconstruct::compile_spec;
use crate::graph::spec::{BackendKind, GraphResourceSpec, GraphSpec, GraphSpecEdge, GraphSpecNode};
use crate::graph::TapeSpec;
use crate::program_engine::ProgramEngine;

/// A compiled graph: one program (plain) or two plus a tape backend (duplex).
///
/// The unboxed shape is part of the planned `Runtime::launch_stream` contract
/// (it moves the engines into a single or duplex callback), so the size
/// difference between the variants is accepted.
#[allow(clippy::large_enum_variant)]
pub enum CompiledStream<T: Transcendental> {
    /// A plain graph — a single program engine.
    Single(ProgramEngine<T>),
    /// A tape-echo graph — a recording program, a playback program, and the
    /// tape backend specification connecting them.
    Duplex {
        /// The recording subgraph's program (capture → record + dry).
        recording: ProgramEngine<T>,
        /// The playback subgraph's program (taps + dry → fb + output).
        playback: ProgramEngine<T>,
        /// Tape backend spec derived from the graph's tape resource + heads.
        tape: TapeSpec,
    },
}

/// Compile a [`GraphSpec`] into a [`CompiledStream`].
///
/// Graphs without passive backends always compile to [`CompiledStream::Single`].
/// With passive backends the graph partitions into subgraphs; exactly two
/// subgraphs become a [`CompiledStream::Duplex`], while a single region (or a
/// graph with passive backends but no active pair) still compiles as one
/// program.
pub fn compile<T: Transcendental + 'static>(
    spec: &GraphSpec,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<CompiledStream<T>, CompileError> {
    let has_passive = spec.backends.iter().any(|b| b.kind == BackendKind::Passive);
    if !has_passive {
        return Ok(CompiledStream::Single(compile_spec(
            spec,
            registry,
            sample_rate,
        )?));
    }

    let parts = partition(spec);
    match parts.len() {
        0 | 1 => Ok(CompiledStream::Single(compile_spec(
            spec,
            registry,
            sample_rate,
        )?)),
        2 => {
            if overlapping(&parts, spec.nodes.len()) {
                return Err(CompileError::Unsupported(
                    "graph partition overlaps between subgraphs".to_string(),
                ));
            }
            let recording = compile_spec(&sub_spec(spec, &parts[0]), registry, sample_rate)?;
            let playback = compile_spec(&sub_spec(spec, &parts[1]), registry, sample_rate)?;
            let tape = tape_spec_from(spec);
            Ok(CompiledStream::Duplex {
                recording,
                playback,
                tape,
            })
        }
        _ => Err(CompileError::Unsupported(
            ">2 active subgraphs unsupported".to_string(),
        )),
    }
}

/// Whether any node is claimed by more than one subgraph.
fn overlapping(parts: &[SubGraph], n: usize) -> bool {
    let mut count = vec![0usize; n];
    for p in parts {
        for &u in &p.nodes {
            count[u] += 1;
        }
    }
    count.iter().any(|&c| c > 1)
}

/// Build a sub-`GraphSpec` from a region: the region's nodes (minus passive
/// backend boundary nodes, which are backends, not compiled builtins) with
/// their internal edges. Cross-region edges are dropped, so a cross-out source
/// node becomes an exposed sink output and a cross-in target port becomes a
/// free input wire.
fn sub_spec(spec: &GraphSpec, region: &SubGraph) -> GraphSpec {
    let passive: Vec<bool> = (0..spec.nodes.len())
        .map(|i| {
            spec.backends
                .iter()
                .any(|b| b.kind == BackendKind::Passive && b.node == i)
        })
        .collect();
    let mut remap = vec![None; spec.nodes.len()];
    let mut kept: Vec<usize> = Vec::new();
    for &u in &region.nodes {
        if passive[u] {
            continue;
        }
        remap[u] = Some(kept.len());
        kept.push(u);
    }
    let nodes: Vec<GraphSpecNode> = kept.iter().map(|&u| spec.nodes[u].clone()).collect();
    let edges: Vec<GraphSpecEdge> = spec
        .edges
        .iter()
        .filter_map(|e| match (remap[e.from], remap[e.to]) {
            (Some(from), Some(to)) => Some(GraphSpecEdge {
                from,
                from_port: e.from_port,
                to,
                to_port: e.to_port,
                kind: e.kind,
            }),
            _ => None,
        })
        .collect();
    GraphSpec {
        nodes,
        edges,
        resources: Vec::new(),
        sample_rate: spec.sample_rate,
        backends: Vec::new(),
    }
}

/// Derive the plain-data [`TapeSpec`] from the graph's tape resource and the
/// passive write/read head attachments.
fn tape_spec_from(spec: &GraphSpec) -> TapeSpec {
    let resource: Option<&GraphResourceSpec> = spec.resources.iter().find(|r| r.kind == "tape");
    let name = resource
        .map(|r| r.name.clone())
        .unwrap_or_else(|| "tape_0".to_string());
    let capacity = resource.map(|r| r.capacity).unwrap_or(1024);
    let mut write_feedback = 0.3;
    let mut read_delays: Vec<f64> = Vec::new();
    for b in &spec.backends {
        if b.kind != BackendKind::Passive {
            continue;
        }
        let node = &spec.nodes[b.node];
        // Head config is read from the attachment params, falling back to the
        // node's own parameter bag (where the graph frontend stores them).
        let param = |name: &str| {
            b.params
                .get(name)
                .or_else(|| node.params.get(name))
                .copied()
        };
        if node.type_name == "write_head" {
            write_feedback = param("feedback").unwrap_or(0.3);
        } else if node.type_name == "read_head" {
            read_delays.push(param("delay").unwrap_or(0.1));
        }
    }
    TapeSpec {
        name,
        capacity,
        write_feedback,
        read_delays,
    }
}
