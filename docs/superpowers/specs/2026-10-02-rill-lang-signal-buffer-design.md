# rill-lang: Signal Track as First-Class Buffers (`FixedBuffer` / `Buffer`) — Design

> **Status:** Approved (user, 2026-10-02).
> **Date:** 2026-10-02
> **Branch:** `feature/rill-lang-categories`
> **Scope:** SP-3 of the categories work. Make the signal track a first-class,
> nameable abstraction in rill-lang: a builtin `FixedBuffer a` type (reifying a
> signal channel) and a `Buffer` typeclass with `instance Buffer (FixedBuffer a)`,
> plus a replacement of the signal-argument surface of builtin algorithm
> signatures with Buffer-typed parameters. Execution system and backend
> interaction do NOT change.

---

## 0. Executive summary

rill-lang has two execution tracks: the **block track** (`Rate::Signal`, whole-buffer
`Instr`s over `FixedBuffer<T, BUF>` block registers) and the **value track**
(`Rate::Value`, arena values + `ValueInstr`, once per tick). `main` is either/or
(`lower.rs:4664`). The two tracks never mix: values can materialize into block
registers (constants, main cells, params, macros), but a block signal is never
observable as a value, and there is no per-sample access from the value track.

SP-3 makes the signal track **nameable in the type system** without touching the
execution engine:

| Piece | What | Where |
|---|---|---|
| Type-level `Scalar` moved to `rill-core` | `Scalar` (enum `Int | Float | Var`) lives in `rill-core`, re-exported by rill-lang | `rill_core::types`, `rill_lang::types` |
| `ParamType::Buffer` | New signal-channel parameter type in `BuiltinSig` | `rill_core::builtin` |
| `SIGNAL_PRELUDE` | New prelude: `FixedBuffer a` type + `Buffer` typeclass | `rill-lang/src/types/ty.rs` |
| Buffer-typed algorithm signatures | Inference/lowering read `Buffer` as a signal channel (`Rate::Signal`) | `types/infer.rs`, `lower.rs` |

Explicitly **out of scope** for SP-3: sample access from the value track, the
"big objects" memory subsystem, `Array`/`Vector` collection types, `List` →
cons-list refactor, and `Block a b` as a first-class arrow value. These are the
next stages (separate specs).

---

## 1. Move `Scalar` to rill-core

**Problem.** `rill_lang::types::Scalar` (`rill-lang/src/types/ty.rs:33-40`) is the
type-level element descriptor of a sample: `Int | Float | Var(TypeVarId)`. It lives
in rill-lang, but the builtin signature layer — which needs to describe typed signal
channels — lives in `rill_core::builtin` (`BuiltinSig`, `ParamType`). rill-core
cannot depend on rill-lang, so a typed buffer parameter would be impossible today.

**Also:** rill-core has only `rill_core::math::Scalar` — a **trait** (numeric bound,
`num.rs:8`), not a type-level enum. It serves runtime arithmetic, not type
description. There is no type-level sample-type descriptor in rill-core.

**Change.**

- New module `rill_core::types`:
  ```rust
  /// The scalar (element) type of a sample, type-level.
  #[derive(Debug, Clone, PartialEq)]
  pub enum Scalar {
      /// Integer.
      Int,
      /// Floating point (the runtime `T`).
      Float,
      /// Unresolved unification variable.
      Var(u32),
  }
  ```
- rill-lang re-exports it: `rill_lang::types::ty` keeps `pub use rill_core::types::Scalar;`
  (or the `types/mod.rs` prelude re-exports it). All existing `use` sites keep
  working; `TypeVarId` remains `pub type TypeVarId = u32` in rill-lang
  (`ty.rs:21`), and `Scalar::Var` carries a raw `u32` in rill-core.

**Rationale.** Moves the type-level channel description to the same crate as the
builtin signature layer, unblocking `ParamType::Buffer(Scalar)` (or a scalar-free
`Buffer`) without a rill-core → rill-lang dependency, and prepares for the
"big objects" memory subsystem which will also live in rill-core.

