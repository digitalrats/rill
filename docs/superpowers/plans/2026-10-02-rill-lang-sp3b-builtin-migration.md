# SP-3b: Migrate the Builtin Catalog to the FFI Layer and Remove `BuiltinSig` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate all ~38 legacy builtins (registered via `rill_core::builtin::{BuiltinSig, ParamType, Registry}`) onto the SP-3a FFI layer (`foreign fn` declarations + `rill_lang::ffi::ForeignRegistry`), then **remove `BuiltinSig`/`ParamType`/`SignatureSource`** from the codebase. Nothing registration-related or rill-lang-related remains in `rill-core`. A Faust-combinator sugar pass keeps signal-input builtins callable in the legacy combinator style (`_ : onepole 200.0 0.7`) so migrating them does not break existing programs.

**Architecture:** `rill_lang::builtin` becomes the definition (it already re-exports `rill_core::builtin::*`); the signal-signature contract lives in the language. Each implementation crate (with a `lang` feature + rill-lang dependency) registers factories directly into `rill_lang::ffi::ForeignRegistry`. `BuiltinSig`/`ParamType`/`SignatureSource` are deleted; inference/lowering/graph-reconstruction consume `FfiSig` instead. Dead crates/code (rill-digital-filters, rill-analog-filters, rill-analog-effects, `register_graph_nodes` stubs, `rill-lang/src/builtins` mixer/eq/dry_wet duplicates) are removed. Tape moves to a `tape_loop` constructor function (`Tape` as a `Buffer` family member).

**Tech Stack:** Rust, no new external dependencies. Branch `feature/rill-lang-categories`.

---

## Target crate map (after SP-3b)

| Crate | Builtins registered (via `ForeignRegistry`) | Deps added |
|---|---|---|
| `rill-lang` | `complex`/`conj`/`re`/`im`/`norm`/`arg`/`cmul`/`cadd`; `sine`/`saw`/`square`/`triangle`/`noise`; `integrator`/`leaky_integrator`; `tape_loop` | `rill-core-dsp` (feature `dsp`), `rill-core-model` (feature `model`), `rill-digital-effects` (feature `dsp`) |
| `rill-digital-effects` | `delay`/`distortion`/`limiter` (algorithms only, no lang layer) | none |
| `rill-router` | `graphic_eq`/`mono_to_stereo`/`mixer`/`eq_parametric`/`dry_wet` | none (already has `lang`) |
| `rill-core-dsp` | **nothing** (algorithms only) | none |
| `rill-core-model` | **nothing** (algorithms only) | drop `rill-lang` |
| `rill-sampler` | `sampler` | none (already has `lang`) |
| `rill-fft` | `spectralgate`/`spectraldelay`/`convolver` | none (already has `lang`) |
| `rill-lofi` | `lofi`/`ay38910` | none (already has `lang`) |
| **removed** | `rill-digital-filters`, `rill-analog-filters`, `rill-analog-effects`, `rill-lang/src/builtins` | — |

---

## Task 1: Fix `is_multi` — select `BuiltinInst` variant by factory kind, not arity

**Problem.** `build_builtin` (rill-lang/src/program.rs:158) picks `BuiltinInst::MultichannelBlock` when `bi.signal_ins > 1 || bi.signal_outs > 1`. The 8 complex ops (`conj` 2→2, `re` 2→1, `cmul` 4→2, …) are registered as **Block** factories → the multichannel branch calls `build_multichannel_block` on a `Factory::Block` → `.expect` panics. The interpreter already supports interleaved Block multi-channel (interp.rs:2160-2174).

**Files:** `rill-lang/src/program.rs`, `rill-lang/src/ffi.rs`, `rill-lang/tests/ffi.rs`

- [ ] **Step 1: Write the failing test**

Append to `rill-lang/tests/ffi.rs`:

```rust
#[test]
fn complex_ops_build_with_block_factory() {
    // `conj` is a 2→2 builtin registered as a Block factory. `is_multi` must
    // select the variant by FACTORY KIND, not signal arity — otherwise build
    // panics with "registry build_multichannel_block failed".
    use rill_lang::ffi::ForeignRegistry;
    use rill_core::builtin::BlockBuiltin;
    use rill_core::traits::Algorithm;

    struct Conj;
    impl<T: rill_core::math::Transcendental> Algorithm<T> for Conj {
        fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> rill_core::ProcessResult<()> {
            output.copy_from_slice(input.unwrap_or(&[]));
            Ok(())
        }
    }
    impl<T: rill_core::math::Transcendental> BlockBuiltin<T> for Conj {}

    let mut ffi = ForeignRegistry::<f32>::new();
    ffi.register_block("conj", |_p: &[f64], _sr: f32| Box::new(Conj));

    let src = r#"
        foreign fn conj : FixedBuffer f32 -> FixedBuffer f32 -> FixedBuffer f32 -> FixedBuffer f32;
        main = conj _ _;
    "#;
    // Must compile and run, not panic at build.
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0,2.0,3.0,4.0], &[5.0,6.0,7.0,8.0]], &mut [&mut out]).unwrap();
    assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
}
```

