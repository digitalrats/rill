# MonoToStereo: Multi-Output Builtins & Closure-Based Graph Compilation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `MultichannelBlockBuiltin` infrastructure, compile graphs into zero-allocation closures, and implement a `MonoToStereo` node with constant-power panning.

**Architecture:** Replace the step-based `ScheduledGraph` execution model with a flat `Vec<NodeClosure>` — each closure owns its algorithm and captures buffer indices. Execution is a single loop: `for node in &mut nodes { node.execute(&mut buffers); }` — zero heap allocation on RT path. `rill-graph` is unchanged (produces `GraphIr`), `graph_compiler.rs` compiles `GraphIr` → `CompiledGraph`, `graph_engine.rs` is rewritten to execute it.

**Tech Stack:** Rust, `rill-core`, `rill-lang`, `rill-router`, `rill-graph`. Uses `unsafe` in `NodeClosure::execute()` (slice construction from `FixedBuffer` raw pointers) — same pattern as current `buf_slices()`. Crate `rill-lang` has `#![deny(unsafe_code)]` — requires `#[allow(unsafe_code)]` on the function.

---

### Task 1: Add `MultichannelBlockBuiltin` trait to rill-core

**Files:**
- Modify: `rill-core/src/builtin.rs`
- Verify: `rill-core/src/lib.rs` (re-exports)
- Test: `cargo test -p rill-core`

- [ ] **Step 1: Add `MultichannelBlockBuiltin` trait and `BuiltinInst::MultichannelBlock` variant**

Add after the existing `BlockBuiltin` trait (after line 28):

```rust
/// A whole-buffer multi-channel built-in with settable params.
pub trait MultichannelBlockBuiltin<T: Transcendental>:
    crate::traits::MultichannelAlgorithm<T> + Send + Sync
{
    /// Set a parameter by index.
    fn set_param(&mut self, _index: usize, _value: &ParamValue) {}
}
```

Replace the `BuiltinInst` enum at the usage site. First, find where `BuiltinInst` is currently defined. Search: `rill-lang/src/program.rs` for `enum BuiltinInst`.

Actually, `BuiltinInst` is in `rill-lang/src/program.rs`, not `rill-core/src/builtin.rs`. Let me check...

Wait, I need to check where `BuiltinInst` is defined. Looking at the exploration data: `program.rs:line 34` — `pub(crate) block_regs: Vec<Vec<T>>`, but the enum is... let me search.

OK, let me just add the trait in `rill-core/src/builtin.rs` and the variant in `rill-lang/src/program.rs`.

Add in `rill-core/src/builtin.rs`, after `BlockBuiltin`:

```rust
/// A whole-buffer multi-channel built-in with settable params.
pub trait MultichannelBlockBuiltin<T: Transcendental>:
    crate::traits::MultichannelAlgorithm<T> + Send + Sync
{
    /// Set a parameter by index.
    fn set_param(&mut self, _index: usize, _value: &ParamValue) {}
}
```

Insert at line 28, after the `BlockBuiltin` trait block.

- [ ] **Step 2: Add `MultichannelBlock` variant to `BuiltinInst` in rill-lang**

In `rill-lang/src/program.rs`, find `enum BuiltinInst` and add the new variant:

```rust
pub(crate) enum BuiltinInst<T: Transcendental> {
    Sample(Box<dyn SampleBuiltin<T>>),
    Block(Box<dyn BlockBuiltin<T>>),
    MultichannelBlock(Box<dyn MultichannelBlockBuiltin<T>>),
}
```

Add the import at the top of `program.rs`:
```rust
use rill_core::builtin::MultichannelBlockBuiltin;
```

- [ ] **Step 3: Add `MultichannelBlockFactory` type and `register_multichannel_block`**

In `rill-core/src/builtin.rs`, add the factory type alias near the existing ones (after line 181):

```rust
type MultichannelBlockFactory<T> = Box<dyn Fn(&[f64], f32) -> Box<dyn MultichannelBlockBuiltin<T>> + Send + Sync>;
```

Add `MultichannelBlock` variant to the internal `Factory` enum:
```rust
enum Factory<T: Transcendental> {
    Sample(SampleFactory<T>),
    Block(BlockFactory<T>),
    MultichannelBlock(MultichannelBlockFactory<T>),
}
```

Add `build_multichannel_block` to `Entry<T>`:
```rust
pub fn build_multichannel_block(
    &self,
    params: &[f64],
    sample_rate: f32,
) -> Option<Box<dyn MultichannelBlockBuiltin<T>>> {
    match &self.factory {
        Factory::MultichannelBlock(f) => Some(f(params, sample_rate)),
        _ => None,
    }
}
```

Add `register_multichannel_block` to `Registry<T>`:
```rust
pub fn register_multichannel_block(
    &mut self,
    sig: BuiltinSig,
    factory: impl Fn(&[f64], f32) -> Box<dyn MultichannelBlockBuiltin<T>> + Send + Sync + 'static,
) {
    debug_assert_eq!(sig.kind, BuiltinKind::Block);
    self.entries.insert(
        sig.name.to_string(),
        Entry {
            sig,
            factory: Factory::MultichannelBlock(Box::new(factory)),
        },
    );
}
```

- [ ] **Step 4: Re-export in rill-core lib.rs**

Add to `rill-core/src/lib.rs` re-exports (near existing `pub use traits::*` or in a dedicated re-export block):
```rust
pub use builtin::MultichannelBlockBuiltin;
```

Check existing re-export pattern in `lib.rs` and match it.

- [ ] **Step 5: Build and fix compilation errors**

```bash
cargo check -p rill-core 2>&1
```
Expected: success, no errors.

- [ ] **Step 6: Check rill-lang compiles with the new variant**