**Decision point (resolved at plan time):** SP-3's `ParamType::Buffer` may be
scalar-free (marker: "typed signal channel", element type lives in rill-lang via
`FixedBuffer a`) — this is the recommended minimal form. A scalar-carrying
`Buffer(Scalar)` is possible after the move; the plan should pick one and keep it
consistent with `FixedBuffer a`.

---

## 2. `ParamType::Buffer` in `BuiltinSig`

**Problem.** A builtin's signal inputs are expressed as *channel counts*
(`ParamType::Signal`, `signal_outs: usize`), not as a typed buffer. There is no way
to express "this argument is a block buffer" or to type a builtin as
`Buffer f32 -> Float -> Float -> Float -> Buffer f32`.

**Change.**

- `rill_core::builtin::ParamType` gains:
  ```rust
  /// A signal wire argument that is a block buffer (typed signal channel).
  /// Contributes to the built-in's input arity, like `Signal`.
  Buffer,
  ```
- Counting helpers treat `Buffer` exactly like `Signal`:
  `signal_ins()` (`builtin.rs:138`), `has_variadic_signal()` (`:146`),
  `min_args()`/`max_args()` (`:153`/`:171`).
- `BuiltinSig::simple()` keeps emitting `ParamType::Signal` — the two are
  inference-equivalent (both count as signal inputs); `Buffer` is the explicit,
  typed spelling used by signatures that want to say "buffer channel".
- Inference (`types/infer.rs:3276`, `:3371`) and lowering (`lower.rs:3633`,
  `:3767`) treat `ParamType::Buffer` identically to `ParamType::Signal`:
  consume a block register, contribute to `signal_ins`.

**Why not scalar-carrying `Buffer(Scalar)`?** rill-core had no type-level `Scalar`
before Section 1. After the move it is available, but carrying it into every
`BuiltinSig::simple()` call adds noise for zero inference gain today (all builtin
signal channels are Float). The plan should default to scalar-free `Buffer` and
note `Buffer(Scalar)` as a future option if element-typed channels are needed.

**Registration surface.** ~40 `BuiltinSig::simple(...)` calls across 10 crates
(`rill-core-dsp`, `rill-digital-effects`, `rill-router`, `rill-analog-effects`,
`rill-fft`, `rill-core-model`, `rill-sampler`, `rill-lofi`, rill-lang's
`register.rs` + `lib.rs`) do not need to change: `simple()` keeps emitting
`Signal`, which remains inference-equivalent. Only hand-built
`BuiltinSig { params: vec![ParamType::Signal, ...] }` structs (rill-router
`mono_to_stereo`, rill-lang mixer, rill-sampler tape) may optionally switch to
`Buffer`; not required.

---

## 3. `SIGNAL_PRELUDE` — `FixedBuffer a` + `Buffer` typeclass

**Problem.** The signal track has no name in the language. Programs are arrows
over implicit block channels; the combinators (`:`, `,`, `<:`, `:>`, `~`, `@`)
are operators on the block track only, and there is no typeclass surface for
buffers.

**Change.** Introduce a second prelude in `rill-lang/src/types/ty.rs`, registered
by `with_builtins` the same way as `CATEGORY_PRELUDE`:

```rill
// SIGNAL_PRELUDE
typeclass Buffer b where { }
instance Buffer (FixedBuffer a);
```

- `FixedBuffer a` — builtin type, kind `* -> *`, reifying a signal channel. Its
  element type `a` is the scalar type of the block's samples. Declared either in
  `SIGNAL_PRELUDE` source or in the builtin type registry (`ctor_kinds` /
  `data_types`), mirroring how `List`/`Maybe` are registered — plan-time decision.
- `Buffer` — typeclass with the base buffer operations. In SP-3 the class is a
  **skeleton** (no methods yet, or a minimal marker set); the operations are
  defined in the next stages. `instance Buffer (FixedBuffer a)` is the concrete
  instance.
