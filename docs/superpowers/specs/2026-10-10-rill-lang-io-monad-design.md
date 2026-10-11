# rill-lang: Full IO Monad (Stage B IO) — Design

> **Status:** Design
> **Date:** 2026-10-10
> **Branch:** `feature/rill-lang-arrays`
> **Scope:** introduce the full IO monad into rill-lang. The program is an IO
> action that the runtime LEADS each tick (Haskell RTS model). This collapses
> Stage C's `(arg..) -> IO` program form into the current work.

---

## 0. Executive summary

rill-lang programs are currently `main : inputs -> outputs` typed over `Buffer`
arrows (B3-2). This design makes the program a **full IO monad**:

```haskell
main :: Buffer -> … -> IO ()
```

- `main`'s λ-parameters are the **input buffers**; outputs are written via
  `writeOutput : Int -> Buffer -> IO ()`; `readInput : Int -> IO Buffer` is also
  provided; `main` returns `()`.
- `IO a` is a real type with `instance Monad IO` (`return`/`bind`); `do`-notation
  works (it already desugars to `bind`).
- DSP functions are **pure** `Buffer -> Buffer` (`onepole : Float -> Float ->
  Buffer -> Buffer`); they compile to `ValueBuffer*` fragments; the runtime
  mutates the buffer in place while leading the IO action (the mutation is
  invisible outside the tick, like `runST`).
- Faust combinators (`:`, `,`, `<:`, `:>`, `~`, `@`) **desugar into ordinary
  functions**; `_`/`i1`/`i2` are ordinary λ-parameter NAMES — HM infers
  `Buffer` from usage, no wire special-casing.
- **Auto-desugar** keeps old programs working: `main = _ : onepole 200.0 0.7`
  becomes `main i1 = do { writeOutput 0 (onepole 200.0 0.7 i1); return () }` —
  buffer outputs become `writeOutput` calls, wire params become `Buffer` params.
- Runtime each tick: materialize inputs as `Value::Buffer` → lead the root
  fragment (the IO action) → drain outputs via `writeOutput`.

The computation path stays unified (B3-2): one value track, everything is a
closure, `run` leads the root fragment. `IoCapture`/`IoPlayback` are unchanged.

---

## 1. Current state (audited)

- `Monad` typeclass exists in `CATEGORY_PRELUDE` (`ty.rs:227`): `return`,
  `bind`; `instance Monad Maybe`, `instance Monad List`. `do`-notation desugars
  to `bind` in `parser.rs` (`name: "bind"`).
- `runST : (forall s. ST s a) -> a` is a builtin signature (`builtin_sigs`,
  `ty.rs:541`); `data ST s a = { unST: a }` in `CATEGORY_PRELUDE`. Rank-N
  application + escape check in `infer_apply_impl`. Lowering desugars
  `runST (fn s -> body)` to the body directly.
- B3-2: signal expressions lower to `ValueBuffer*` value-track ops over
  `Value::Buffer(usize)` handles backed by a program-level `RegionHandle`
  buffer table. `process()` materializes inputs into the buffer table, leads
  the value track, drains outputs from `value_output_regs`. Block track deleted.
- `main` is currently a signal expression (`main = <buffer-expr>`); its λ-params
  are control parameters (`main gain = …` → `main_cells`/`set_param`). The
  program entry/exit is `main : inputs -> outputs` typed over `Buffer`.
- Faust combinators are AST nodes (`Expr::Seq/Par/Split/Merge/Loop/Delay`),
  lowered by `lower_seq`/`lower_par`/… producing `ValueBuffer*` ops. `_`
  (`Expr::Wire`) is a special signal-input placeholder.

---

## 2. Design

### 2.1 `IO a` type + `instance Monad IO`

Add to `CATEGORY_PRELUDE` (or a new `IO_PRELUDE`):

```haskell
data IO a = { unIO: a };
instance Monad IO where {
    return x = IO { unIO: x };
    bind m f = IO { unIO: (f m.unIO).unIO };
};
```

- `IO a` is a record wrapping `a` — the value track already executes record
  construction/projection; no new `Value` variant needed.