```bash
cargo check -p rill-lang 2>&1
```
Fix any match exhaustiveness errors from the new `BuiltinInst` variant. Add `_ => {}` or appropriate handling in match arms that don't need to handle `MultichannelBlock`.

- [ ] **Step 7: Commit**

```bash
git add rill-core/src/builtin.rs rill-core/src/lib.rs rill-lang/src/program.rs
git commit -m "feat(rill-core): add MultichannelBlockBuiltin trait and registry support"
```

---

### Task 2: Implement `MonoToStereo` node in rill-router

**Files:**
- Create: `rill-router/src/pan.rs`
- Modify: `rill-router/src/lib.rs`
- Modify: `rill-router/src/lang.rs`
- Modify: `rill-router/src/register.rs`
- Test: `cargo test -p rill-router`

- [ ] **Step 1: Create `rill-router/src/pan.rs`**

```rust
use rill_core::math::Transcendental;
use rill_core::traits::{MultichannelAlgorithm, ProcessResult};

/// Pan law — determines gain distribution between left and right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanLaw {
    /// √2/2 * (cos(θ) + sin(θ)) per channel, constant perceived loudness.
    ConstantPower,
    /// Linear fade: left = 1-pan, right = pan. -3 dB dip at center.
    Linear,
}

impl PanLaw {
    /// Compute (left_gain, right_gain) for pan in [-1.0, 1.0].
    /// -1.0 = full left, 0.0 = center, 1.0 = full right.
    pub fn gains(self, pan: f32) -> (f32, f32) {
        let pan = pan.clamp(-1.0, 1.0);
        match self {
            PanLaw::ConstantPower => {
                let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                (angle.cos(), angle.sin())
            }
            PanLaw::Linear => {
                let right = (pan + 1.0) * 0.5;
                (1.0 - right, right)
            }
        }
    }
}

/// Converts a mono signal to stereo with pan and exponential smoothing.
pub struct MonoToStereo<T: Transcendental> {
    pan_law: PanLaw,
    pan: f32,
    smoothing: f32,
    left_gain: f32,
    right_gain: f32,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Transcendental> MonoToStereo<T> {
    /// Create a new MonoToStereo converter.
    pub fn new(pan_law: PanLaw, pan: f32, smoothing: f32) -> Self {
        let (lg, rg) = pan_law.gains(pan);
        Self {
            pan_law,
            pan,
            smoothing,
            left_gain: lg,
            right_gain: rg,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Set pan position: -1.0 (full left) to 1.0 (full right).
    pub fn set_pan(&mut self, pan: f32) { self.pan = pan.clamp(-1.0, 1.0); }

    /// Set smoothing coefficient for exponential moving average.
    /// 0.0 = instant, 1.0 = no change. Typical: 0.05–0.2.
    pub fn set_smoothing(&mut self, s: f32) { self.smoothing = s.clamp(0.0, 1.0); }
}

impl<T: Transcendental> Default for MonoToStereo<T> {
    fn default() -> Self {
        Self::new(PanLaw::ConstantPower, 0.0, 0.1)
    }
}

impl<T: Transcendental> MultichannelAlgorithm<T> for MonoToStereo<T> {
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 2 }

    fn process(
        &mut self,
        inputs: &[&[T]],
        outputs: &mut [&mut [T]],
    ) -> ProcessResult<()> {
        let mono = inputs[0];
        let left = outputs[0];
        let right = outputs[1];

        let (tg_l, tg_r) = self.pan_law.gains(self.pan);
        self.left_gain += self.smoothing * (tg_l - self.left_gain);
        self.right_gain += self.smoothing * (tg_r - self.right_gain);

        let lg = T::from_f32(self.left_gain);
        let rg = T::from_f32(self.right_gain);

        for i in 0..mono.len() {
            let s = mono[i];
            left[i] = s * lg;
            right[i] = s * rg;
        }
        Ok(())
    }

    fn reset(&mut self) {
        let (lg, rg) = self.pan_law.gains(self.pan);
        self.left_gain = lg;
        self.right_gain = rg;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_power_center_equals() {
        let mut ms = MonoToStereo::<f32>::new(PanLaw::ConstantPower, 0.0, 0.0);
        let input = [1.0f32; 64];
        let mut left = [0.0f32; 64];
        let mut right = [0.0f32; 64];
        ms.process(&[&input[..]], &mut [&mut left[..], &mut right[..]]).unwrap();
        for i in 0..64 {
            assert!((left[i] - right[i]).abs() < 1e-6);
        }
    }

    #[test]
    fn pan_hard_left_silences_right() {
        let mut ms = MonoToStereo::<f32>::new(PanLaw::ConstantPower, -1.0, 0.0);
        let input = [1.0f32; 64];
        let mut left = [0.0f32; 64];
        let mut right = [0.0f32; 64];
        ms.process(&[&input[..]], &mut [&mut left[..], &mut right[..]]).unwrap();
        assert!(left.iter().any(|&v| v > 0.0));
        assert!(right.iter().all(|&v| v < 1e-6));
    }

    #[test]
    fn smoothing_ramps_to_target() {
        let mut ms = MonoToStereo::<f32>::new(PanLaw::ConstantPower, 1.0, 0.1);
        // Force left_gain to zero, right_gain to 1.0 by calling reset without pan change
        // Actually reset sets to current pan. Set pan to center first.
        let input = [1.0f32; 64];
        let mut left = [0.0f32; 64];
        let mut right = [0.0f32; 64];

        // First block: at pan=1.0, right_gain starts at target
        ms.process(&[&input[..]], &mut [&mut left[..], &mut right[..]]).unwrap();

        // Change pan to -1.0 — gains should start moving
        ms.set_pan(-1.0);
        ms.process(&[&input[..]], &mut [&mut left[..], &mut right[..]]).unwrap();
        // left_gain should have increased from 0.0, right_gain decreased
        assert!(left[0] > 0.0, "left channel should have non-zero signal after pan change");
    }
}
```

