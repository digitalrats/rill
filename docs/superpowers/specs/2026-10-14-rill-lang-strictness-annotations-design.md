# rill-lang `!` — strictness annotations: design

Date: 2026-10-14
Status: proposed (awaiting review)

## Intent

rill-lang runs a lazy (call-by-need) value track in `complete` mode: every call
to a named λ-parameter definition and every `let` binding suspends as a thunk
(`ValueLazy`), built on the arena's `ThunkCell` machinery. The programmer
currently has **no way to force** — the compiler never emits `ValueForce`
(`ir.rs:505`), even though the VM instruction and the full `force()` runtime
(`interp.rs:2611`, `ThunkState::Unforced/Forcing/Forced/Bottom` with
memoization and black-hole detection) have existed for the lazy evaluator.

This feature gives the user an explicit strictness marker: **`!`**. The token is
available — `Tok::Cut` — left inert after the signal combinators were removed
in the Haskell 2010 migration (an expression-position `!` is currently a
parse error).

`not` remains the logical negation operator. This design leaves it untouched.

## Normative sources

The single mechanism (`!` → force at a specific point of the generated IR) has
**two** normative sources, and both are honored:

| Position | Source | Force point |
|---|---|---|
| Strictness flag in `data`/`newtype` fields | **Haskell 2010** (§4.2.1 `strictness-flags`) | At *construction*: the constructor is strict in that field |
| Bang pattern (def/λ param, `case`/`match` arm, `let`) | GHC extension `BangPatterns` | At *binding*: force before the body runs |

This is a deliberate, documented extension beyond the project's strict "Haskell
2010" baseline — the same way `do {}` and `?param` are. Rhe semantics for bang
patterns are taken verbatim from GHC: forcing means "evaluate to WHNF".

## Goals

- `!` accepted in five positions:
  1. Definition parameter: `step !x = x * 0.5`
  2. Lambda parameter: `\ !x -> x * 0.5`
  3. `case`/`match` arm pattern: `case c of !v => v + 1` — both whole-pattern
     bangs `!(Just v)` and payload-position bangs `Just !v` (both fall out of
     `parse_pattern` recursion; the force sits after payload extraction, the
     same "force at binding" rule)
  4. `data` strict fields, both product (`{ cutoff :: !Float }`) and sum
     (`data T = C !Int`)
  5. `let` binding, both expression `let !x = e in …` and `do { let !x = e; … }`
- All five lower to the **same instruction**: `ValueInstr::ValueForce`.
- Strict mode (`MODE strict`): `!` is accepted and semantically inert — strict
  mode creates no thunks, so forcing is the identity. `f !x` ≡ `f x` in strict.
- `!=` (`Tok::NotEq`) is unaffected; `!` in bare expression position remains a
  parse error.

## Non-goals / anti-goals

- `$!` (Haskell 2010 strict application) — out of scope. The `!` operator can
  be resurrected later if needed; nothing in this design blocks it.
- `UNPACK` / unlifted fields — out of scope.
- Changing `not`, `!=`, or any existing operator.
- Touching the legacy signal-arrow modules (`arrow.rs`, `graph/reconstruct.rs`)
  whose docs still reference the removed `Expr::Cut`. Pre-existing stale docs;
  handled by the migration's dead-code task, not here.
- `!` is **not a type**: it carries no information into `ValueTy`. Inference of
  a bang-annotated thing is the type of the thing.

## Key constraints (verified against the code)

- **VM is done.** `ValueInstr::ValueForce { dst, src }` (`ir.rs:505`) is a real
  instruction: forces the source to WHNF and shares the head ref with `dst`
  (`backend/interp.rs:1464`). `force()` already handles every `ThunkState`
  including black holes → bottom, and memoizes.
- **Lowering never emits it today.** `ValueForce` appears in lowering only in
  the `has_instr` classifier (`lower.rs:133`). This feature is its first
  product-code emission.
- **`!=` lexes before lone `!`** (`lexer.rs:295` vs `:467`), so renaming the
  token is safe.
- **Serde is derived** — new fields/variants flow through automatically.
- **The compiler will NOT catch every pattern site.** Several handlers use
  catch-all `_ =>` / `if let` arms that would silently swallow `Pattern::Bang`
  with no compile error. Known traps (each needs an explicit fix, see
  §6): `lower.rs:822` (`_ => true` in `statically_lowerable`), `lower.rs:4237`
  (`_ => false` arm selection), `lower.rs:4313` (`_ => …` binding), `lower.rs:4291`
  (`let Pattern::Var = p else …`), `reduce.rs:24` (`_ => {}` in `pattern_vars`).
- **Currying (plan Task 6) keeps `Param`** — the strictness flag on `Param`
  survives the def→nested-lambda rewrite untouched.
- **`ValueTy::Thunk` unification (plan Task 7) is orthogonal** — `!` never
  appears in types.

## Design

### 1. Lexer (`lexer.rs`)

- Rename `Tok::Cut` → `Tok::Bang` (enum at `:19`, emitters at `:467` and
  `:525`).
