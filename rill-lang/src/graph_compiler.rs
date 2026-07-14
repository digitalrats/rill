//! Compile a GraphIr into a CompiledGraph for zero-allocation execution.

use std::collections::HashMap;

use rill_core::buffer::{Buffer, FixedBuffer};
use rill_core::math::Transcendental;
use rill_core::traits::{Algorithm, MultichannelAlgorithm};

use crate::builtin::{Registry, SignatureSource};
use crate::graph_ir::{EdgeKind, GraphIr};

/// Owned algorithm variants — SISO or MIMO.
pub enum AlgorithmVariant<T: Transcendental> {
    /// Single-input, single-output block built-in.
    Siso(Box<dyn crate::builtin::BlockBuiltin<T>>),
    /// Multi-input, multi-output block built-in.
    Mimo(Box<dyn rill_core::builtin::MultichannelBlockBuiltin<T>>),
}

/// One compiled graph node — owns its algorithm and buffer routing.
pub struct NodeClosure<T: Transcendental, const BUF_SIZE: usize> {
    pub(crate) algo: AlgorithmVariant<T>,
    input_indices: Vec<usize>,
    output_indices: Vec<usize>,
    /// Pre-allocated, cleared+re-filled each tick — zero allocation.
    input_slices: Vec<&'static [T]>,
    output_slices: Vec<&'static mut [T]>,
}