- [ ] **Step 2: Run tests to verify MonoToStereo**

```bash
cargo test -p rill-router -- pan
```
Expected: tests pass.

- [ ] **Step 3: Register MonoToStereo as a lang builtin**

In `rill-router/src/lang.rs`, add the new `GraphicEqBuiltin`-style registration wrapper and register the builtin.

Add imports at the top:
```rust
use rill_core::builtin::MultichannelBlockBuiltin;
use crate::pan::{MonoToStereo, PanLaw};
use rill_core::traits::MultichannelAlgorithm;
```

Since `MonoToStereo` already implements `MultichannelAlgorithm`, we just need to implement `MultichannelBlockBuiltin` and register it. The `MultichannelBlockBuiltin` is auto-implemented by the trait (default `set_param`). But we need param routing. Add a wrapper:

```rust
struct MonoToStereoBuiltin<T: Transcendental> {
    inner: MonoToStereo<T>,
}

impl<T: Transcendental> MultichannelAlgorithm<T> for MonoToStereoBuiltin<T> {
    fn num_inputs(&self) -> usize { self.inner.num_inputs() }
    fn num_outputs(&self) -> usize { self.inner.num_outputs() }
    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.inner.process(inputs, outputs)
    }
    fn reset(&mut self) { self.inner.reset(); }
}

impl<T: Transcendental> MultichannelBlockBuiltin<T> for MonoToStereoBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        match index {
            0 => {
                if let Some(v) = value.as_f32() {
                    self.inner.set_pan(v);
                }
            }
            1 => {
                if let Some(v) = value.as_f32() {
                    self.inner.set_smoothing(v);
                }
            }
            _ => {}
        }
    }
}
```

Register in `register_router_builtins`:
```rust
reg.register_multichannel_block(
    BuiltinSig {
        name: "mono_to_stereo",
        params: vec![
            ParamType::Signal,
            ParamType::Float,
            ParamType::Float,
        ],
        signal_outs: 2,
        kind: BuiltinKind::Block,
        param_names: vec!["input", "pan", "smoothing"],
    },
    |params, _sr| {
        Box::new(MonoToStereoBuiltin::<T> {
            inner: MonoToStereo::new(
                PanLaw::ConstantPower,
                params[0] as f32,
                params[1] as f32,
            ),
        })
    },
);
```

- [ ] **Step 4: Export the pan module**

In `rill-router/src/lib.rs`, add:
```rust
pub mod pan;
```

Add to the public re-exports:
```rust
pub use pan::{MonoToStereo, PanLaw};
```

- [ ] **Step 5: Add graph name mapping**

In `rill-graph/src/graph.rs`, in the `build_ir` function, add the name mapping in the existing `mapped` match block (after line 258):
```rust
"rill/mono_to_stereo" => "mono_to_stereo",
```

- [ ] **Step 6: Build and check**

```bash
cargo check -p rill-router --features lang 2>&1
cargo check -p rill-graph 2>&1
```
Expected: success, no errors.

- [ ] **Step 7: Commit**

```bash
git add rill-router/src/pan.rs rill-router/src/lib.rs rill-router/src/lang.rs rill-graph/src/graph.rs
git commit -m "feat(rill-router): add MonoToStereo node with constant-power panning"
```

---

### Task 3: Create `CompiledGraph` types and `graph_compiler` in rill-lang

**Files:**
- Create: `rill-lang/src/graph_compiler.rs`
- Modify: `rill-lang/src/lib.rs`

- [ ] **Step 1: Add `graph_compiler` module declaration**

In `rill-lang/src/lib.rs`, add after the existing module declarations:
```rust
pub mod graph_compiler;
```

- [ ] **Step 2: Create `graph_compiler.rs`**