- `!=` (`Tok::NotEq`) continues to lex first (`:295`) — untouched.
- No new expression token: `!` is only meaningful in pattern/param/type-field
  position.

### 2. AST (`ast.rs`)

- `Pattern::Bang(Box<Pattern>)` — force the scrutinee to WHNF, then match the
  inner pattern. Applies at the whole-pattern level; with `parse_pattern`
  recursion it also naturally covers payload-position bangs (`Just !v` ⇒
  `Ctor("Just", [Bang(Var "v")])`).
- `Param` gains `strict: bool` (existing construction sites default to `false`).
- **Let-bindings:** a `let !x = e` binding carries a strictness bit on the
  binding def: `Def::Eval` gains `strict: bool`. This feature lands after
  migration Task 14, by which point `Def::Local` is deleted and every
  let-form (expression `let … in` and `do { let …; }`) binds through
  `Def::Eval` — so `strict` has exactly one def carrier.
- `Def::Data.fields` and `Def::Sum.ctors` gain a strictness bit per
  field/payload type (exact shape — builder's call; must round-trip through
  serde and render).
- **`DataInfo` is not a type, but it carries the force target.** The ctor
  lowering paths (record fields `lower.rs:1220`–`:1310`, sum payloads
  `:1319`–`:1343`, `sum_ctor` `:4541`) read `env.data_types`
  (`DataInfo::Record/Sum`, built at `infer.rs:1026`/`:1045`) — not the AST defs.
  The strictness bit must be plumbed into `DataInfo` as well, or data-field
  strictness silently no-ops.

### 3. Parser (`parser.rs`)

- `parse_pattern` (`:245`): leading `Tok::Bang` → `Pattern::Bang(parse_pattern())`
  (this recursive call also yields `Just !v` for ctor args, `:284`).
- `is_pattern_start` (`:158`): add `Tok::Bang`.
- Def params (`parse_top_def` ~`:418`) and λ params (`:997`): optional leading
  `!` before the identifier → `Param { strict: true, .. }`.
- `let` (`:881`) and `do`-`let` (`:1096`): accept `let !x = e`.
- `parse_data_def` (`:459`): accept `!` before a field type (product) and
  before a payload type (sum).
- `!` in expression atom/prefix position: still a parse error (no `Expr` node).
  `!=` handled by the lexer before the parser ever sees a lone `!`.

### 4. Inference (`types/infer.rs`)

- Pattern typing and arm analysis (`:1761`, `:1860`, `:1897`, `:1937`):
  `Pattern::Bang(p)` recurses into `p` — same type, same coverage
  (var/wildcard/ctor) classification.
- `Def::Data`/`Def::Sum` processing (`:1026`, `:1045`): strict fields change no
  types, but the `DataInfo` strictness bits are built here.

### 5. Lowering (`lower.rs`)

One helper: **emit `ValueForce { dst: fresh, src: bound_reg }` when a binding
is strict.** Placement:

- **Def/λ params**: fragment entry — `lower_fragment` (`:3313`) binds params as
  value registers 0..n; for a strict param, force right after binding and
  remap the param to the fresh dst register, before the body's instructions.
  Fragments are compiled once and shared across call sites, so the force runs
  per call correctly.
- **`let` (complete)**: `let !x = e` — build the thunk, then force at the bind
  site instead of leaving it for first use (`lower.rs:2339` complete branch).
  Strict mode: bind inline, force is identity.
- **`case`/`match`** — see §6: bang arms route to runtime dispatch with a
  **per-arm** force (never match-top), matching GHC ordered-clause semantics:
  `case bottom of { _ => 5 }` must still be 5 (a wildcard arm never forces).
- **Data ctors**: for strict fields, the argument register is forced before the
  constructor head is built — driven by `DataInfo`'s strictness bits (§2).

Uniform emission in both modes is the default (in strict mode the instruction
is an identity by construction of the VM); skipping emission in strict mode is
an equivalent optimization the builder may choose.

### 6. Match dispatch: excluded from the static path (builder-check resolution)

The static fast path (`statically_lowerable`, `lower.rs:817`–`:823`) currently
classifies any `Ctor/Var/Wild` arm as statically lowerable; today a `Bang` arm
would be swallowed by `_ => true` (:822) and silently miscompile. Resolution:

- Arms whose pattern contains `Bang` **anywhere** are excluded from the static
  fast path (same precedent as guarded arms: `lower.rs:761` already routes
  guarded arms to runtime dispatch) and handled by the runtime sequential
  dispatch (`:4218`–`:4405`), where the arm test chain forces the scrutinee
  **just before testing that arm**.
- `pattern_vars` (`reduce.rs:24`) recurses through `Bang` so bound variables are
  not dropped from shadowing-aware substitution.
- The `let Pattern::Var(name) = p else { continue }` payload-binding in the
  static path (`lower.rs:4291`) is unreachable for bang arms after the routing
  rule above; assert or keep defensive.

### 7. Reduce (`reduce.rs`)