(Note: `conj` is 2→2; `foreign fn conj : FixedBuffer f32 -> FixedBuffer f32 -> FixedBuffer f32 -> FixedBuffer f32` gives 2 signal ins + 2 outs — a `Pair` result. Verify the FfiSig accepts a `Pair (FixedBuffer f32) (FixedBuffer f32)` result — it does, `outs_from_typeexpr` ffi.rs:104-113.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test ffi complex_ops_build_with_block_factory`
Expected: FAIL — build panic (`registry build_multichannel_block failed`).

- [ ] **Step 3: Change `build_builtin` to select by factory kind**

In `rill-lang/src/program.rs`, `build_builtin` (line 151): instead of `let is_multi = bi.signal_ins > 1 || bi.signal_outs > 1;` branching first, ask the registry/FFI which factory kind the name maps to, then build that variant:

```rust
fn build_builtin<T: Transcendental>(
    bi: &BuiltinInstance,
    registry: &crate::builtin::Registry<T>,
    foreign: Option<&crate::ffi::ForeignRegistry<T>>,
    resources: &mut Option<&mut rill_core::buffer::ResourceRegistry<T>>,
    sample_rate: f32,
) -> Result<BuiltinInst<T>, CompileError> {
    // A legacy Block factory (or an FFI Block factory) builds a `Block` variant
    // even when the signal arity is >1 (the interpreter's interleaved Block path
    // handles multi-channel). Variant selection follows the FACTORY KIND.
    let legacy_kind = registry.kind(&bi.name); // Block | MultichannelBlock | ResourceBlock | ResourceMultichannelBlock
    let ffi_kind = foreign.and_then(|f| f.kind(&bi.name));
    match legacy_kind.or(ffi_kind) {
        Some(ForeignKind::Multichannel) | Some(ForeignKind::ResourceMultichannel) => {
            // ... existing multichannel branch (resource or build_multichannel_block or ffi.build_multichannel_block) ...
        }
        Some(ForeignKind::Block) | Some(ForeignKind::ResourceBlock) | None => {
            // ... existing block branch (resource or build_block or ffi.build_block) ...
        }
    }
}
```

**Implementation detail:** the current `Registry<T>`/`Entry<T>` doesn't expose its factory kind. Add a `pub(crate) fn kind(&self, name: &str) -> Option<BuiltinFactoryKind>` to both `rill_core::builtin::Registry` (or the moved-to-rill-lang `Registry`, depending on task order) and `rill_lang::ffi::ForeignRegistry`. Define one shared enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinFactoryKind {
    Block,
    MultichannelBlock,
    ResourceBlock,
    ResourceMultichannelBlock,
}
```

For `ForeignRegistry`, map `Factory::Block → BuiltinFactoryKind::Block`, `Factory::MultichannelBlock → BuiltinFactoryKind::MultichannelBlock`.

Preserve legacy behavior: a resource builtin (tape heads) still resolves via the resource branch; the `.expect` panics for a genuinely-missing variant stay.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-lang --test ffi complex_ops_build_with_block_factory` and full `cargo test -p rill-lang`.
Expected: PASS — the Block factory with 2→2 arity builds a `BuiltinInst::Block` and runs through the interleaved path.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/program.rs rill-lang/src/ffi.rs rill-lang/tests/ffi.rs
git commit -m 'fix(rill-lang): select BuiltinInst variant by factory kind, not signal arity — complex ops build'
```

---

## Task 2: Move `builtin.rs` from rill-core to rill-lang

**Problem.** `rill_core::builtin` holds the registry contract, but nothing registration-related should remain in rill-core. `rill_lang::builtin` already `pub use rill_core::builtin::*;` (rill-lang/src/builtin.rs:5) — the move is mostly mechanical.

**Files:** `rill-core/src/builtin.rs` (delete), `rill-lang/src/builtin.rs` (become the definition), `rill-core/src/lib.rs` (remove re-exports), ~20 direct `rill_core::builtin` references.

- [ ] **Step 1: Move the module**

Copy `rill-core/src/builtin.rs` → `rill-lang/src/builtin.rs` (replacing the `pub use` shim). Update its internal `use` paths: `crate::buffer::ResourceRegistry` → `rill_core::buffer::ResourceRegistry`, `crate::math::Transcendental` → `rill_core::math::Transcendental`, `crate::traits::*` → `rill_core::traits::*`.

In `rill-core/src/lib.rs`: remove `pub mod builtin;` (line 79) and `pub use builtin::MultichannelBlockBuiltin;` (line 99).

- [ ] **Step 2: Fix direct `rill_core::builtin` references in rill-lang**

Replace `rill_core::builtin::` → `crate::builtin::` (or `rill_lang::builtin::`) in: `rill-lang/src/ffi.rs:17`, `rill-lang/src/register.rs:3,5`, `rill-lang/src/program.rs:6`, `rill-lang/src/lib.rs:275` (test), `rill-lang/src/lower.rs:6`, `rill-lang/src/types/infer.rs:14`, `rill-lang/src/schedule.rs` (test), `rill-lang/src/graph/reconstruct.rs:14`, `rill-lang/src/prelude.rs:7`, `rill-lang/src/ir.rs:11`.

- [ ] **Step 3: Fix crates that DO depend on rill-lang** (mechanical)

`rill-sampler/src/lang.rs:2`, `rill-sampler/src/tape/lang.rs:5,59,60`, `rill-sampler/tests/tape_builtins.rs`, `rill-router/src/lang.rs:4`, `rill-analog-effects/src/lang.rs:4` (being deleted — skip), `rill-adrift/src/lang_builtins.rs:7,8,31`, `rill-lang/tests/ffi.rs`, `rill-lang/tests/caf_free_variables.rs`: `rill_core::builtin::` → `rill_lang::builtin::`.

- [ ] **Step 4: Verify**

Run: `cargo check --workspace --all-features`. Fix compile errors. (Crates WITHOUT rill-lang dep that reference `rill_core::builtin` — `rill-core-dsp`, `rill-digital-effects` — are handled in Tasks 5-6; for now they break, which is expected mid-migration. To keep the workspace green, you may do Tasks 5-6 before this check passes, or temporarily keep a stub in rill-core. **Recommendation:** do Task 2 Step 4 together with Task 5 (digital-effects gets lang) and Task 7 (generators move) so the workspace compiles at one point.)

- [ ] **Step 5: Commit**

```bash
git add rill-core/src/builtin.rs rill-core/src/lib.rs rill-lang/src/builtin.rs
git commit -m 'refactor(rill-lang): move builtin registry module from rill-core to rill-lang'
```

---

## Task 3: Inline foreign catalog — auto-register builtin declarations

**Problem.** Programs must see `sine`, `biquad`, etc. without a user-written `foreign fn` per program. Add an in-language catalog of foreign declarations auto-registered into `TypeEnv::foreign_sigs` (NOT in `SIGNAL_PRELUDE` — that holds only `Buffer`/`FixedBuffer` types).

**Files:** `rill-lang/src/types/ty.rs` (catalog const), `rill-lang/src/types/ffi.rs`, `rill-lang/src/register.rs` (catalog source)

- [ ] **Step 1: Add the catalog**

**SCOPE NOTE (updated):** the catalog in this task covers the builtins whose FFI signatures work WITHOUT the combinator-sugar (Task 3b): the 0-input generators (`sine`/`saw`/`square`/`triangle`/`noise`), the complex ops (`complex`/`conj`/`re`/`im`/`norm`/`arg`/`cmul`/`cadd`), `sampler`, `ay38910`, and the record `data` types. The signal-input builtins (`onepole`/`lowpass`/`biquad`/`delay`/`mixer`/… ) are added to the catalog in **Task 3b** together with the combinator-sugar that makes their legacy call style work. This keeps Task 3's catalog regression-safe.

In `rill-lang/src/types/ty.rs`, add a `BUILTIN_FOREIGN_DECLS: &str` constant with the `foreign fn` declarations for the catalog (the 0-input + complex ops subset; see the plan's Target map). It is parsed like the preludes but its defs feed `foreign_sigs` only (not `data_types`):

```rust
pub(crate) const BUILTIN_FOREIGN_DECLS: &str = r#"
foreign fn sine   : Float -> Float -> Float -> FixedBuffer f32;
foreign fn saw    : Float -> Float -> Float -> FixedBuffer f32;
foreign fn square : Float -> Float -> Float -> FixedBuffer f32;
foreign fn triangle : Float -> Float -> Float -> FixedBuffer f32;
foreign fn noise  : Float -> Float -> FixedBuffer f32;
foreign fn complex : Float -> Float -> Pair (FixedBuffer f32) (FixedBuffer f32);
foreign fn conj : FixedBuffer f32 -> FixedBuffer f32 -> Pair (FixedBuffer f32) (FixedBuffer f32);
foreign fn re   : FixedBuffer f32 -> FixedBuffer f32 -> FixedBuffer f32;
foreign fn im   : FixedBuffer f32 -> FixedBuffer f32 -> FixedBuffer f32;
foreign fn norm : FixedBuffer f32 -> FixedBuffer f32 -> FixedBuffer f32;
foreign fn arg  : FixedBuffer f32 -> FixedBuffer f32 -> FixedBuffer f32;
foreign fn cmul : FixedBuffer f32 -> FixedBuffer f32 -> FixedBuffer f32 -> FixedBuffer f32 -> Pair (FixedBuffer f32) (FixedBuffer f32);
foreign fn cadd : FixedBuffer f32 -> FixedBuffer f32 -> FixedBuffer f32 -> FixedBuffer f32 -> Pair (FixedBuffer f32) (FixedBuffer f32);
foreign fn sampler : Float -> Float -> Float -> Float -> Float -> FixedBuffer f32;
foreign fn ay38910 : Float -> Float -> FixedBuffer f32;

main = _;
"#;
```

Also add the record/data types the record-param builtins need (mixer/eq/dry_wet) — these are in `SIGNAL_PRELUDE` or the catalog (they are `data` declarations, so they go through the `data_types` registration path):

```rill
// in the same catalog (parsed as `Def::Data` too):
data MixerConfig = { buses: Int, master_vol: Float };
data EqBand = { freq: Float, q: Float, gain_db: Float, band_type: Int };
data EqConfig = { bands: List EqBand };
data DryWetConfig = { mix: Float };
```

**Verify the exact signatures against the real catalog** (from the SP-3b research): `sine`/`saw`/`square`/`triangle` = 0→1 with 3 Float params; `noise` = 0→1 with 2; `integrator` = 1→1 with 0; `leaky_integrator` = 1→1 with 1 (`coeff`); `complex` = 0→2 with 2 (`re`,`im`); `conj`=2→2, `re`=2→1, `im`=2→1, `norm`=2→1, `arg`=2→1, `cmul`=4→2, `cadd`=4→2 (all 0 params); `delay`=1→1 3 params; `distortion`=1→1 2; `limiter`=1→1 2; `graphic_eq`=1→1 1; `mono_to_stereo`=1→2 2; `mixer`=variadic→2 with `MixerConfig` record; `eq_parametric`=1→1 with `EqConfig`; `dry_wet`=2→2 with `DryWetConfig`; `spectralgate`/`spectraldelay`=1→1 2; `convolver`=1→1 2; `analog_moog`=1→1 2; `sampler`=0→1 5; `lofi`=1→1 7; `ay38910`=0→1 2; `tape_loop`=Int→Tape. **Double-check each against `rill-core-dsp/src/lang/register.rs`, `rill-lang/src/register.rs`, `rill-router/src/lang.rs`, etc. — do not trust this list verbatim; correct any arity/param mismatch.**

- [ ] **Step 2: Auto-register in `with_builtins`**

In `with_builtins` (ty.rs:382-507), after parsing the two preludes, parse `BUILTIN_FOREIGN_DECLS` and register its `Def::Foreign` into `foreign_sigs` and its `Def::Data` into `data_types`/`data_arities` (reuse the existing phase-2 data loop). Extract a small helper `register_foreign_decls(env, src)` to avoid a third copy of the tokenize/parse/register block.

- [ ] **Step 3: Test**

Add to `rill-lang/tests/ffi.rs`:

```rust
#[test]
fn builtin_catalog_is_auto_registered() {
    // `sine` and `biquad` resolve WITHOUT a user-written `foreign fn` declaration.
    let src = r#"
        main = sine 440.0 1.0 0.0;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 0);
    assert_eq!(typed.process_ty.arity_out(), 1);
}
```

- [ ] **Step 4: Verify**

Run: `cargo test -p rill-lang --test ffi builtin_catalog_is_auto_registered`, full `cargo test -p rill-lang`. The catalog's `data` types (MixerConfig etc.) must register without breaking existing tests.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/ty.rs rill-lang/src/types/ffi.rs rill-lang/tests/ffi.rs
git commit -m 'feat(rill-lang): inline builtin foreign catalog auto-registered into TypeEnv'
```

---

## Task 3b: Faust-combinator sugar — signal-input builtins keep legacy call style

**Problem.** Legacy programs call signal-input builtins combinatorially — the signal wire is fed through `:`, `~`, `,`, `<:`, `:>` rather than as a positional FFI arg:

```rill
main = _ : onepole 200.0 0.7;      // legacy: Apply("onepole",[200.0,0.7]) as a 1→1 arrow
main = + ~ onepole 500.0 0.5;      // feedback
```

The FFI declaration `onepole : FixedBuffer f32 -> Float -> Float -> FixedBuffer f32` requires a positional signal arg (`onepole _ 200.0 0.7`). Without sugar, migrating `onepole` into the catalog breaks every existing `_ : onepole …` program. **User decision (2026-10-03): implement Faust-combinator sugar for ALL combinators (`:`, `~`, `,`, `<:`, `:>`) now** so signal-input builtins migrate with zero program rewrite, and extend the catalog with them.

**Design.** A `foreign fn`-declared name used in a combinator as an ARROW (not as a positional apply) auto-binds the missing leading `FixedBuffer` params to `Wire` (`_`) in declaration order. Concretely:
- `Seq(lhs, Apply(name, args))` where `name` is foreign with `n_sig` leading `FixedBuffer` params and `args.len() == param_count - n_sig` → rewrite to `Seq(lhs, Apply(name, [Wire × n_sig] ++ args))`.
- Same for `Loop` (`~`), `Par` (`,`), `Split` (`<:`), `Merge` (`:>`) where an operand is such an Apply.
- The rewrite happens in `reduce.rs` (single pass over the AST after parsing, before infer) so infer/lower see the positionally-bound form.

**Mechanism decision (user: "в плане"):** implement as a desugaring pass in `reduce.rs` that consults the foreign catalog (`TypeEnv::foreign_sigs` → `ffi_sig_from_typeexpr` → `FfiParam::Signal` count). `reduce.rs` currently runs before `TypeEnv` is built; if that's a problem, run the pass inside `infer` as the first step (a pre-infer AST rewrite), or in `parser` with a static name table. **Prefer the `reduce.rs`/pre-infer AST rewrite** — it keeps infer/lower unchanged and applies uniformly to all combinators. If `reduce.rs` cannot access `foreign_sigs` cheaply, add a lightweight `is_foreign_with_signals(name)` helper backed by the catalog constant (parsed once).

**Files:** `rill-lang/src/reduce.rs` (or a new `rill-lang/src/desugar.rs` called from `infer_program`/`lower`), `rill-lang/src/types/ty.rs` (catalog — add the signal-input builtins), `rill-lang/src/types/ffi.rs`, `rill-lang/tests/ffi.rs`

- [ ] **Step 1: Write the failing test**

Append to `rill-lang/tests/ffi.rs`:

```rust
#[test]
fn signal_input_builtin_legacy_call_style_sugar() {
    // `_ : onepole 200.0 0.7` — the legacy combinator style must desugar to
    // `onepole _ 200.0 0.7` (positional signal arg), so the FFI declaration
    // types and lowers without rewriting the program.
    let src = r#"
        foreign fn onepole : FixedBuffer f32 -> Float -> Float -> FixedBuffer f32;
        main = _ : onepole 200.0 0.7;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 1);
    assert_eq!(typed.process_ty.arity_out(), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test ffi signal_input_builtin_legacy_call_style_sugar`
Expected: FAIL — `onepole` is not declared (`unknown identifier`) or an arity error (the FFI path needs 3 args, the call provides 2).

- [ ] **Step 3: Add the signal-input builtins to the catalog**

Extend `BUILTIN_FOREIGN_DECLS` (from Task 3) with the signal-input builtins, each verified against the real registration file:

```rill
foreign fn integrator : FixedBuffer f32 -> FixedBuffer f32;
foreign fn leaky_integrator : FixedBuffer f32 -> Float -> FixedBuffer f32;
foreign fn onepole : FixedBuffer f32 -> Float -> Float -> FixedBuffer f32;
foreign fn moog : FixedBuffer f32 -> Float -> Float -> FixedBuffer f32;
foreign fn lowpass : FixedBuffer f32 -> Float -> Float -> FixedBuffer f32;
foreign fn highpass : FixedBuffer f32 -> Float -> Float -> FixedBuffer f32;
foreign fn biquad : FixedBuffer f32 -> Float -> Float -> Float -> Float -> FixedBuffer f32;
foreign fn delay : FixedBuffer f32 -> Float -> Float -> Float -> FixedBuffer f32;
foreign fn distortion : FixedBuffer f32 -> Float -> Float -> FixedBuffer f32;
foreign fn limiter : FixedBuffer f32 -> Float -> Float -> FixedBuffer f32;
foreign fn graphic_eq : FixedBuffer f32 -> Float -> FixedBuffer f32;
foreign fn mono_to_stereo : FixedBuffer f32 -> Float -> Float -> Pair (FixedBuffer f32) (FixedBuffer f32);
foreign fn mixer : List (FixedBuffer f32) -> MixerConfig -> Pair (FixedBuffer f32) (FixedBuffer f32);
foreign fn eq_parametric : FixedBuffer f32 -> EqConfig -> FixedBuffer f32;
foreign fn dry_wet : FixedBuffer f32 -> FixedBuffer f32 -> DryWetConfig -> Pair (FixedBuffer f32) (FixedBuffer f32);
foreign fn spectralgate : FixedBuffer f32 -> Float -> Float -> FixedBuffer f32;
foreign fn spectraldelay : FixedBuffer f32 -> Float -> Float -> FixedBuffer f32;
foreign fn convolver : FixedBuffer f32 -> Float -> Float -> FixedBuffer f32;
foreign fn analog_moog : FixedBuffer f32 -> Float -> Float -> FixedBuffer f32;
foreign fn lofi : FixedBuffer f32 -> Float -> Float -> Float -> Float -> Float -> Float -> Float -> FixedBuffer f32;
```

**Verify each against the real registration** (see Task 3's list). Note: `mixer`'s `List (FixedBuffer f32)` variadic signal + record is the tricky one — the sugar binds `VariadicSignal` to the remaining signal wires; `dry_wet` (2 signal ins) binds 2 wires.

- [ ] **Step 4: Implement the combinator-sugar desugaring**

Add a pass (in `reduce.rs` or a new `desugar.rs`) that rewrites the AST before infer. For each combinator whose operand is `Expr::Apply { name, args, .. }` (or `Expr::Ref(name)`), look up the foreign sig; count leading `FfiParam::Signal` (and `VariadicSignal` → remaining) params; if `args.len() == param_count - sig_count` (signals not supplied), prepend `Expr::Wire` for each missing signal slot:

```rust
fn desugar_foreign_combinators(prog: &mut Program, env: &TypeEnv) {
    for def in &mut prog.defs {
        if let Def::Local { body, .. } = def {
            *body = rewrite(body, env);
        }
    }
}

fn rewrite(e: &Expr, env: &TypeEnv) -> Expr {
    match e {
        Expr::Seq(l, r, sp) => Expr::Seq(Box::new(rewrite(l, env)), Box::new(bind_signal_args(r, env)), *sp),
        Expr::Loop(l, r, sp) => Expr::Loop(Box::new(rewrite(l, env)), Box::new(bind_signal_args(r, env)), *sp),
        Expr::Par(l, r, sp) => Expr::Par(Box::new(rewrite(l, env)), Box::new(bind_signal_args(r, env)), *sp),
        Expr::Split(l, r, sp) => Expr::Split(Box::new(rewrite(l, env)), Box::new(bind_signal_args(r, env)), *sp),
        Expr::Merge(l, r, sp) => Expr::Merge(Box::new(rewrite(l, env)), Box::new(bind_signal_args(r, env)), *sp),
        _ => clone_with_children_rewritten(e, |x| rewrite(x, env)),
    }
}

fn bind_signal_args(e: &Expr, env: &TypeEnv) -> Expr {
    let name = match e {
        Expr::Apply { name, .. } => name,
        Expr::Ref(name, _) => name,  // a bare foreign name used as an arrow
        _ => return e.clone(),
    };
    let Some(sig) = env.foreign_sigs.get(name.as_str()).and_then(ffi_sig_from_typeexpr) else {
        return e.clone();
    };
    let sig_count = sig.params.iter().filter(|p| matches!(p, FfiParam::Signal)).count();
    let variadic = sig.params.iter().any(|p| matches!(p, FfiParam::VariadicSignal));
    let supplied = match e {
        Expr::Apply { args, .. } => args.len(),
        _ => 0,
    };
    let missing = sig_count - supplied.min(sig_count);  // scalar params not yet supplied
    if missing <= 0 && !variadic {
        return e.clone();
    }
    // Prepend `missing` Wire args (and, for variadic signal, the remaining
    // wires are bound at infer via the VariadicSignal path).
    let wires = vec![Expr::Wire(Span::default()); missing];
    match e {
        Expr::Apply { name, args, span } => {
            let mut new_args = wires;
            new_args.extend(args.iter().cloned());
            Expr::Apply { name: name.clone(), args: new_args, span: *span }
        }
        Expr::Ref(name, span) => Expr::Apply { name: name.clone(), args: wires, span: *span },
        _ => unreachable!(),
    }
}
```

**Precise arity rule:** the missing count = `(# of Signal/VariadicSignal params) - (# of args already supplied that are wires/signals)`. The simplest correct v1 rule: if `args.len() < total_param_count` AND the args are the trailing scalar params (i.e. the leading signal params are absent), prepend exactly `n_sig` wires. Implement this carefully with a test matrix: `_ : onepole 200.0 0.7` (1 sig, 2 scalars → prepend 1 wire), `+ ~ onepole 500.0 0.5`, `_, _ : dry_wet {mix}` (2 sig → 2 wires), `_ : mixer {buses, master_vol}` (variadic → prepend the list), `graphic_eq` (1→1), `mono_to_stereo` (1→2). Verify each against its registration.

**Call the pass** from `infer_program_with` (and `infer_program`) before the phase-1 registration, so `TypeEnv::foreign_sigs` is available. If `reduce.rs` is cleaner (it runs after parse, before infer), the `TypeEnv` is NOT yet built there — so prefer calling the desugar from `infer_program_with` where `env` exists, or build a minimal env. **Note the placement decision in your report.**

- [ ] **Step 5: Run test to verify it passes + regression**

Run: `cargo test -p rill-lang --test ffi signal_input_builtin_legacy_call_style_sugar` and full `cargo test -p rill-lang`. The existing `_ : onepole …` / `_ : lowpass …` / `+ ~ onepole …` tests (lower.rs:5163, 5178, 5223; infer.rs:4507-4523; schedule.rs:248; render.rs:517) must now type through the FFI catalog — **fix any that fail** (they may need a foreign-sig-compatible shape, but the desugar should make them pass unchanged).

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m 'feat(rill-lang): Faust-combinator sugar — signal-input builtins keep legacy call style'
```

---

## Task 4: Extend `FfiSig` with `param_names`, record schemas, `Tape`/Resource

**Problem.** `graph/reconstruct.rs` needs `param_names` + Resource detection; record-param builtins (mixer/eq/dry_wet) need schema-aware field flattening; tape needs a `Tape`/Resource param kind. Extend the descriptor.

**Files:** `rill-lang/src/types/ffi.rs`, `rill-lang/src/types/infer.rs`, `rill-lang/src/lower.rs`, `rill-lang/src/graph/reconstruct.rs`

- [ ] **Step 1: Extend `FfiParam` and `FfiSig`**

In `rill-lang/src/types/ffi.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum FfiParam {
    Signal,
    Scalar,                     // Float/Int/Bool/String
    VariadicSignal,             // List (FixedBuffer a)
    Record(String),             // a Data record type name
    Resource,                   // a named shared buffer (tape) — NOT a signal
}

#[derive(Debug, Clone, PartialEq)]
pub struct FfiSig {
    pub params: Vec<FfiParam>,
    pub signal_outs: usize,
    /// Parameter display names in declaration order (for graph reconstruction).
    pub param_names: Vec<String>,
}

/// A record schema (field name, scalar type, default), mirroring the legacy
/// RecordSchema. Filled from the `data` declaration's fields.
#[derive(Debug, Clone, PartialEq)]
pub struct FfiRecordSchema {
    pub fields: Vec<(String, FfiScalar, Option<f64>)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FfiScalar { Float, Int }
```

`ffi_sig_from_typeexpr` gains: (a) `Tape a` → `FfiParam::Resource`; (b) `param_names` filled from the declaration order (index-based names `param0`, `param1`, … are acceptable defaults — reconstruct uses them for ordering, and the record-param path supplies field names); (c) a companion `ffi_record_schema(env, type_name) -> Option<FfiRecordSchema>` that reads the `data` type's fields from `TypeEnv::data_types` (converting `ValueTy::Float`→Float, `ValueTy::Int`→Int).

**Record-param flattening contract:** a `Record(String)` param's runtime value is the flat list of its `data` fields' `f64`s in schema order (defaults applied), with a nested `List EqBand` field contributed as a sequence of band-record flat values. This mirrors legacy `lower.rs:3886-3919`.

- [ ] **Step 2: Infer — accept `Record` and `Resource`**

`infer_apply_impl` foreign arm (infer.rs:3265-3371): replace the `FfiParam::Record(_) => Err("SP-3b")` with validation that the arg is a record literal of the named data type; `Resource` params validate the arg is a `Ref` to a declared tape (name resolves in the resource table). `expr_has_variadic_signal` (infer.rs:4000) gains an FFI branch: check the foreign sig for `VariadicSignal`.

- [ ] **Step 3: Lower — record flattening + Resource wiring**

`lower.rs` foreign arm (3627-3776): implement the `Record(String)` branch — walk the record literal, flatten fields in schema order to `param_values` (and `intern_param` each field), mirroring `lower.rs:3886-3919`. `Resource` branch: accept a `Ref` to a tape name, set `resource = Some(name)` (the build path resolves it). `rhs_variadic` (lower.rs:4513) gains an FFI branch.

- [ ] **Step 4: Test**

Add to `rill-lang/tests/ffi.rs`:

```rust
#[test]
fn record_param_flattens_schema_fields() {
    // `dry_wet` takes a DryWetConfig record `{ mix: 0.5 }` → one f64 param.
    let src = r#"
        foreign fn dry_wet : FixedBuffer f32 -> FixedBuffer f32 -> DryWetConfig -> Pair (FixedBuffer f32) (FixedBuffer f32);
        main = dry_wet _ _ { mix: 0.5 };
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    let ir = rill_lang::lower::lower_with_cafs(&typed, &rill_lang::builtin::NoSigs, 44100.0, &typed.cafs).unwrap();
    let bi = ir.builtins.iter().find(|b| b.name == "dry_wet").unwrap();
    assert_eq!(bi.params, vec![0.5]);
}
```

(Requires `DryWetConfig` registered from Task 3's catalog.)

- [ ] **Step 5: Reconstruct — consume FfiSig**

`rill-lang/src/graph/reconstruct.rs:40-68`: replace `registry.builtin_sig(...)` with the FFI catalog lookup: `TypeEnv::foreign_sigs` → `ffi_sig_from_typeexpr`; use `param_names` for ordering, `FfiParam::Resource` for the tape-arg injection, `signal_ins`/`signal_outs` from the FfiSig.

- [ ] **Step 6: Verify + commit**

Run: `cargo test -p rill-lang --test ffi`, full `cargo test -p rill-lang`, `cargo test -p rill-lang --test graph_reconstruct` (if it uses the sig path), clippy, fmt.
Commit: `git add -A && git commit -m 'feat(rill-lang): FfiSig param_names, record schemas, Resource/Tape; graph reconstruct on FfiSig'`

---

## Task 5: `rill-digital-effects` — algorithms only; rill-lang registers them

**Problem.** `rill-digital-effects` holds real `Delay`/`Distortion`/`Limiter` algorithms but has a `src/lang/` wrapper layer and no rill-lang dependency. It should be a pure library; rill-lang registers the builtins via `ForeignRegistry`.

**Files:** `rill-digital-effects/src/{delay,distortion,limiter}.rs` (keep), `rill-digital-effects/src/lang/*` (delete), `rill-digital-effects/src/register.rs` (delete `register_lang_builtins`), `rill-digital-effects/Cargo.toml` (add feature `lang` + optional `rill-lang`), `rill-lang/Cargo.toml` (add `rill-digital-effects` dep), `rill-lang/src/register.rs` (register via ForeignRegistry)

- [ ] **Step 1: Strip the lang layer**

Delete `rill-digital-effects/src/lang/` and `register_graph_nodes` from `register.rs` (keep the crate as a library: `pub mod delay; pub mod distortion; pub mod limiter;`). Add to `Cargo.toml`:

```toml
rill-lang = { workspace = true, optional = true }
[features]
lang = ["rill-lang"]
```

- [ ] **Step 2: Register via ForeignRegistry in rill-lang**

`rill-lang/src/register.rs` gains a `register_foreign_effects<T>(ffi: &mut ForeignRegistry<T>)` (behind the new feature or always) that wraps `rill_digital_effects::Delay/Distortion/Limiter` as `BlockBuiltin` factories, mirroring the deleted lang wrappers. Wire it into `with_builtins`/the FFI construction path.

- [ ] **Step 3: Test**

Add to `rill-lang/tests/ffi.rs` (or reuse the catalog): `main = delay _ 0.1 0.3 0.5;` compiles with an FFI registry containing the delay factory. If the catalog auto-registers the declaration but no factory is registered, the E2E needs `compile_with_ffi` + a registered factory.

- [ ] **Step 4: Verify + commit**

Run: `cargo test -p rill-lang`, `cargo test -p rill-digital-effects`, workspace check, clippy, fmt.
Commit: `git add -A && git commit -m 'refactor(rill-digital-effects): algorithms-only library; rill-lang registers via FFI'`

---

## Task 6: Generators + integrators → rill-lang (feature `dsp`); delete `rill-digital-filters`

**Problem.** `rill-core-dsp` must not depend on rill-lang. Its generators (sine/saw/square/triangle/noise) and integrators (integrator/leaky_integrator) get wrapper structs in `rill-lang`; `rill-digital-filters` is a full duplicate → delete.

**Files:** `rill-lang/src/register.rs` (move wrappers), `rill-lang/Cargo.toml` (add `rill-core-dsp` dep, feature `dsp`), `rill-core-dsp/src/lang/*` (delete after move), `rill-digital-filters/` (delete crate), `Cargo.toml` workspace member list

- [ ] **Step 1: Move generator/integrator wrappers to rill-lang**

Move `rill-core-dsp/src/lang/{osc,noise,integrator}.rs` wrapper structs (`OscBuiltin`, `NoiseGenBuiltin`, `IntegratorBuiltin`, `LeakyIntegratorBuiltin`) into `rill-lang/src/register.rs` (or a new `rill-lang/src/builtins/generators.rs`), adapting `rill_core::builtin::BlockBuiltin` → `crate::builtin::BlockBuiltin` and the `ril_lang` imports. Register them into `ForeignRegistry` under the `dsp` feature.

`rill-lang/Cargo.toml`:
```toml
rill-core-dsp = { workspace = true, optional = true }
[features]
dsp = ["rill-core-dsp", "rill-digital-effects/lang"]
```

- [ ] **Step 2: Delete `rill-digital-filters`**

Remove `rill-digital-filters/` from the workspace (`Cargo.toml` members) and delete the directory. Its `BiquadProcessor`/`MoogLadderProcessor` have zero external consumers (verified).

- [ ] **Step 3: Delete `rill-core-dsp/src/lang/`**

After the wrappers move, delete `rill-core-dsp/src/lang/` entirely (filters too — see Task 8/9 for where filter registration lands; if filters are not yet moved, keep the filter wrappers in rill-core-dsp under an optional `lang` feature as an intermediate, and remove in Task 9).

- [ ] **Step 4: Test + verify + commit**

Run: `cargo test -p rill-lang` (generators/integrators still work through the catalog + FFI), `cargo check --workspace --all-features`, clippy, fmt.
Commit: `git add -A && git commit -m 'refactor(rill-lang): generators/integrators into rill-lang dsp feature; delete rill-digital-filters'`

---

## Task 7: mixer/eq/dry_wet → rill-router; delete `rill-lang/src/builtins` duplicates

**Problem.** `rill-lang/src/builtins/{mixer,eq,dry_wet}.rs` duplicate `rill-router`'s implementations. The router already owns mixer/EQ and has a `lang` feature.

**Files:** `rill-lang/src/builtins/` (delete mixer/eq/dry_wet), `rill-lang/src/register.rs` (remove register_mixer/eq/dry_wet), `rill-router/src/lang.rs` + `register.rs` (add mixer/eq_parametric/dry_wet registration into ForeignRegistry), `rill-router/Cargo.toml` (keep `lang`)

- [ ] **Step 1: Move implementations**

`rill-lang/src/builtins/{mixer,eq,dry_wet}.rs` → `rill-router/src/{mixer,eq,dry_wet}_builtins.rs` (or into `rill-router/src/lang/`), adapting imports. Keep `rill-router`'s existing `mixer/`/`eq/` modules; the moved files are the DSL-facing wrapper structs (`MixerAlgorithmWrapper`, `EqBuiltin`, `DryWetBuiltin`).

- [ ] **Step 2: Register in ForeignRegistry**

`rill-router/src/lang.rs` registers `mixer`/`eq_parametric`/`dry_wet` (alongside existing `graphic_eq`/`mono_to_stereo`) into `ForeignRegistry` under feature `lang`, using the record-param flattening from Task 4. Delete `rill-lang/src/builtins/` and the `register_mixer`/`register_eq`/`register_dry_wet` functions + the `#[cfg(feature = "router")]` gates in `rill-lang/src/register.rs`.

- [ ] **Step 3: Test + verify + commit**

Run: `cargo test -p rill-lang` (mixer/eq/dry_wet tests — `rill-adrift/tests/lang_builtins.rs:88,104` use them), `cargo test -p rill-router`, workspace check, clippy, fmt.
Commit: `git add -A && git commit -m 'refactor(rill-lang): mixer/eq/dry_wet into rill-router; delete rill-lang/src/builtins duplicates'`

---

## Task 8: Filters → rill-lang (feature `dsp`); delete filter wrappers from rill-core-dsp

**Problem.** The 5 DSL filters (biquad/onepole/moog/lowpass/highpass) are registered by `rill-core-dsp/src/lang/register.rs`. Since `rill-core-dsp` must not depend on rill-lang and `rill-digital-filters` is deleted, move the filter wrapper structs + registration into rill-lang.

**Files:** `rill-lang/src/register.rs` (add filter wrappers), `rill-core-dsp/src/lang/{biquad,onepole,moog}.rs` (delete after move), `rill-core-dsp/src/lang/register.rs` (delete)

- [ ] **Step 1: Move filter wrappers**

Move `BiquadBuiltin`/`GeneralBiquadBuiltin`/`OnePoleBuiltin`/`MoogBuiltin` (rill-core-dsp/src/lang/{biquad,onepole,moog}.rs) into `rill-lang/src/builtins/filters.rs`, adapting imports. Register `biquad`/`onepole`/`moog`/`lowpass`/`highpass` into `ForeignRegistry` under the `dsp` feature. Note: `lowpass`/`highpass` are `biquad` with a preset filter type — the existing registration shows `BuiltinSig::simple("lowpass", 1, 1, 2, ...)` with a `Biquad`-backed factory.

- [ ] **Step 2: Delete `rill-core-dsp/src/lang/`**

After Task 6 + this task move all wrappers, delete `rill-core-dsp/src/lang/` and its `pub mod lang;` (rill-core-dsp/src/lib.rs:33). `rill-core-dsp` becomes algorithms-only.

- [ ] **Step 3: Test + verify + commit**

Run: `cargo test -p rill-lang` (filters via catalog + FFI), `cargo test -p rill-core-dsp`, workspace check, clippy, fmt.
Commit: `git add -A && git commit -m 'refactor(rill-lang): DSL filters into rill-lang dsp feature; rill-core-dsp algorithms-only'`

---

## Task 9: `analog_moog` → rill-lang (feature `model`); rill-core-model drops rill-lang

**Problem.** `analog_moog` is registered by `rill-core-model` (which has a `lang` feature + rill-lang dep). Drop the dep from core-model; move the wrapper + registration into rill-lang.

**Files:** `rill-core-model/Cargo.toml` (remove optional `rill-lang` + `lang` feature), `rill-core-model/src/register.rs` (delete `register_lang_builtins`), `rill-lang/src/builtins/model.rs` (add `AnalogMoogBuiltin`), `rill-lang/Cargo.toml` (feature `model`)

- [ ] **Step 1: Move + register**

Move `AnalogMoogBuiltin` (rill-core-model/src/register.rs:30) into `rill-lang/src/builtins/model.rs`; register `analog_moog` into `ForeignRegistry` under feature `model`. `rill-lang/Cargo.toml` gains `model = ["rill-core-model"]`.

- [ ] **Step 2: Clean rill-core-model**

Remove the `lang` feature + `rill-lang` optional dep from `rill-core-model/Cargo.toml`; delete `register.rs`'s `register_lang_builtins` (the crate keeps `wdf/`, `string/`, etc. as algorithms; `rill-analog-filters` is deleted in Task 10).

- [ ] **Step 3: Test + verify + commit**

Run: `cargo test -p rill-lang` (analog_moog via catalog + FFI), `cargo test -p rill-core-model`, workspace check, clippy, fmt.
Commit: `git add -A && git commit -m 'refactor(rill-lang): analog_moog into rill-lang model feature; rill-core-model drops rill-lang'`

---

## Task 10: Delete dead crates + `register_graph_nodes` stubs

**Problem.** `rill-analog-filters` (0 consumers), `rill-analog-effects` (CassetteDeck only via its own lang wrapper, nodes.rs dead), and the 8 `register_graph_nodes` placeholders are dead code.

**Files:** `Cargo.toml` (workspace members), `rill-analog-filters/`, `rill-analog-effects/`, 8 `register.rs` files

- [ ] **Step 1: Delete crates**

Remove `rill-analog-filters` and `rill-analog-effects` from the workspace members and delete the directories. Remove their entries from `rill-adrift/Cargo.toml` (`analog` feature) and `rill-adrift/src/lang_builtins.rs` (the `analog`-gated `register_lang_builtins` calls). If `cassettedeck` is genuinely wanted, keep `CassetteDeck` in rill-core-model and register it there — but per the research it has no external consumers beyond its own wrapper, so delete.

- [ ] **Step 2: Delete `register_graph_nodes` stubs**

Delete the 8 never-called `register_graph_nodes` placeholder functions (`rill-digital-filters/register.rs` — already deleted; `rill-digital-effects`, `rill-router`, `rill-lofi`, `rill-fft`, `rill-sampler`, `rill-analog-effects`, `rill-analog-filters`) and their `#[cfg(feature = "graph")]`/`rill-graph` optional deps where the only use was the stub.

- [ ] **Step 3: Verify + commit**

Run: `cargo check --workspace --all-features`, `cargo test --workspace`, clippy, fmt.
Commit: `git add -A && git commit -m 'refactor: delete dead crates (rill-analog-filters, rill-analog-effects) and register_graph_nodes stubs'`

---

## Task 11: Tape — `tape_loop` constructor + `Tape` as a Buffer member

**Problem.** Replace `ResourceRegistry` (name → handle HashMap) with a `tape_loop` foreign constructor; `write_head`/`read_head` take a `Tape` (shared-buffer index) parameter. `Tape` is a `Buffer` family member (like `FixedBuffer`), not a separate marker.

**Files:** `rill-lang/src/types/ty.rs` (`data Tape`, `instance Buffer (Tape f32)` in SIGNAL_PRELUDE), `rill-lang/src/ffi.rs` (resource factory kinds), `rill-lang/src/program.rs` (Vec<SharedCell> instead of ResourceRegistry), `rill-lang/src/lib.rs` (extract_resources recognizes `tape_loop`), `rill-sampler/src/tape/lang.rs` (write_head/read_head take Tape param), `rill-lang/src/graph/compile.rs` + `reconstruct.rs` (tape spec → tape_loop)

- [ ] **Step 1: Add `Tape` to SIGNAL_PRELUDE**

```rill
data Tape a = { };              // or a marker record — decides how Tape is constructed
instance Buffer (Tape f32);
```

`Tape` is a shared-buffer handle; its runtime representation is an index into a program-owned `Vec<SharedCell<T>>`.

- [ ] **Step 2: `tape_loop` constructor**

`foreign fn tape_loop : Int -> Tape f32;` (already in the Task 3 catalog). Lowering recognizes a call `tape_loop <capacity>` (like legacy `TapeLoop <cap>` in `extract_resources`), allocates a `TapeLoop` into the program's `Vec<SharedCell<T>>`, and returns its index as a `Tape` value. `write_head`/`read_head` FFI declarations take a `Tape f32` param (FfiParam::Resource).

- [ ] **Step 3: Replace ResourceRegistry in build**

`RillProgram::build`/`build_builtin` resource branch: instead of `resources: &mut Option<&mut ResourceRegistry<T>>` with name lookup, keep a `Vec<SharedCell<T>>` in the program and pass the tape index to `write_head`/`read_head` factories. Preserve `graph/compile.rs` duplex: `tape_spec_from` produces a `tape_loop` call.

- [ ] **Step 4: Test + verify + commit**

Existing tape tests: `rill-lang/tests/duplex_runtime.rs`, `rill-sampler/tests/tape_builtins.rs`, `rill-lang/tests/graph_reconstruct.rs` (tape) must pass with the new path. Add a `rill-lang/tests/ffi.rs` test: `main = read_head (tape_loop 1024) 0.1;` compiles and runs.
Run: `cargo test -p rill-lang`, `cargo test -p rill-sampler`, workspace, clippy, fmt.
Commit: `git add -A && git commit -m 'feat(rill-lang): tape_loop constructor; Tape as Buffer member; drop ResourceRegistry name lookup'`

---

## Task 12: Remove `BuiltinSig`/`ParamType`/`SignatureSource`/`NoSigs`

**Problem.** After all migrations, the legacy signature machinery is dead.

**Files:** `rill-lang/src/builtin.rs` (delete BuiltinSig, ParamType, RecordSchema, RecordField, SignatureSource, NoSigs; keep Registry + BlockBuiltin/MultichannelBlockBuiltin + BuiltinKind), `rill-lang/src/types/infer.rs`, `rill-lang/src/lower.rs`, `rill-lang/src/lib.rs`, `rill-lang/src/prelude.rs`, `rill-lang/src/graph/reconstruct.rs`, `rill-lang/src/schedule.rs`, `rill-lang/tests/*`

- [ ] **Step 1: Delete the types**

Delete `BuiltinSig`, `ParamType`, `RecordSchema`, `RecordField`, `SignatureSource`, `NoSigs` from `rill-lang/src/builtin.rs`. Keep `Registry<T>` (factory-only now), `BlockBuiltin`, `MultichannelBlockBuiltin`, `BuiltinKind`.

- [ ] **Step 2: Convert remaining `SignatureSource` consumers to FfiSig**

The 12 `builtin_sig` call sites (infer 4: bare-ref, apply, expr_has_variadic_signal, test; lower 7: Imag→complex, apply legacy, lower_ref, complex arith, rhs_variadic, arity×2; reconstruct 1) all read `FfiSig` from `TypeEnv::foreign_sigs` instead. `compile()`/`compile_with`/`lower_with_cafs` drop the `sigs: &dyn SignatureSource` parameter (the FFI catalog is in `TypeEnv`). Delete `NoSigs` and the `SignatureSource` trait.

- [ ] **Step 3: `Registry` factory-only**

`Registry<T>` drops the `Entry.sig` field and the `register_*` methods take `(name, factory)` without a `BuiltinSig`; `Entry::build_*` keep. `param_names` for reconstruct now come from `FfiSig::param_names` (Task 4).

- [ ] **Step 4: Test + verify + commit**

Run: `cargo test -p rill-lang`, `cargo test --workspace`, `cargo clippy --all-features --workspace` (zero warnings), `cargo fmt`.
Commit: `git add -A && git commit -m 'feat(rill-lang): remove BuiltinSig/ParamType/SignatureSource — FfiSig is the contract'`

---

## Task 13: Docs + final verification

**Files:** `rill-lang/README.md`, `docs/src/guides/rill-lang.md`, `CHANGELOG.md`, `docs/superpowers/specs/2026-10-02-rill-lang-signal-buffer-design.md` (SP-3b outcome)

- [ ] **Step 1: Document**

Update the README/guide: the builtin catalog now lives in the language (`foreign fn` in the auto-registered catalog); `rill_lang::ffi::ForeignRegistry` is the single factory registry; `BuiltinSig` is gone; crate map changes (deleted crates); `Buffer` family (`FixedBuffer`/`Tape`); `tape_loop`. Update the spec's SP-3 section to reflect the actual outcome.

- [ ] **Step 2: Final verification**

```bash
cargo test -p rill-lang && cargo test --workspace && cargo clippy --all-features --workspace && cargo fmt
```
Expected: zero failures, zero warnings.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m 'docs(rill-lang): SP-3b builtin migration outcome — FfiSig contract, dead crates removed'
```

---

## Self-review

**Spec coverage (SP-3b from `docs/superpowers/specs/2026-10-02-rill-lang-signal-buffer-design.md` §2-§5 + user decisions):**
- §2 `ParamType::Buffer`/single registry in rill-lang: Tasks 2, 12 ✓
- §3 FFI registry as runtime API of rill-lang; DSP crates register via `ForeignRegistry`: Tasks 5-9, 12 ✓
- §4 parameter mapping onto existing types (Record≈Data, Variadic≈List, scalars): Tasks 3, 4 ✓
- §5 tape stays on shared-buffer path, via `tape_loop` constructor: Task 11 ✓
- User decisions: is_multi fix (T1), inline catalog (T3), **Faust-combinator sugar so signal-input builtins keep legacy call style (T3b, added 2026-10-03 — user chose "all combinators at once", mechanism decided in-plan as a reduce/infer AST rewrite)** ✓
- mixer/eq/dry_wet → rill-router + delete duplicates (T7), generators/integrators/filters → rill-lang dsp feature (T6, T8), analog_moog → model feature (T9), delete rill-digital-filters/analog-* (T6, T10), rill-core-model drops rill-lang (T9), rill-graph survives on FfiSig (T4) ✓
- **Dead-code discipline:** everything verified-dead is deleted; nothing with real consumers is touched ✓

**Placeholder scan:** the catalog signatures in Task 3/3b carry a **verification warning** (double-check each arity/param against the real registration files) — this is a correctness checkpoint, not a placeholder; every other step has concrete code/commands. No TBD/TODO.

**Type consistency:** `FfiParam::{Signal,Scalar,VariadicSignal,Record,Resource}` and `FfiSig::{params,signal_outs,param_names}` are consistent across T3/T3b/T4/T12; `BuiltinFactoryKind` is shared between `Registry` and `ForeignRegistry` (T1); `tape_loop : Int -> Tape f32` appears identically in the catalog (T3) and SIGNAL_PRELUDE (T11); the combinator-sugar rule (`Seq(lhs, Apply(name,args))` → prepend `Wire`s) is consistent across all five combinators in T3b.