```rust
//! Compile a GraphIr into a CompiledGraph for zero-allocation execution.

use std::collections::HashMap;

use rill_core::buffer::FixedBuffer;
use rill_core::math::Transcendental;
use rill_core::traits::{Algorithm, MultichannelAlgorithm};

use crate::builtin::Registry;
use crate::graph_ir::{EdgeKind, GraphIr};

/// Owned algorithm variants.
pub enum AlgorithmVariant<T: Transcendental> {
    /// Single-input, single-output block builtin.
    Siso(Box<dyn crate::builtin::BlockBuiltin<T>>),
    /// Multi-input/output block builtin.
    Mimo(Box<dyn rill_core::builtin::MultichannelBlockBuiltin<T>>),
}

/// One compiled graph node — owns its algorithm and buffer routing.
pub struct NodeClosure<T: Transcendental, const BUF_SIZE: usize> {
    algo: AlgorithmVariant<T>,
    input_indices: Vec<usize>,
    output_indices: Vec<usize>,
    /// Pre-allocated, cleared+re-filled each tick — zero allocation.
    input_slices: Vec<&'static [T]>,
    output_slices: Vec<&'static mut [T]>,
}

impl<T: Transcendental, const BUF_SIZE: usize> NodeClosure<T, BUF_SIZE> {
    #[allow(unsafe_code)]
    pub fn execute(&mut self, buffers: &mut [FixedBuffer<T, BUF_SIZE>]) {
        match &mut self.algo {
            AlgorithmVariant::Siso(algo) => {
                let input = buffers[self.input_indices[0]].as_slice();
                let output = buffers[self.output_indices[0]].as_mut_slice();
                Algorithm::process(algo.as_mut(), Some(input), output).ok();
            }
            AlgorithmVariant::Mimo(algo) => {
                // Rebuild slice vecs from buffer base pointers — no allocation
                self.input_slices.clear();
                for &idx in &self.input_indices {
                    let ptr = buffers[idx].as_ptr();
                    let slice: &[T] =
                        unsafe { std::slice::from_raw_parts(ptr, BUF_SIZE) };
                    // SAFETY: FixedBuffer storage does not move during processing.
                    // Same unsafe assumption as current buf_slices().
                    self.input_slices
                        .push(unsafe { std::mem::transmute::<&[T], &'static [T]>(slice) });
                }
                self.output_slices.clear();
                for &idx in &self.output_indices {
                    let ptr = buffers[idx].as_mut_ptr();
                    let slice: &mut [T] =
                        unsafe { std::slice::from_raw_parts_mut(ptr, BUF_SIZE) };
                    self.output_slices
                        .push(unsafe { std::mem::transmute::<&mut [T], &'static mut [T]>(slice) });
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

    pub fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        match &mut self.algo {
            AlgorithmVariant::Siso(algo) => algo.set_param(index, value),
            AlgorithmVariant::Mimo(algo) => algo.set_param(index, value),
        }
    }
}

/// A compiled graph ready for zero-allocation execution.
pub struct CompiledGraph<T: Transcendental, const BUF_SIZE: usize> {
    pub buffers: Vec<FixedBuffer<T, BUF_SIZE>>,
    pub nodes: Vec<NodeClosure<T, BUF_SIZE>>,
    pub inputs: usize,
    pub outputs: usize,
    pub output_mapping: Vec<usize>,
    pub node_names: Vec<String>,
}

/// Compile a GraphIr into a CompiledGraph.
pub fn compile<T: Transcendental, const BUF_SIZE: usize>(
    ir: &GraphIr,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<CompiledGraph<T, BUF_SIZE>, String> {
    let n_nodes = ir.nodes.len();

    // 1. Map node names to topo positions
    let pos: HashMap<&String, usize> = ir
        .topo_order
        .iter()
        .enumerate()
        .map(|(i, n)| (n, i))
        .collect();

    // 2. Build edge buffer mapping (same zero-copy sharing as current graph_lower)
    let mut edge_buffers: HashMap<(&String, usize, &String, usize), usize> = HashMap::new();
    let mut buffer_counter: usize = ir.inputs; // reserve 0..inputs for graph inputs
    let mut output_bufs_per_node: Vec<Vec<usize>> = Vec::with_capacity(n_nodes);

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
            if &edge.from_node == name && edge.kind == EdgeKind::Signal {
                let key = (&edge.from_node, edge.from_port, &edge.to_node, edge.to_port);
                edge_buffers.insert(key, output_bufs[edge.from_port]);
            }
        }
    }

    let n_bufs = buffer_counter;

    // 3. Build NodeClosures
    let mut nodes: Vec<NodeClosure<T, BUF_SIZE>> = Vec::with_capacity(n_nodes);
    let mut node_names: Vec<String> = Vec::new();

    for (idx, name) in ir.topo_order.iter().enumerate() {
        let node = ir.nodes.get(name).unwrap();
        let arity = node.arity;

        // Resolve input buffer indices from edges
        let mut input_bufs: Vec<usize> = Vec::new();
        for edge in &ir.edges {
            if &edge.to_node == name && edge.kind == EdgeKind::Signal {
                let key = (&edge.from_node, edge.from_port, &edge.to_node, edge.to_port);
                if let Some(&buf) = edge_buffers.get(&key) {
                    if input_bufs.len() <= edge.to_port {
                        input_bufs.resize(edge.to_port + 1, 0);
                    }
                    input_bufs[edge.to_port] = buf;
                }
            }
        }

        // Look up builtin signature
        let sig = registry
            .builtin_sig(&node.builtins[0].name)
            .ok_or_else(|| format!("unknown builtin: {}", node.builtins[0].name))?;

        // Instantiate algorithm
        let entry = registry.get(&node.builtins[0].name).unwrap();
        let output_bufs = output_bufs_per_node[idx].clone();
        let n_in = input_bufs.len();
        let n_out = output_bufs.len();

        let algo = if n_in <= 1 && n_out == 1 {
            let block = entry
                .build_block(&node.builtins[0].params, sample_rate)
                .ok_or_else(|| format!("failed to build block: {}", node.builtins[0].name))?;
            AlgorithmVariant::Siso(block)
        } else {
            let mimo = entry
                .build_multichannel_block(&node.builtins[0].params, sample_rate)
                .ok_or_else(|| format!("failed to build multichannel block: {}", node.builtins[0].name))?;
            AlgorithmVariant::Mimo(mimo)
        };

        let input_slices = Vec::with_capacity(n_in);
        let output_slices = Vec::with_capacity(n_out);

        nodes.push(NodeClosure {
            algo,
            input_indices: input_bufs,
            output_indices: output_bufs,
            input_slices,
            output_slices,
        });
        node_names.push(name.clone());
    }

    // 4. Build output mapping (leaf nodes → graph outputs)
    let mut output_mapping = Vec::new();
    for name in &ir.topo_order {
        let is_leaf = !ir.edges.iter().any(|e| &e.from_node == name && e.kind == EdgeKind::Signal);
        if is_leaf {
            let idx = pos[name];
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
```

- [ ] **Step 3: Build and fix compilation errors**

```bash
cargo check -p rill-lang 2>&1
```
Expected: compile errors about `as_ptr`/`as_mut_ptr` not existing on `FixedBuffer`. Let me check the actual API...

Check `rill-core/src/buffer/buffer_trait.rs` for `FixedBuffer` methods. Read the full file for `as_ptr`/`as_mut_ptr` methods. If they don't exist, add them or use `as_slice().as_ptr()`.

Fix: use `buffers[idx].as_slice().as_ptr()` and `buffers[idx].as_mut_slice().as_mut_ptr()` instead.