- `return`/`bind` are ordinary `Monad` methods; `do`-notation works unchanged.
- The runtime never "runs" `bind` per se — `main` lowers to a root fragment
  whose value-track ops materialize/lead the computation, and `writeOutput`
  drains. The `IO` wrapper is a typing convention: the program's *type* is
  `IO ()`, its *computation* is the root fragment.

### 2.2 Program form: `main :: Buffer -> … -> IO ()`

`main` becomes a function whose λ-parameters are the program's INPUT buffers
(typed `Buffer`), body is an IO action, result `()`:

```haskell
main i1 i2 = do
    o1 <- return (onepole 200.0 0.7 i1)   -- or a let binding
    writeOutput 0 o1
    writeOutput 1 o2
    return ()
```

- The **entry/exit**: `process(inputs, outputs)` materializes `inputs` into the
  program's buffer table (as B3-2 does), calls `main` with the input buffer
  registers, leads the returned IO action's root fragment, and `writeOutput`
  writes into the external output slices.
- `readInput : Int -> IO Buffer` reads the k-th materialized input buffer.

### 2.3 FFI: one ordinary arrow `Buffer -> … -> Buffer`

FFI is an **ordinary first-class function**, dispatched through the SAME
`run_call` path as any user function. A foreign call compiles to a
**Callable whose fragment holds a sequence: extract the Buffer argument
registers from the fragment's local frame, then one `ValueCallForeign`** that
invokes the `Algorithm`:

```rust
ValueInstr::ValueCallForeign { dst: usize, srcs: Vec<usize>, instance: usize },
```

- Scalar params are NOT value registers: constants are folded into
  `BuiltinInstance.params` at build (currying — the arrow's scalar args are
  applied at construction), live `?name`/`read_param` params flow via
  `param_bindings` → `set_param` before `process`. The `Algorithm` stays a
  pure function over its baked params.
- The foreign call's fragment is `[ValueCallForeign { dst, srcs, instance }]`;
  applying it to a Buffer goes through `ValueCallFunc` → `run_call`. This makes
  the FFI arrow a first-class value, so **HOF works for FFI**: `map
  (onepole 200.0 0.7) buffers`, and mixer algorithms fold a mono kernel over a
  `List Buffer`.
- **Multichannel is collections, not a runtime mechanism.** All multichannel
  algorithms reduce to `List Buffer -> List Buffer` or `List Buffer -> Buffer`
  (`dry_wet`); `mono_to_stereo` is `Buffer -> Pair Buffer`. The signature
  expresses channels via `Pair (FixedBuffer f32)`/`List (FixedBuffer f32)`, and
  the value track handles them with ordinary collection ops. `Algorithm`
  stays **SISO** (`process(Option<&[T]>, &mut [T])` — zero overhead, existing
  algorithms untouched); multichannel is built from mono kernels + HOF on the
  language side.
- `MultichannelAlgorithm`, `MAX_BUILTIN_CHANNELS`, and the interleaved
  `Block` path are **deleted**.

### 2.4 Faust combinators → ordinary functions (desugar)

`Expr::Seq/Par/Split/Merge/Loop/Delay` desugar into ordinary function
applications at `desugar` time (the existing `desugar` pass):

| Combinator | Desugar |
|---|---|
| `A : B` | `compose a b` (the `Arrow` method) |
| `A , B` | `par a b` |
| `A <: B` | `split a b` |
| `A :> B` | `merge a b` |
| `A ~ B` | `feedback a b` |
| `A @ n` | `delay n a` |
| `_` | the first λ-parameter name (`i1`) |

Each becomes a call to a prelude function operating on `Buffer -> Buffer`
arrows. The arrow type is `Kleisli IO Buffer Buffer` or a dedicated `Buffer`
arrow; the existing `Arrow` typeclass/`Kleisli` machinery is the substrate.

`_` is no longer `Expr::Wire` (a special signal placeholder): it is an ordinary
λ-parameter of `main` (and of any signal lambda). The parser maps `_` to a
fresh parameter name in `main`'s parameter list.

### 2.5 Auto-desugar old programs

`main = <buffer-expr>` (and `main gain = <buffer-expr>` for control params)
auto-desugars to the IO form:

```
main = _ : onepole 200.0 0.7
→ main i1 = do { writeOutput 0 (onepole 200.0 0.7 i1); return () }

main = osc , osc          -- two outputs
→ main = do { writeOutput 0 osc; writeOutput 1 osc; return () }
```

Rules:
- `_` wires in `<buffer-expr>` become `main`'s λ-parameters (`i1`, `i2`, …)
  in order of first use.
- The N buffer outputs of `<buffer-expr>` become `writeOutput 0 …; …;
  writeOutput N-1 …`.
- Control parameters (`main gain = …` where `gain` is a `?param` or a scalar
  used as a control) remain control params — B3-2's `main_cells`/`set_param`
  path.

### 2.6 Runtime: lead the IO action

`run` leads the root fragment each tick:

1. `process(inputs, outputs)`: set `block_len`, materialize each input slice
   into `buffers[input_buffers[k]]`.
2. Call `main` with the input buffer registers (root fragment, base=0, empty
   env) — this is the "lead the IO action" step. The fragment's instructions
   mutate buffers in place (`ValueBuffer*`), read inputs (`ValueReadInput`),
   write outputs (`ValueWriteOutput`).
3. Drain: each `ValueWriteOutput` copies its source buffer into
   `outputs[idx]`.

`main` itself is compiled as a `Value::Callable` referencing a root fragment;
the runtime invokes it (like Haskell RTS invokes `main`).

---

## 3. Backward compatibility, RT safety

- **Old programs** (`main = <buffer-expr>`) keep working via auto-desugar.
- **RT safety:** unchanged — one value track, no new allocation. `writeOutput`
  copies within the existing buffer table / external slices.
- **`_` semantics:** a bare `_` still means "the program input" in `main` via
  auto-desugar; inside explicit `fn i1 -> …` it is an ordinary name.
- **DSP purity:** a `Buffer -> Buffer` function's in-place mutation is hidden
  behind the leading IO action (ST-style); the type system never exposes a
  partially-mutated buffer.

---

## 4. Testing

- `main` in IO form: `main i1 = do { writeOutput 0 (onepole 200.0 0.7 i1);
  return () }` produces the same output as the old `main = _ : onepole 200.0
  0.7`.
- Auto-desugar: old `main = <buffer-expr>` programs (signal suite) compile and
  run unchanged.
- `do`-notation + `return`/`bind` over `IO`: a program chaining two
  `writeOutput` calls.
- `readInput`: `main = do { i <- readInput 0; writeOutput 0 i; return () }`.
- Re-enable the `#[ignore]`d signal-lambda tests (currying_wires, func_values)
  and rill-adrift source-node tests.

---

## 5. File map

| Area | Files |
|---|---|
| Types | `types/ty.rs` (`IO a` + `instance Monad IO`; `readInput`/`writeOutput` builtin sigs) |
| Parser | `parser.rs` (`_` as param name; main form), `desugar.rs` (combinators→functions; old-main auto-desugar) |
| IR | `ir.rs` (`ValueReadInput`, `ValueWriteOutput`) |
| Lowering | `lower.rs` (main as root fragment; writeOutput/readInput lowering) |
| Interp | `backend/interp.rs` (`ValueReadInput`/`ValueWriteOutput` arms; lead main each tick) |
| Program | `program.rs` (output drain via writeOutput sites) |
| Tests | existing suite (auto-desugar keeps it green), new IO tests |

---

## 6. Implementation order

1. **IO-1 — Types.** `IO a` + `instance Monad IO` in prelude; `readInput`/
   `writeOutput` builtin signatures. Compiles.
2. **IO-2 — Desugar.** Faust combinators → functions; `_` as param name; main
   in IO form; old-main auto-desugar. Existing programs compile (infer sees the
   IO form).
3. **IO-3 — IR + Lowering.** `ValueReadInput`/`ValueWriteOutput`; main lowers as
   a root fragment; writeOutput sites = outputs.
4. **IO-4 — Runtime.** Lead main each tick; drain via writeOutput.
5. **IO-5 — Tests.** New IO tests; re-enable `#[ignore]`d signal-lambda +
   source-node tests; full suite + clippy + fmt green.

Each step lands independently; old programs keep working from IO-2 onward.