- Registration reuses `register_decls` + the prelude-only `Def::Data` pass
  pattern from SP-2 Task 9 (a `data`/builtin-type in the prelude registers into
  `data_types`/`ctor_kinds`/`data_arities`).

**Future growth.** `SIGNAL_PRELUDE` is the home for the Faust combinators
(`:`, `,`, `<:`, `~`, `@` as methods/functions) and buffer math in later stages —
the prelude is extended, not replaced. This is the "where the signal track gets a
language-level API" document.

---

## 4. Buffer-typed algorithm signatures

**Problem.** Today a builtin call `biquad ...` infers as
`ArrowTy::uniform(signal_ins, signal_outs, Scalar::Float)` (`infer.rs:3402`) — a
bare channel count. The signature cannot state that the signal argument is a
buffer.

**Change.** With `ParamType::Buffer` (Section 2) and `FixedBuffer a` (Section 3),
the type-level surface is:

- A builtin signature written with `ParamType::Buffer` for its signal inputs
  reads as `Buffer f32 -> ... -> Buffer f32` in the language's type vocabulary.
- Inference/lowering map `Buffer` → `Rate::Signal` channel exactly as `Signal`
  today: arity flows through the existing channel-count synthesis, scalar is
  `Scalar::Float`, and the call still lowers to `Instr::CallBlock`.
- Optionally, `SIGNAL_PRELUDE` or a registry comment documents the canonical
  spelling of the builtin catalog in Buffer terms (e.g. `biquad : Buffer f32 ->
  Float -> Float -> Float -> Buffer f32`), giving the catalog a typed,
  nameable surface without changing any registration.

**Non-goal:** passing a *buffer value* to a builtin as a first-class argument or
returning one is NOT in this increment (that is the "big objects" / `Block a b`
stage). SP-3 only types the existing whole-buffer call surface with the buffer
type name.

---

## 5. Out of scope (next stages, separate specs)

| Stage | Content |
|---|---|
| Sample access (bridge) | Block→value: instructions reading a block register / its samples into the value track; `length`/`index` over buffers from value code |
| "Big objects" subsystem | Refcounted heap-backed buffers in rill-core, allocated before processing, freed after (like `FixedBuffer`); unifies program blocks + algorithm internal buffers |
| `Array` / `Vector` + classes | New collection types with the canonical Haskell `Array` semantics; possibly `List` → proper cons-list (Haskell `List` performance guarantees) |
| SP-4: `Block a b` first-class arrow | Block transform as a first-class value (descriptor), `instance Arrow Block`, `do`-notation over blocks, HOF over blocks |
| Variant 3 (ST-style) | After the next refactor: program `(arg..) -> IO`, streams as built-in language entities, backend interaction through the stream concept |

---

## Files (SP-3)

| Area | Files |
|---|---|
| `Scalar` move | `rill-core/src/types.rs` (new) or `rill-core/src/types/mod.rs`; `rill-lang/src/types/ty.rs`, `rill-lang/src/types/mod.rs` (re-export) |
| `ParamType::Buffer` | `rill-core/src/builtin.rs`; optional `rill-core-dsp/.../register.rs` etc. for hand-built sigs |
| `SIGNAL_PRELUDE` | `rill-lang/src/types/ty.rs` (`SIGNAL_PRELUDE` const + `with_builtins` registration) |
| Inference/lowering | `rill-lang/src/types/infer.rs` (Signal-match sites), `rill-lang/src/lower.rs` |
| Docs | `rill-lang/README.md`, `docs/src/guides/rill-lang.md`, `CHANGELOG.md` |
| Tests | `rill-lang/tests/*` (buffer/typeclass/arrow additions) |

## Verification

- `cargo test -p rill-lang` after each task; `cargo test --workspace`,
  `cargo clippy --all-features --workspace`, `cargo fmt` before finishing.
- Zero new external dependencies. No `unsafe`.
- Regression: existing signal programs (combinators, builtin calls) compile
  unchanged — the block track is untouched.
- Branch: `feature/rill-lang-categories`; conventional commits, single-quoted `-m`.