Run: `cargo check -p rill-lang 2>&1` and fix any remaining errors. Expected: success.

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/graph_compiler.rs rill-lang/src/lib.rs
git commit -m "feat(rill-lang): add graph_compiler with closure-based compilation"
```

---

### Task 4: Rewrite graph_engine.rs with CompiledGraph

**Files:**
- Modify: `rill-lang/src/graph_engine.rs`
- Modify: `rill-lang/src/lib.rs`
- Modify: `rill-lang/src/program_runner.rs`

- [ ] **Step 1: Write the new `graph_engine.rs`**

Replace the entire content of `rill-lang/src/graph_engine.rs`. The new engine runs `CompiledGraph` with zero allocation:

```rust
//! Execution engine for CompiledGraph with a FixedBuffer pool.
//!
//! Runs a flat vector of NodeClosures in topological order. Parameters are
//! routed from actor mailbox commands to the correct node within the graph.
//! Zero heap allocation on the real-time signal path.

use std::collections::HashMap;
use std::sync::Arc;

use rill_core::buffer::FixedBuffer;
use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::traits::bridge::BridgeAlgorithm;
#[cfg(feature = "router")]
use rill_core::traits::MultichannelAlgorithm;
use rill_core::traits::{Algorithm, ParamValue, ProcessResult};
use rill_core_actor::{ActorRef, Mailbox};

use crate::graph_compiler::{CompiledGraph, NodeClosure};

#[cfg(feature = "debug")]
use crate::debug::{CmdStr, CommandFrame, DebugControl, ProbeSlot};
#[cfg(feature = "debug")]
use rill_core::queues::spsc::SpscQueue;
#[cfg(feature = "debug")]
use std::sync::atomic::Ordering;

/// Map from parameter name to its index in the node's parameter list.
pub type ParamMap = HashMap<String, usize>;

/// A deferred parameter update.
struct PendingParam {
    node_idx: usize,
    param_idx: usize,
    value: ParamValue,
}

/// Graph execution engine running a CompiledGraph.
pub struct CompiledGraphEngine<T: Transcendental, const BUF_SIZE: usize> {
    graph: CompiledGraph<T, BUF_SIZE>,
    pending: Vec<PendingParam>,
    param_maps: Vec<HashMap<String, usize>>,
    anchor_map: HashMap<String, usize>,
    mailbox: Arc<Mailbox<CommandEnum>>,
    actor_ref: ActorRef<CommandEnum>,
    #[cfg(feature = "debug")]
    pub(crate) probe_slots: Vec<std::sync::Arc<ProbeSlot>>,
    #[cfg(feature = "debug")]
    pub(crate) command_queue: std::sync::Arc<SpscQueue<CommandFrame, 256>>,
    #[cfg(feature = "debug")]
    pub(crate) debug_control: DebugControl,
}

