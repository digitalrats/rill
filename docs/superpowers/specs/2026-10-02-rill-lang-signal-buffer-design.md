# rill-lang: Signal Track as First-Class Buffers — FFI Layer Design (SP-3)

> **Status:** Implemented (SP-3a + SP-3b, 2026-10-03). See
> `docs/superpowers/plans/2026-10-02-rill-lang-sp3b-builtin-migration.md`.
> **Date:** 2026-10-02
> **Branch:** `feature/rill-lang-categories`
> **Scope:** SP-3 of the categories work. Introduce an **FFI layer in rill-lang**:
> builtin algorithm signatures are declared in the language itself
> (`foreign fn name : FixedBuffer f32 -> ...`) over a first-class buffer type
> (`FixedBuffer a` / `Buffer`), and the Rust-side builtin registry becomes a
> runtime API of rill-lang that holds only factories. `BuiltinSig`/`ParamType`
> are removed. Execution system and backend interaction do NOT change.

---

## 0. Executive summary

rill-lang has two execution tracks: the **block track** (`Rate::Signal`, whole-buffer
`Instr`s over `FixedBuffer<T, BUF>` block registers) and the **value track**
(`Rate::Value`, arena values + `ValueInstr`, once per tick). `main` is either/or
(`lower.rs:4664`). The two tracks never mix.

The builtin algorithm catalog is currently described by `BuiltinSig`/`ParamType`
in `rill-core` (channel counts, not types). SP-3 replaces that contract with an
**FFI layer inside rill-lang**: signatures live in the language, Rust provides
only implementations. This is the first step toward the long-term goal of
removing dedicated signal channels entirely in favor of parameters
(program `(arg..) -> IO`).

| Piece | What | Where |
|---|---|---|
| `foreign fn` declarations | Builtin signatures written in rill types (`foreign fn biquad : FixedBuffer f32 -> Float -> Float -> Float -> FixedBuffer f32`) | `parser.rs`, `ast.rs`, `reduce.rs` |
| `FixedBuffer a` / `Buffer` | First-class type name for the single signal-channel kind | `SIGNAL_PRELUDE` + type registry |
| FFI registry | Runtime API of rill-lang holding name → factory; the only source of signal signatures | `rill_lang::ffi` |
| Factory contract | Rust-side `&[f64]` + sample_rate → `Box<dyn Algorithm<T>>`, defined in rill-core | `rill_core` types |
| `BuiltinSig`/`ParamType` | **Removed** from `rill-core`; replaced by language-side FFI declarations | `rill_core::builtin` |

Explicitly **out of scope** for SP-3: sample access from the value track, the
"big objects" memory subsystem, `Array`/`Vector` collection types, `List` →
cons-list refactor, `Buffer a b` as a first-class arrow value, and the tape
builtins (see §5). These are the next stages (separate specs).

---

## 1. The FFI layer in rill-lang

**Principle: the language owns its foreign contract.** A builtin algorithm's
signature is declared in rill-lang; Rust crates provide implementations matched by
name. This inverts the current model where `rill_core::BuiltinSig` describes the
signature *for* the language.

**Syntax (carried arrows, parsed by the existing type parser):**

```rill
// SIGNAL_PRELUDE + user code
foreign fn biquad : FixedBuffer f32 -> Float -> Float -> Float -> FixedBuffer f32;
foreign fn sine   : Float -> Float -> Float -> FixedBuffer f32;
foreign fn mixer  : List (FixedBuffer f32) -> MixerConfig -> (FixedBuffer f32, FixedBuffer f32);
```

- `foreign fn name : TypeExpr;` — a top-level declaration. `name` becomes a
  resolvable name whose type is the carried `TypeExpr`.
- `FixedBuffer f32` is the signal-channel parameter (see §2). `Float`/`Int`/`Bool`/
  `String` are compile-time scalar parameters. `List (FixedBuffer f32)` is a variadic
  signal-channel list. A record literal type (`MixerConfig`) is a `Data`-typed
  record parameter (see §4).
- The declaration registers into the type environment (like a builtin name with a
  known signature) and the FFI registry (runtime factory binding).

**AST.** `Def::Foreign { name: String, sig: TypeExpr, span: Span }` — a new `Def`
variant, registered in `TypeEnv` alongside typeclass/instance/data declarations.