impl<T: Transcendental, const BUF_SIZE: usize> NodeClosure<T, BUF_SIZE> {
    /// Execute this node's algorithm, reading from and writing to the buffer pool.
    #[allow(unsafe_code)]
    pub fn execute(&mut self, buffers: &mut [FixedBuffer<T, BUF_SIZE>]) {
        match &mut self.algo {
            AlgorithmVariant::Siso(algo) => {
                let bufs_ptr = buffers.as_mut_ptr();
                let input = if self.input_indices.is_empty() {
                    None
                } else {
                    let in_buf = unsafe { &*bufs_ptr.add(self.input_indices[0]) };
                    Some(in_buf.as_slice())
                };
                let out_buf = unsafe { &mut *bufs_ptr.add(self.output_indices[0]) };
                Algorithm::process(algo.as_mut(), input, out_buf.as_mut_slice()).ok();
            }
            AlgorithmVariant::Mimo(algo) => {
                let bufs_ptr = buffers.as_mut_ptr();
                self.input_slices.clear();
                for &idx in &self.input_indices {
                    let buf = unsafe { &*bufs_ptr.add(idx) };
                    self.input_slices
                        .push(unsafe { std::mem::transmute::<&[T], &'static [T]>(buf.as_slice()) });
                }
                self.output_slices.clear();
                for &idx in &self.output_indices {
                    let buf = unsafe { &mut *bufs_ptr.add(idx) };
                    self.output_slices.push(unsafe {
                        std::mem::transmute::<&mut [T], &'static mut [T]>(buf.as_mut_slice())
                    });
                }
                MultichannelAlgorithm::process(
                    algo.as_mut(),
                    &self.input_slices,
                    &mut self.output_slices,
                )
                .ok();
            }
        }
    }

    /// Set a parameter by index on the owned algorithm.
    pub fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        match &mut self.algo {
            AlgorithmVariant::Siso(algo) => algo.set_param(index, value),
            AlgorithmVariant::Mimo(algo) => algo.set_param(index, value),
        }
    }

    /// Reset the owned algorithm to its initial state.
    pub fn reset(&mut self) {
        match &mut self.algo {
            AlgorithmVariant::Siso(algo) => Algorithm::reset(algo.as_mut()),
            AlgorithmVariant::Mimo(algo) => MultichannelAlgorithm::reset(algo.as_mut()),
        }
    }
}

/// A compiled graph ready for zero-allocation execution.
pub struct CompiledGraph<T: Transcendental, const BUF_SIZE: usize> {
    /// Pre-allocated buffer pool (fixed-size, stack-friendly).
    pub buffers: Vec<FixedBuffer<T, BUF_SIZE>>,
    /// Compiled node closures in topological order.
    pub nodes: Vec<NodeClosure<T, BUF_SIZE>>,
    /// Number of graph input channels.
    pub inputs: usize,
    /// Number of graph output channels.
    pub outputs: usize,
    /// Buffer indices mapping to graph output channels.
    pub output_mapping: Vec<usize>,
    /// Node names in topological order (for anchor-based param routing).
    pub node_names: Vec<String>,
}

/// Compile a GraphIr into a CompiledGraph.
pub fn compile<T: Transcendental, const BUF_SIZE: usize>(
    ir: &GraphIr,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<CompiledGraph<T, BUF_SIZE>, String> {
    // 1. Build edge buffer mapping (same zero-copy sharing as current graph_lower)
    let mut edge_buffers: HashMap<(String, usize, String, usize), usize> = HashMap::new();
    let mut buffer_counter: usize = ir.inputs;
    let mut output_bufs_per_node: Vec<Vec<usize>> = Vec::new();

    for name in &ir.topo_order {
        let node = ir.nodes.get(name).unwrap();
        let mut output_bufs = Vec::new();
        for _port in 0..node.arity.1 {
            let buf = buffer_counter;
            buffer_counter += 1;
            output_bufs.push(buf);
        }
        output_bufs_per_node.push(output_bufs.clone());

        for edge in &ir.edges {
            if edge.from_node == *name && edge.kind == EdgeKind::Signal {
                edge_buffers.insert(
                    (
                        edge.from_node.clone(),
                        edge.from_port,
                        edge.to_node.clone(),
                        edge.to_port,
                    ),
                    output_bufs[edge.from_port],
                );
            }
        }
    }

    let n_bufs = buffer_counter;

    // 2. Build NodeClosures
    let mut nodes: Vec<NodeClosure<T, BUF_SIZE>> = Vec::new();
    let mut node_names: Vec<String> = Vec::new();

    for (idx, name) in ir.topo_order.iter().enumerate() {
        let node = ir.nodes.get(name).unwrap();

        let mut input_bufs: Vec<usize> = Vec::new();
        for edge in &ir.edges {
            if edge.to_node == *name && edge.kind == EdgeKind::Signal {
                let key = (
                    edge.from_node.clone(),
                    edge.from_port,
                    edge.to_node.clone(),
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

        if input_bufs.is_empty() && idx < ir.inputs {
            for port in 0..node.arity.0.min(ir.inputs) {
                input_bufs.push(port);
            }
        }

        let output_bufs = output_bufs_per_node[idx].clone();
        let n_in = input_bufs.len();
        let n_out = output_bufs.len();

        let algo = if node.ir.builtins.is_empty() {
            let prog =
                crate::program::RillProgram::<T>::new_with(node.ir.clone(), registry, sample_rate)
                    .map_err(|e| format!("program creation: {e}"))?;
            AlgorithmVariant::Siso(Box::new(prog))
        } else {
            let bi = &node.ir.builtins[0];
            let _sig = registry
                .builtin_sig(&bi.name)
                .ok_or_else(|| format!("unknown builtin: {}", bi.name))?;

            let entry = registry.get(&bi.name).unwrap();

            if n_in <= 1 && n_out == 1 {
                let block = entry
                    .build_block(&bi.params, sample_rate)
                    .ok_or_else(|| format!("failed to build block: {}", bi.name))?;
                AlgorithmVariant::Siso(block)
            } else {
                let mimo = entry
                    .build_multichannel_block(&bi.params, sample_rate)
                    .ok_or_else(|| format!("failed to build multichannel block: {}", bi.name))?;
                AlgorithmVariant::Mimo(mimo)
            }
        };

        nodes.push(NodeClosure {
            algo,
            input_indices: input_bufs,
            output_indices: output_bufs,
            input_slices: Vec::with_capacity(n_in),
            output_slices: Vec::with_capacity(n_out),
        });
        node_names.push(name.clone());
    }

    // 3. Build output mapping (leaf nodes -> graph outputs)
    let mut output_mapping = Vec::new();
    for name in &ir.topo_order {
        let is_leaf = !ir
            .edges
            .iter()
            .any(|e| e.from_node == *name && e.kind == EdgeKind::Signal);
        if is_leaf {
            let idx = ir.topo_order.iter().position(|n| n == name).unwrap();
            for &buf in &output_bufs_per_node[idx] {
                output_mapping.push(buf);
            }
        }
    }

    let buffers = vec![FixedBuffer::<T, BUF_SIZE>::new(); n_bufs];

    Ok(CompiledGraph {
        buffers,
        nodes,
        inputs: ir.inputs,
        outputs: ir.outputs,
        output_mapping,
        node_names,
    })
}