impl<T: Transcendental, const BUF_SIZE: usize> CompiledGraphEngine<T, BUF_SIZE> {
    pub fn new(
        graph: CompiledGraph<T, BUF_SIZE>,
        mailbox: Arc<Mailbox<CommandEnum>>,
    ) -> Self {
        let param_maps: Vec<HashMap<String, usize>> = vec![HashMap::new(); graph.nodes.len()];
        let anchor_map: HashMap<String, usize> = graph
            .node_names
            .iter()
            .enumerate()
            .map(|(i, name)| (name.clone(), i))
            .collect();
        let actor_ref = mailbox.actor_ref();

        Self {
            graph,
            pending: Vec::new(),
            param_maps,
            anchor_map,
            mailbox,
            actor_ref,
            #[cfg(feature = "debug")]
            probe_slots: Vec::new(),
            #[cfg(feature = "debug")]
            command_queue: std::sync::Arc::new(SpscQueue::new()),
            #[cfg(feature = "debug")]
            debug_control: DebugControl::new(),
        }
    }

    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.actor_ref.clone()
    }

    fn drain_mailbox(&mut self) {
        while let Some(cmd) = self.mailbox.pop() {
            if let CommandEnum::SetParameter(ref sp) = cmd {
                let param_name = sp.parameter.as_str();
                if !sp.anchor.is_empty() {
                    if let Some(&node_idx) = self.anchor_map.get(&sp.anchor) {
                        if let Some(&idx) = self.param_maps[node_idx].get(param_name) {
                            self.pending.push(PendingParam {
                                node_idx,
                                param_idx: idx,
                                value: sp.value.clone(),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Run one tick. Zero heap allocation.
    pub fn process_tick(
        &mut self,
        inputs: &[&[T]],
        outputs: &mut [&mut [T]],
    ) -> ProcessResult<()> {
        self.drain_mailbox();

        // Apply pending params
        for p in self.pending.drain(..) {
            if p.node_idx < self.graph.nodes.len() {
                self.graph.nodes[p.node_idx].set_param(p.param_idx, &p.value);
            }
        }

        // Copy graph inputs into buffer pool
        for (i, input) in inputs.iter().enumerate() {
            if i < self.graph.buffers.len() {
                let buf = self.graph.buffers[i].as_mut_slice();
                let n = input.len().min(buf.len());
                buf[..n].copy_from_slice(&input[..n]);
            }
        }

        // Execute all nodes
        for node in &mut self.graph.nodes {
            node.execute(&mut self.graph.buffers);
        }

        // Copy buffer pool to graph outputs
        for (i, output) in outputs.iter_mut().enumerate() {
            if i < self.graph.output_mapping.len() {
                let src = self.graph.output_mapping[i];
                if src < self.graph.buffers.len() {
                    let buf = self.graph.buffers[src].as_slice();
                    let n = output.len().min(buf.len());
                    output[..n].copy_from_slice(&buf[..n]);
                }
            }
        }

        Ok(())
    }

    pub fn reset(&mut self) {
        for buf in &mut self.graph.buffers {
            buf.fill(T::ZERO);
        }
        for node in &mut self.graph.nodes {
            match &mut node.algo {
                crate::graph_compiler::AlgorithmVariant::Siso(algo) => {
                    Algorithm::reset(algo.as_mut());
                }
                crate::graph_compiler::AlgorithmVariant::Mimo(algo) => {
                    MultichannelAlgorithm::reset(algo.as_mut());
                }
            }
        }
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Algorithm<T>
    for CompiledGraphEngine<T, BUF_SIZE>
{
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let bufs: &[&[T]] = if let Some(inp) = input { &[inp] } else { &[] };
        let out_bufs: &mut [&mut [T]] = &mut [output];
        MultichannelAlgorithm::process(self, bufs, out_bufs)
    }

    fn reset(&mut self) {
        Self::reset(self);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> MultichannelAlgorithm<T>
    for CompiledGraphEngine<T, BUF_SIZE>
{
    fn num_inputs(&self) -> usize {
        self.graph.inputs
    }

    fn num_outputs(&self) -> usize {
        self.graph.outputs
    }

    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.process_tick(inputs, outputs)
    }

    fn reset(&mut self) {
        Self::reset(self);
    }
}
```

Wait — this code references `node.algo` as a public field. But `NodeClosure.algo` is private. Need to either make it public or add a `reset` method.

Fix: add `pub fn reset` to `NodeClosure` in `graph_compiler.rs`, and make `algo` pub(crate):

In `graph_compiler.rs`, change:
```rust
pub struct NodeClosure<T: Transcendental, const BUF_SIZE: usize> {
    pub(crate) algo: AlgorithmVariant<T>,
    ...
```

And add reset method:
```rust
pub fn reset(&mut self) {
    match &mut self.algo {
        AlgorithmVariant::Siso(algo) => Algorithm::reset(algo.as_mut()),
        AlgorithmVariant::Mimo(algo) => MultichannelAlgorithm::reset(algo.as_mut()),
    }
}
```

Let me rewrite the full engine code properly.

- [ ] **Step 2: Actually write the engine — production-quality full file**

Write the complete `graph_engine.rs`. Key differences from current:
- No `SubEngine`, no `DuplexData` (duplex not needed in first version — can be added later)
- No `ScheduledGraph`, no `Step` enum
- No `buf_slices()`
- `process_tick` is allocation-free
- Keep the `CompiledGraphEngine` name (renamed from `RillGraphEngine`)

```rust
//! Execution engine for CompiledGraph with a FixedBuffer pool.
//!
//! Runs a flat vector of NodeClosures in topological order. Zero heap
//! allocation on the real-time signal path.

use std::collections::HashMap;
use std::sync::Arc;

use rill_core::buffer::FixedBuffer;
use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::traits::{Algorithm, MultichannelAlgorithm, ParamValue, ProcessResult};
use rill_core_actor::{ActorRef, Mailbox};

use crate::graph_compiler::{AlgorithmVariant, CompiledGraph};

#[cfg(feature = "debug")]
use crate::debug::{CmdStr, CommandFrame, DebugControl, ProbeSlot};
#[cfg(feature = "debug")]
use rill_core::queues::spsc::SpscQueue;
#[cfg(feature = "debug")]
use std::sync::atomic::Ordering;

pub type ParamMap = HashMap<String, usize>;

struct PendingParam {
    node_idx: usize,
    param_idx: usize,
    value: ParamValue,
}

/// Graph execution engine running a [`CompiledGraph`].
pub struct CompiledGraphEngine<T: Transcendental, const BUF_SIZE: usize> {
    graph: CompiledGraph<T, BUF_SIZE>,
    pending: Vec<PendingParam>,
    param_maps: Vec<HashMap<String, usize>>,
    anchor_map: HashMap<String, usize>,
    mailbox: Arc<Mailbox<CommandEnum>>,
    actor_ref: ActorRef<CommandEnum>,
    #[cfg(feature = "debug")]
    pub(crate) probe_slots: Vec<std::sync::Arc<ProbeSlot>>,
    #[cfg(feature = "debug")]
    pub(crate) command_queue: std::sync::Arc<SpscQueue<CommandFrame, 256>>,
    #[cfg(feature = "debug")]
    pub(crate) debug_control: DebugControl,
}

impl<T: Transcendental, const BUF_SIZE: usize> CompiledGraphEngine<T, BUF_SIZE> {
    pub fn new(
        graph: CompiledGraph<T, BUF_SIZE>,
        mailbox: Arc<Mailbox<CommandEnum>>,
    ) -> Self {
        let actor_ref = mailbox.actor_ref();
        let anchor_map: HashMap<String, usize> = graph
            .node_names
            .iter()
            .enumerate()
            .map(|(i, name)| (name.clone(), i))
            .collect();
        let param_maps = vec![HashMap::new(); graph.nodes.len()];

        Self {
            graph,
            pending: Vec::new(),
            param_maps,
            anchor_map,
            mailbox,
            actor_ref,
            #[cfg(feature = "debug")]
            probe_slots: Vec::new(),
            #[cfg(feature = "debug")]
            command_queue: std::sync::Arc::new(SpscQueue::new()),
            #[cfg(feature = "debug")]
            debug_control: DebugControl::new(),
        }
    }

    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.actor_ref.clone()
    }

    pub fn param_map(&self) -> &HashMap<String, usize> {
        self.param_maps.first().unwrap_or(&EMPTY_MAP)
    }

    fn drain_mailbox(&mut self) {
        #[cfg(feature = "debug")]
        let block_idx = self.debug_control.block_index.load(Ordering::Relaxed);

        while let Some(cmd) = self.mailbox.pop() {
            if let CommandEnum::SetParameter(ref sp) = cmd {
                let param_name = sp.parameter.as_str();
                let mut applied = false;
                if !sp.anchor.is_empty() {
                    if let Some(&node_idx) = self.anchor_map.get(&sp.anchor) {
                        if let Some(&idx) = self.param_maps[node_idx].get(param_name) {
                            self.pending.push(PendingParam {
                                node_idx,
                                param_idx: idx,
                                value: sp.value.clone(),
                            });
                            applied = true;
                        }
                    }
                } else {
                    for (node_idx, map) in self.param_maps.iter().enumerate() {
                        if let Some(&idx) = map.get(param_name) {
                            self.pending.push(PendingParam {
                                node_idx,
                                param_idx: idx,
                                value: sp.value.clone(),
                            });
                            applied = true;
                            break;
                        }
                    }
                }
                #[cfg(feature = "debug")]
                if applied {
                    let _ = self.command_queue.push(CommandFrame {
                        block_index: block_idx,
                        command_kind: CmdStr::new("SetParameter"),
                        node_name: CmdStr::new(&sp.anchor),
                        param_name: CmdStr::new(&format!("{}", sp.parameter)),
                        value_repr: CmdStr::new(&format!("{:?}", sp.value)),
                    });
                }
            }
        }
    }

    pub fn process_tick(
        &mut self,
        inputs: &[&[T]],
        outputs: &mut [&mut [T]],
    ) -> ProcessResult<()> {
        #[cfg(feature = "debug")]
        {
            self.debug_control
                .block_index
                .fetch_add(1, Ordering::Relaxed);
        }
        self.drain_mailbox();

        #[cfg(feature = "debug")]
        {
            while self.debug_control.global_pause.load(Ordering::Acquire)
                && !self.debug_control.global_resume.load(Ordering::Acquire)
            {
                std::hint::spin_loop();
            }
            self.debug_control
                .global_resume
                .store(false, Ordering::Release);
        }

        // Phase 0: apply pending param changes
        for p in self.pending.drain(..) {
            if p.node_idx < self.graph.nodes.len() {
                self.graph.nodes[p.node_idx].set_param(p.param_idx, &p.value);
            }
        }

        // Phase 1: copy graph inputs into buffer pool
        for (i, input) in inputs.iter().enumerate() {
            if i < self.graph.inputs && i < self.graph.buffers.len() {
                let buf = self.graph.buffers[i].as_mut_slice();
                let n = input.len().min(buf.len());
                buf[..n].copy_from_slice(&input[..n]);
            }
        }

        // Phase 2: execute all nodes in topological order
        for node in &mut self.graph.nodes {
            node.execute(&mut self.graph.buffers);
        }

        // Phase 3: copy buffer pool to graph outputs
        for (i, output) in outputs.iter_mut().enumerate() {
            if i < self.graph.output_mapping.len() {
                let src = self.graph.output_mapping[i];
                if src < self.graph.buffers.len() {
                    let buf = self.graph.buffers[src].as_slice();
                    let n = output.len().min(buf.len());
                    output[..n].copy_from_slice(&buf[..n]);
                }
            }
        }

        Ok(())
    }

    pub fn reset(&mut self) {
        for buf in &mut self.graph.buffers {
            buf.fill(T::ZERO);
        }
        for node in &mut self.graph.nodes {
            node.reset();
        }
    }
}

static EMPTY_MAP: HashMap<String, usize> = HashMap::new();

impl<T: Transcendental, const BUF_SIZE: usize> Algorithm<T>
    for CompiledGraphEngine<T, BUF_SIZE>
{
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let bufs: &[&[T]] = if let Some(inp) = input { &[inp] } else { &[] };
        let out_bufs: &mut [&mut [T]] = &mut [output];
        MultichannelAlgorithm::process(self, bufs, out_bufs)
    }

    fn reset(&mut self) {
        Self::reset(self);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> MultichannelAlgorithm<T>
    for CompiledGraphEngine<T, BUF_SIZE>
{
    fn num_inputs(&self) -> usize {
        self.graph.inputs
    }

    fn num_outputs(&self) -> usize {
        self.graph.outputs
    }

    fn process(
        &mut self,
        inputs: &[&[T]],
        outputs: &mut [&mut [T]],
    ) -> ProcessResult<()> {
        self.process_tick(inputs, outputs)
    }

    fn reset(&mut self) {
        Self::reset(self);
    }
}
```

Also add `reset()` method to `NodeClosure` in `graph_compiler.rs`:
```rust
pub fn reset(&mut self) {
    match &mut self.algo {
        AlgorithmVariant::Siso(algo) => Algorithm::reset(algo.as_mut()),
        AlgorithmVariant::Mimo(algo) => MultichannelAlgorithm::reset(algo.as_mut()),
    }
}
```

- [ ] **Step 3: Update `NodeClosure` visibility in graph_compiler.rs**

Make `algo` field `pub(crate)` so the engine can access it for `reset()`:
```rust
pub struct NodeClosure<T: Transcendental, const BUF_SIZE: usize> {
    pub(crate) algo: AlgorithmVariant<T>,
    ...
```

- [ ] **Step 4: Update `lib.rs` — remove `graph_lower`, update `compile_graph`, add `graph_compiler` types**

In `rill-lang/src/lib.rs`:

Remove `pub mod graph_lower;` (line 17)

Replace `compile_graph` function (lines 92-138) with:

```rust
/// Compile rill-lang source into a graph engine.
pub fn compile_graph<T: Transcendental>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<graph_engine::CompiledGraphEngine<T, 512>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens, src.as_bytes())?;
    let mut typed = types::infer::infer_program_with(&program, registry)?;
    typed.program = reduce::reduce(&typed.program);
    let ir = lower::lower_with(&typed, registry, sample_rate)?;
    validate_block_builtins(&ir)?;

    // Build a single-node GraphIr
    use crate::graph_ir::{EdgeKind, GraphEdge, GraphIr, GraphNode};
    use std::collections::HashMap;
    let mut nodes: indexmap::IndexMap<String, GraphNode> = indexmap::IndexMap::new();
    nodes.insert(
        "main".to_string(),
        GraphNode {
            arity: (ir.num_inputs, ir.num_outputs),
            ir,
            params: vec![],
            keep: false,
            inline: false,
            is_bridge: false,
            feedback_read: vec![],
            feedback_write: vec![],
        },
    );
    let graph_ir = GraphIr {
        inputs: 1,
        outputs: 1,
        nodes,
        edges: vec![],
        topo_order: vec!["main".to_string()],
    };

    let compiled = graph_compiler::compile::<T, 512>(&graph_ir, registry, sample_rate)
        .map_err(|e| CompileError::Unsupported(e))?;

    let mailbox = Arc::new(Mailbox::new(64));
    Ok(graph_engine::CompiledGraphEngine::new(compiled, mailbox))
}
```

Add `indexmap` to the imports at the top of `compile_graph`.

- [ ] **Step 5: Update `program_runner.rs` for the new engine type**

Replace `RillGraphEngine<f32>` with `CompiledGraphEngine<f32, 512>` in `program_runner.rs`:

```rust
use crate::graph_engine::CompiledGraphEngine;

pub struct ProgramRunner {
    engine: CompiledGraphEngine<f32, 512>,
    ...
}

impl ProgramRunner {
    pub fn new(
        engine: CompiledGraphEngine<f32, 512>,
        ...
    ) -> Self { ... }

    pub fn engine(&self) -> &CompiledGraphEngine<f32, 512> {
        &self.engine
    }

    fn process_tick(&mut self, tick: &ClockTick) {
        ...
        let _ = self.engine.process_tick(
            &input_slice,
            &mut [&mut self.output_buf[..block_size]],
        );
        ...
    }
}
```

- [ ] **Step 6: Delete `graph_lower.rs`**

```bash
rm rill-lang/src/graph_lower.rs
```

- [ ] **Step 7: Add `buffer` import to rill-lang Cargo.toml if needed**

Check if `rill-core::buffer::FixedBuffer` is accessible from `rill-lang`. `rill-lang` depends on `rill-core` — the `FixedBuffer` type should be reachable via `rill_core::buffer::FixedBuffer`.

Check by running: `cargo check -p rill-lang 2>&1`

- [ ] **Step 8: Fix all compilation errors**

```bash
cargo check -p rill-lang 2>&1
```

Fix any remaining errors iteratively. Expected categories:
- Missing `use` imports in `graph_engine.rs` and `graph_compiler.rs`
- Wrong `FixedBuffer` API calls (use `as_slice()`, `as_mut_slice()`, `fill()`)
- `graph_lower` references in other files that weren't updated

- [ ] **Step 9: Fix rill-adrift references (separate commit)**

Update `rill-adrift/src/modular/mod.rs` to use the new API:

```rust
// Find and replace:
// rill_lang::graph_lower::lower(&ir)
// → rill_lang::graph_compiler::compile::<f32, 512>(&ir, &registry, sample_rate)

// rill_lang::graph_engine::RillGraphEngine::new(schedule, programs, mailbox, buf_size)
// → rill_lang::graph_engine::CompiledGraphEngine::new(compiled, mailbox)
```

Run: `cargo check -p rill-adrift 2>&1` and fix errors.

- [ ] **Step 10: Run full workspace check**

```bash
cargo check --workspace 2>&1
```
Expected: zero errors (warnings OK).

- [ ] **Step 11: Commit**

```bash
git add rill-lang/src/graph_engine.rs rill-lang/src/graph_compiler.rs rill-lang/src/lib.rs rill-lang/src/program_runner.rs rill-adrift/src/modular/mod.rs
git rm rill-lang/src/graph_lower.rs
git commit -m "feat(rill-lang): replace ScheduledGraph with closure-based CompiledGraph engine"
```

---

### Task 5: Migrate mixer bus_buffers to FixedBuffer

**Files:**
- Modify: `rill-lang/src/builtins/mixer.rs`

- [ ] **Step 1: Check mixer bus buffer type**

Read the current `bus_buffers` field and construction in `rill-lang/src/builtins/mixer.rs`. Change:

```rust
// Before:
bus_buffers: Vec<Vec<T>>,
// After:
bus_buffers: Vec<FixedBuffer<T, BUF_SIZE>>,
```

Adjust construction and usage. The mixer is a lang builtin that runs inside `RillProgram` or as a `NodeClosure`. Its internal buffers should use `FixedBuffer`.

Implementation detail: the mixer is generic over `T`, not `const BUF_SIZE`. We may need to make the mixer generic over `BUF_SIZE` too, or use a type-erased approach. Check the actual mixer code first.

- [ ] **Step 2: Fix compilation**

```bash
cargo check -p rill-lang 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add rill-lang/src/builtins/mixer.rs
git commit -m "refactor(rill-lang): migrate mixer bus buffers to FixedBuffer"
```

---

### Task 6: Full workspace test and lint

**Files:** none (verification only)

- [ ] **Step 1: Run all tests**

```bash
cargo test --workspace 2>&1
```
Expected: all tests pass.

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --workspace --all-features 2>&1
```
Fix any warnings.

- [ ] **Step 3: Run rustfmt**

```bash
cargo fmt --all
```

- [ ] **Step 4: Commit**

```bash
git commit -am "style: clippy fixes and formatting"
```