**Inference.** A `Ref`/`Apply` to a foreign name types through the declared
signature: each `Buffer` argument is a signal channel (`Rate::Signal`, contributes
to input arity), each scalar argument is a compile-time constant parameter, the
result `Buffer`s are signal outputs. Arity synthesis is unchanged from today's
`ParamType::Signal` counting — only the source of truth moves from
`rill_core::BuiltinSig` to the language declaration.

**Lowering.** A foreign call emits `Instr::CallBlock` exactly as today. The
runtime FFI registry maps the name to its factory; the language-side signature
provides the arity/param info that `BuiltinInstance` needs. No IR change.

---

## 2. `FixedBuffer a` / `Buffer` — the single signal-channel type

**The signal wire is strictly `FixedBuffer[BUF]`** — there is one signal-channel
kind in rill today (a block of `BUF` samples, `n ≤ BUF` processed per tick), and
there will not be another for some time. `Buffer` is its language-level type name.

```rill
typeclass Buffer b where { }      // skeleton; base ops come in later stages
instance Buffer (FixedBuffer a);
```

- `FixedBuffer a` — builtin type, kind `* -> *`; `a` is the element scalar type
  (`f32`/`f64`), the size `BUF` is implicit (one size for all). Registered in the
  type registry (`ctor_kinds`/`data_types`) like `List`/`Maybe`.
- `Buffer` — typeclass over buffer types, `instance Buffer (FixedBuffer a)`. In
  SP-3 it is a **skeleton** (no methods yet); the base operations and the Faust
  combinator surface arrive in later stages.
- Declared in **`SIGNAL_PRELUDE`** — a second prelude in `rill-lang/src/types/ty.rs`
  alongside `CATEGORY_PRELUDE`, registered by `with_builtins`. This is the future
  home of the Faust combinators (`:`, `,`, `<:`, `~`, `@`) and buffer math.

---

## 3. The FFI registry — runtime API of rill-lang

`BuiltinSig`/`ParamType`/`SignatureSource` are **removed** from `rill-core`. The
signal-signature contract moves into rill-lang:

- `rill_lang::ffi` — a registry of `name → factory`, where a factory is
  `&[f64] + sample_rate → Box<dyn Algorithm<T>>` (or the multichannel variant).
  The factory types (`Algorithm`, `MultichannelAlgorithm`, `Transcendental`)
  remain defined in `rill-core`; only the signature description is removed.
- Registration is a runtime API: `rill_lang::ffi::register::<T>(name, factory)`,
  called by `rill-adrift` (the aggregator) and by user code. The signature comes
  from the language declaration, not from the registration.
- `RillProgram::build` (program.rs:236) resolves a foreign call by name → factory
  from the FFI registry; arity/params come from the compiled language signature.

**Cross-crate consequence:** DSP crates stop depending on a signature type. They
export factory registrations through a rill-core-defined contract
(`register_factories`) with **no rill-lang dependency** — the same direction as
today (DSP crates → rill-core only). `rill-adrift` and user code bind the
factories into `rill_lang::ffi` and provide the language declarations.

---

## 4. Parameter type mapping (existing language types)

The old `ParamType` variants map onto **existing rill type constructs** — no new
type-level machinery is required:

| `ParamType` (old) | FFI signature type (rill) | Notes |
|---|---|---|
| `Signal` | `FixedBuffer f32` | Signal channel, strictly `FixedBuffer[BUF]` |
| `Float` | `Float` | Compile-time constant |
| `Int` | `Int` | Compile-time constant |
| `String` | `String` | Compile-time string |
| `Bool` | `Bool` | Compile-time bool |
| `Variadic(Signal)` | `List (FixedBuffer f32)` | Variadic signal channels |
| `Record(schema)` | a `Data` record type | `data MixerConfig = { ... }` + record literal; Record is semantically `Data` |
| `Enum` | (unused today) | not needed for the current catalog |
| `Resource` | — | **tape builtins stay outside FFI** (§5) |

**Why no new types:** `TypeExpr` is `TName | TApp | TFunc` (ast.rs:16-22);
records are declared as `data` types and constructed with record literals, and
`List` is already a builtin `App("List", [t])`. Variadic signal channels are
expressed as `List (FixedBuffer f32)`, which the existing variadic signal path
(`infer.rs:3371`) already handles.

---

## 5. Tape builtins (`write_head`/`read_head`) stay outside FFI

