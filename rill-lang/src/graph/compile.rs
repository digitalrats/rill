//! Compile a [`GraphSpec`] into a [`CompiledStream`]: one program (plain graph)
//! or two programs plus a shared tape backend (duplex).
//!
//! A plain graph (no passive backends) compiles into a single program. A graph
//! with passive tape-head backends partitions into a recording and a playback
//! subgraph; each sub-spec is reconstructed and compiled against a shared
//! [`ResourceRegistry`], and a plain-data [`TapeSpec`] is derived for the
//! runtime.

use rill_core::buffer::{ResourceRegistry, TapeLoop};
use rill_core::math::Transcendental;

use crate::builtin::Registry;
use crate::error::CompileError;
use crate::graph::partition::{partition, SubGraph};
use crate::graph::reconstruct::compile_spec;
use crate::graph::spec::{GraphSpec, GraphSpecEdge, GraphSpecNode, NodeBackendKind};
use crate::graph::TapeSpec;
use crate::program_engine::ProgramEngine;

/// A compiled graph: one program (plain) or two plus a shared tape (duplex).
#[allow(clippy::large_enum_variant)]
pub enum CompiledStream<T: Transcendental, const BUF: usize> {
    /// A plain graph — a single program engine.
    Single(ProgramEngine<T, BUF>),
    /// A tape-echo graph — a recording program, a playback program, the shared
    /// tape registry, and the tape backend specification.
    Duplex {
        /// The recording subgraph's program (`[capture, fb] → [dryL, dryR]`).
        recording: ProgramEngine<T, BUF>,
        /// The playback subgraph's program (`[dryL, dryR] → [fb, out]`).
        playback: ProgramEngine<T, BUF>,
        /// Shared tape registry used by both engines (one buffer per channel).
        resources: ResourceRegistry<T>,
        /// Tape backend spec derived from the graph's tape resource + heads.
        tape: TapeSpec,
    },
}