- Pattern traversal sites (`Pattern::`): recurse through `Bang` (§6).
- β-reduction and strictness: complete mode already keeps `Apply` for
  parameterized definitions (never β-reduces, `reduce.rs:395`), so the force
  survives at the call site. In strict mode β-reduction is safe: substituting a
  forced arg into a thunkless body is identity. Document this invariant; do not
  let a future β-reduction flatline a strict param without inserting the force
  around the substituted argument.

### 8. Render (`render.rs`)

- `Pattern::Bang(p)` → `!` + inner (incl. nested `Just !v`); `Param.strict` →
  `!` prefix (`:371`–`:480`); strict data fields/ctor payloads → `!` before the
  type.
- Roundtrip: parser accepts exactly what render prints.

### 9. VM

Nothing. `ValueForce` and `force()` already exist.

## Semantics summary (what a user can rely on)

- `data C 1 !x` — `x`'s argument is WHNF by the time `C` exists; matching
  `C 1 x` never forces a second time.
- `f !x = body` (complete mode) — `body` runs with `x` already in WHNF; the
  thunk memo cell (if any) holds the forced head.
- `case v of !p => body` — forcing happens **when that arm is reached**, before
  `p` is tested; other arms' evaluation order is unchanged.
- `case v of Just !x => f x` — forces the payload after extraction.
- `let !x = e in body` — `e` is forced once at the `let`, the memoized head is
  shared by every use (call-by-need, just eager).
- Strict mode: identical behavior with or without `!` everywhere.

## Tests

- Lexer: `kinds("!x")` → `[Bang, Ident(x), Eof]`; `!=` still `NotEq`.
- Parser roundtrips for all five positions, incl. `!(Just v)`, `Just !v`, and
  `C !Int` in both product and sum data.
- Inference: `!x` infers the type of `x`; strict fields don't leak into
  `ValueTy`; `DataInfo` carries the strictness bits.
- Lowering: complete-mode `f !x` emits `ValueForce` before the body; `let !x`
  forces at bind; bang arms route to runtime dispatch with per-arm force
  (`case bottom of { _ => 5 }` stays 5); strict ctor args are forced. Strict
  mode: `!` produces identically-behaving output to no `!`.
- End-to-end: the existing lazy evaluation tests (`force_unforced_thunk_runs_and_memoizes`,
  `force_black_hole_is_bottom`, etc.) already cover the VM; add a compile→run
  test proving a bang-annotated signal program behaves identically in both modes
  and that a black hole stays bottom under `!`.
- Migration discipline: `cargo check/test --features dsp`, zero warnings,
  `cargo fmt`.

## Sequencing

The feature sits on top of the in-flight Haskell 2010 migration
(`docs/superpowers/plans/2026-10-12-rill-lang-haskell2010-unary-core.md`). It
depends on currying (Task 6: `Param` survives the def→λ rewrite) and on the
complete-mode value lowering being settled (Task 10). It is orthogonal to the
`ValueTy::Thunk` type unification (Task 7–8).

**Decision (user, 2026-10-14):** the feature is added to the existing plan as
one or two follow-up tasks, executed **after** Task 14 (dead-code removal).
This is not about schedule — it defines the implementation surface: after
Task 14 only `Def::Eval` survives (no `Def::Local`), def params live in
`Expr::Lambda.params` (post-Task 6), and lowering is value-only — so the
strict bit has exactly two carriers (`Param` and `Def::Eval`) and one
param-force placement (λ entry). Landing it earlier would mean touching dying
structures (`Def::Local`, def-param path) that Task 14 deletes; that is now
off the table.

## Builder-check resolutions (calls made during review)

1. **Payload-position bangs (`Just !v`): in scope.** `parse_pattern` recursion
   produces the AST for free; the force after payload extraction is the same
   "force at binding" rule as whole-pattern bangs.
2. **Bang arms exclude the static fast path.** Mirrors the existing guarded-arm
   precedent; prevents the silent miscompile at `statically_lowerable`.
3. **`DataInfo` carries the strictness bits** — the ctor lowering reads it, not
   the AST defs.
4. **Explicit audit of `_ =>` / `if let` pattern swallows** replaces the
   (wrong) "exhaustive match will catch it" assumption. Known traps listed in
   §Key constraints and §6.
5. **Let-representation:** `Def::Eval.strict` only — the feature lands after
   Task 14, so `Def::Local` is gone and needs no strictness bit. (See
   §Sequencing, user decision 2026-10-14.)

## Left to the builder

- Exact shape of the data-field strictness representation (AST + `DataInfo`).
- Whether to skip `ValueForce` emission in strict mode (equivalent, cosmetic).
- Naming: `strict` field on `Param` vs a `Pattern::Bang`-based def-param model.

## Verification checklist before "done"

- [ ] `cargo build -p rill-lang --features dsp`
- [ ] `cargo test -p rill-lang --features dsp`
- [ ] Rationalized clippy: zero warnings
- [ ] `cargo fmt`
- [ ] No change to `not`, `!=`, or strict-mode output
- [ ] A bang-annotated program compiles and runs on the complete-mode value track