The tape heads implement **one writer + many readers sharing one `TapeLoop`**
buffer. Today this is bound by name through `ResourceRegistry<T>`
(`rill-core/src/buffer/registry.rs`): the DSL declares `tape = TapeLoop <cap>`,
the compiler allocates the buffer once and hands a `SharedWriter`/`SharedReader`
pair to the heads via `registry.writer(name)` / `registry.reader(name)`
(`rill-sampler/src/tape/lang.rs:28-54`), both referencing one `SharedCell`.

**Decision (SP-3b outcome):** the tape heads moved onto the FFI layer through a
`tape_loop` foreign constructor (`Int -> Tape f32`). `Tape` is a `Buffer`
family member whose runtime representation is an index into a program's shared
tape cells; `write_head`/`read_head` take it as a `Resource` param. A NAMED
binding (`tape = tape_loop <capacity>`) creates one cell shared by every
`Ref(tape)`; an INLINE `tape_loop <capacity>` allocates a fresh cell per call.
The legacy name-based `ResourceRegistry` path remains only for the graph-duplex
compile path (SP-3b Task 11).

- `write_head`/`read_head` are catalog FFI builtins (`rill-sampler` registers
  their factories as `register_resource_*`), **inside** the FFI layer.
- The FFI layer covers the remaining ~38 builtins (all `Signal`/`Float`
  signatures, plus `mixer`/`eq_parametric`/`dry_wet` via `List`/`Data` types).
- The single shared cell per named tape is not a workaround for memory
  management: in the static dataflow model, liveness = program lifetime and the
  buffer is allocated at build time. A true "resource with automatic release"
  (`use`/refcount) only becomes meaningful in Turing-complete programs — the
  future `(arg..) -> IO` refactor, where buffers become values with managed
  lifetimes. The tape heads move there and `Resource` dies naturally.

---

## 6. Relationship to the long-term goal

The end goal is **removing dedicated signal channels entirely in favor of
parameters**: a program is `(arg..) -> IO`, streams are built-in language
entities, backend interaction flows through the stream concept. SP-3 is the first
step:

- Signal channels become named (`FixedBuffer`), so the type system can talk about
  them uniformly.
- The builtin contract moves into the language, so the language (not `rill-core`)
  owns the vocabulary in which the future `(arg..) -> IO` program is typed.
- `FixedBuffer` as a parameter is the seed of "buffers are values"; the tape
  `TapeLoop`-as-parameter scheme and the ST-style variant build on this later.

---

## Files (SP-3)

| Area | Files |
|---|---|
| `foreign fn` syntax | `parser.rs` (`parse_foreign_def`), `ast.rs` (`Def::Foreign`), `render.rs`, `reduce.rs` |
| Type registration | `types/ty.rs` (`SIGNAL_PRELUDE`, foreign sig table, `FixedBuffer`/`Buffer` types), `types/infer.rs` (foreign-name typing, `Buffer` as signal channel), `types/unify.rs` (`Buffer` unification) |
| Lowering | `lower.rs` (foreign call → `CallBlock`; `FixedBuffer` signal args; `List (FixedBuffer f32)` variadic) |
| FFI registry | `rill_lang::ffi` (new module), `program.rs` (`RillProgram::build` factory resolution) |
| rill-core cleanup | `rill_core/src/builtin.rs` — remove `BuiltinSig`/`ParamType`/`SignatureSource`; keep `Algorithm`/`BlockBuiltin`/`MultichannelBlockBuiltin`/`Registry`(factory-only) |
| DSP crates | ~10 crates' `lang/register.rs` — factory-only registrations, drop `BuiltinSig` |
| Docs | `rill-lang/README.md`, `docs/src/guides/rill-lang.md`, `CHANGELOG.md` |
| Tests | `rill-lang/tests/*` (ffi, buffer, typeclass, arrow, collections regression) |

## Verification

- `cargo test -p rill-lang` after each task; `cargo test --workspace`,
  `cargo clippy --all-features --workspace`, `cargo fmt` before finishing.
- Zero new external dependencies. No `unsafe`.
- Regression: existing signal programs (combinators, builtin calls, tape) compile
  unchanged — the block track and `ResourceRegistry` are untouched.
- The ~38 FFI-covered builtins keep their exact current arity/param behavior.
- Branch: `feature/rill-lang-categories`; conventional commits, single-quoted `-m`.