/// Compile a [`GraphSpec`] into a [`CompiledStream`].
///
/// Graphs without passive backends always compile to [`CompiledStream::Single`].
/// With passive backends the graph partitions into subgraphs; exactly two
/// subgraphs become a [`CompiledStream::Duplex`].
pub fn compile<T: Transcendental + 'static, const BUF: usize>(
    spec: &GraphSpec,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<CompiledStream<T, BUF>, CompileError> {
    let has_passive = spec
        .nodes
        .iter()
        .any(|n| n.backend == Some(NodeBackendKind::Passive));
    if !has_passive {
        return Ok(CompiledStream::Single(compile_spec::<T, BUF>(
            spec,
            registry,
            sample_rate,
        )?));
    }

    let parts = partition(spec);
    match parts.len() {
        0 | 1 => Ok(CompiledStream::Single(compile_spec::<T, BUF>(
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
            let mut resources = ResourceRegistry::<T>::new();
            let cap = spec
                .resources
                .iter()
                .find(|r| r.kind == "tape")
                .map(|r| r.capacity)
                .unwrap_or(1024);
            resources.register_buffer(
                "tape_0",
                Box::new(TapeLoop::<T>::new(cap).ok_or_else(|| {
                    CompileError::Unsupported("tape capacity must be > 0".into())
                })?),
            );
            let recording =
                compile_sub::<T, BUF>(spec, &parts[0], registry, sample_rate, &mut resources)?;
            let playback =
                compile_sub::<T, BUF>(spec, &parts[1], registry, sample_rate, &mut resources)?;
            let tape = tape_spec_from(spec);
            Ok(CompiledStream::Duplex {
                recording,
                playback,
                resources,
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

/// Compile one subgraph against the shared registry.
///
/// The recording subgraph uses a deterministic template (one mixer split into
/// the write head and the dry outputs) so the capture input is not duplicated
/// by fan-out reconstruction. The playback subgraph keeps ALL region nodes —
/// including the passive head builtins (`read_head`) — compiled as ordinary
/// builtins referencing the shared tape.
fn compile_sub<T: Transcendental + 'static, const BUF: usize>(
    spec: &GraphSpec,
    region: &SubGraph,
    registry: &Registry<T>,
    sample_rate: f32,
    resources: &mut ResourceRegistry<T>,
) -> Result<ProgramEngine<T, BUF>, CompileError> {
    let sub = sub_spec(spec, region);
    let program = if is_recording(spec, region) {
        let src = render_recording(spec, region, registry);
        crate::parser::parse(&crate::lexer::tokenize(&src)?, src.as_bytes())?
    } else {
        crate::graph::reconstruct::reconstruct(&sub, registry)?
    };
    crate::compile_program_with_resources::<T, BUF>(&program, registry, sample_rate, resources)
}

/// Whether the region is the recording side (contains the write head).
fn is_recording(spec: &GraphSpec, region: &SubGraph) -> bool {
    region
        .nodes
        .iter()
        .any(|&u| spec.nodes[u].type_name == "write_head")
}

/// Render the recording program: inputs `[capture, fb]`, outputs
/// `[record, dryL, dryR]` — one mixer split into the write head and the dry
/// outputs (no stateful fan-out duplication).
fn render_recording<T: Transcendental>(
    spec: &GraphSpec,
    region: &SubGraph,
    registry: &Registry<T>,
) -> String {
    let wh = region
        .nodes
        .iter()
        .find(|&&u| spec.nodes[u].type_name == "write_head")
        .map(|&u| &spec.nodes[u]);
    let (dt, fb) = match wh {
        Some(n) => (
            n.params.get("delay_time").copied().unwrap_or(0.5),
            n.params.get("feedback").copied().unwrap_or(0.3),
        ),
        None => (0.5, 0.3),
    };
    let cap = spec
        .resources
        .iter()
        .find(|r| r.kind == "tape")
        .map(|r| r.capacity)
        .unwrap_or(96000);
    let _ = registry;
    format!(
        "tape_0 = TapeLoop {cap}\nmain = (_ :> mixer {{ buses: 0, master_vol: 1.0 }}) <: ( ( _ <: ( ((_ , _) :> write_head tape_0 {dt} {fb}) , _ ) ) , _ )"
    )
}

/// Build a sub-`GraphSpec` from a region: the region's nodes (including the
/// passive head builtins) with their internal edges. Cross-region edges are
/// dropped; a cross-out channel becomes an exposed `boundary_out` output ONLY
/// when its source node still has an internal consumer (a pure sink's channel
/// is already a program output); a cross-in target port becomes a free input
/// wire.
fn sub_spec(spec: &GraphSpec, region: &SubGraph) -> GraphSpec {
    let mut remap = vec![None; spec.nodes.len()];
    let mut kept: Vec<usize> = Vec::new();
    for &u in &region.nodes {
        remap[u] = Some(kept.len());
        kept.push(u);
    }
    let nodes: Vec<GraphSpecNode> = kept.iter().map(|&u| spec.nodes[u].clone()).collect();
    let mut edges: Vec<GraphSpecEdge> = Vec::new();
    let mut boundary_out: Vec<(usize, usize)> = Vec::new();
    let mut input_ports: Vec<(usize, usize)> = Vec::new();
    for e in &spec.edges {
        match (remap[e.from], remap[e.to]) {
            (Some(from), Some(to)) => edges.push(GraphSpecEdge {
                from,
                from_port: e.from_port,
                to,
                to_port: e.to_port,
                kind: e.kind,
            }),
            // Edge crossing OUT of this region.
            (Some(_), None) => {
                let from = remap[e.from].unwrap();
                if !boundary_out.contains(&(from, e.from_port)) {
                    boundary_out.push((from, e.from_port));
                }
            }
            // Edge crossing INTO this region: the target port becomes a free
            // input wire.
            (None, Some(_)) => {
                let to = remap[e.to].unwrap();
                if !input_ports.contains(&(to, e.to_port)) {
                    input_ports.push((to, e.to_port));
                }
            }
            _ => {}
        }
    }
    // Active input backends (capture) attach at a free port: expose it as an
    // input port.
    for b in &spec.backends {
        if !b.input {
            if let Some(idx) = remap[b.node] {
                if !input_ports.contains(&(idx, b.port)) {
                    input_ports.push((idx, b.port));
                }
            }
        }
    }
    // A cross-out channel whose source node has NO internal consumers is
    // already a program output (the sink's output); drop the redundant entry.
    let has_internal_out: Vec<bool> = (0..nodes.len())
        .map(|u| edges.iter().any(|e| e.from == u))
        .collect();
    boundary_out.retain(|&(node, _)| has_internal_out[node]);
    GraphSpec {
        nodes,
        edges,
        resources: Vec::new(),
        sample_rate: spec.sample_rate,
        backends: Vec::new(),
        boundary_out,
        input_ports,
    }
}

/// Derive the plain-data [`TapeSpec`] from the graph's tape resource and the
/// passive write/read head nodes.
fn tape_spec_from(spec: &GraphSpec) -> TapeSpec {
    let resource = spec.resources.iter().find(|r| r.kind == "tape");
    let name = resource
        .map(|r| r.name.clone())
        .unwrap_or_else(|| "tape_0".to_string());
    let capacity = resource.map(|r| r.capacity).unwrap_or(1024);
    let mut write_feedback = 0.3;
    let mut read_delays: Vec<f64> = Vec::new();
    for node in &spec.nodes {
        if node.backend != Some(NodeBackendKind::Passive) {
            continue;
        }
        if node.type_name == "write_head" {
            write_feedback = node.params.get("feedback").copied().unwrap_or(0.3);
        } else if node.type_name == "read_head" {
            read_delays.push(node.params.get("delay").copied().unwrap_or(0.1));
        }
    }
    TapeSpec {
        name,
        capacity,
        write_feedback,
        read_delays,
    }
}
