# rill-lang: Rank-N Polymorphism + Thunk Unification — Design (Stage B sub-plan)

> **Stage:** B sub-plan — prerequisite for the `ST` effect monad and the future
> `(arg..) -> IO` form.
> **Parent spec:** `2026-10-10-rill-lang-value-signal-unification-st-design.md`.
> **Scope:** (A) full **rank-N polymorphism** — `forall` type expressions, value-type
> variable generalization, `runST : (forall s. ST s a) -> a` with automatic
> escape rejection; (B) **Thunk unification** — a Rust `Thunk` trait unifying
> `run_fragment` / `run_thunk_body` in the interpreter.

---

## 0. Executive summary

rill-lang's type system has `Scheme { vars }` (quantified variables) but only
generalizes **signal element** type variables (`Scalar::Var` in a channel's
`elem`); **value** type variables (`ValueTy::Var` in a channel's `vty`) are
monomorphic. The `ST` effect monad needs a phantom type `s` that cannot escape a
`runST` region; the earlier one-point check-escaping scan proved fragile (two
verified bypasses). The robust fix is **full rank-N polymorphism**: `forall`
type expressions, value-tyvar generalization, and a polymorphic `runST`
argument — escaping becomes rejected automatically because `s` is quantified.

Separately, the interpreter has two ~90%-identical fragment runners
(`run_fragment`, `run_thunk_body`). A Rust `Thunk` trait unifies them, reducing
~120 duplicated lines and simplifying the dispatch that polymorphic functions
will exercise.

Order: **rank-N first** (pure type-system work, no runtime change), **Thunk
unification second** (pure runtime refactor, no type-system change).

---

## 1. Context and current state (audited)

### 1.1 Rank-N machinery already present

| Piece | Location | Role |
|---|---|---|
| `Scheme { vars: Vec<TypeVarId>, ty }` | `types/ty.rs:172` | Quantified type variables |
| `Subst { map: Scalar, value_map: ValueTy }` | `types/ty.rs:184` | Scalar + value substitutions |
| `ValueTy::Var/TyConVar/TyConApp` | `types/ty.rs:70-81` | Value + type-constructor variables |
| `resolve_value` | `types/ty.rs:1064` | Follow value-substitution chains |
| `vty_mentions_tyvar` | `types/infer.rs:1581` | Free-value-var occurrence scan |
| `TypeExpr::TName/TApp/TFunc` | `ast.rs:17` | Type syntax (no `forall` yet) |

**Gap:** `Ctx::free_vars` (`infer.rs:136`) collects only `Scalar::Var` from
channel `elem`; `instantiate` (`infer.rs:114`) substitutes only those. Value
channels (`Rate::Value`) are cloned verbatim (`Channel::value(b.vty.clone())`).

### 1.2 Thunk/Closure duplication (audited)

`Value::Closure(ArenaRef, u32)` and `Value::Thunk { fragment, env, cell }` are
both "fragment + env". Two runners:

| | `run_fragment` (interp.rs:2090) | `run_thunk_body` (interp.rs:2362) |
|---|---|---|
| args source | value registers (`args: &[usize]`) | env-record tail (captures ++ args split by `value_ins`) |
| ~line count | ~122 | ~137 |
| steps 0-6 | scratch reserve → env→cells → watermark → args copy → run blocks → result → drain | identical |
| memo | none (eager) | memo cell (via `force()`) |

`ValueCallLazy` (interp.rs:1011) builds `Value::Thunk` from `Value::Closure` by
concatenating the closure's env fields with the argument refs into one Record.
`force()` (interp.rs:2233) drives `Value::Thunk` only.

---

## 2. Part A — Rank-N polymorphism

### 2.1 Syntax: `forall`

Add `TForall(Vec<String>, Box<TypeExpr>)` to `ast.rs::TypeExpr`:

```rill
id    : forall a. a -> a
runST : (forall s. ST s a) -> a
map   : forall a b. (a -> b) -> [a] -> [b]
```

Parser: `forall` keyword followed by one or more type-var names then `.` then a
`TypeExpr`. Render round-trips it. Reduce is unaffected (types are erased
before reduce).

### 2.2 Value-type-variable generalization

1. **`free_vars`** (`infer.rs:136`): also collect free value-tyvars from each
   channel's `vty` — `ValueTy::Var(v)`, `TyConVar(v)`, `TyConApp(v, ..)` head,
   resolved through `subst.value_map`. New helper `value_free_vars(t, subst)`
   (traverse `ValueTy` like `vty_mentions_tyvar` but collect/dedup/resolve).
2. **`instantiate`** (`infer.rs:114`): when substituting `Scheme.vars`, also
   map value-tyvars: for `Rate::Value` channels, substitute quantified
   `Var(v)`/`TyConVar(v)`/`TyConApp(v, args)` with fresh vars. New helper
   `value_instantiate(t, remap)`.

### 2.3 `runST` (rank-2 application)

`runST : (forall s. ST s a) -> a`. `Expr::RunST { body: Lambda }` (syntax already
in the tree from the superseded B2) infers as:

1. Fresh `TypeVarId` for `s`.
2. Bind `s` in scope (in `ctx.locals` like the `Expr::Lambda` arm, save/restore)
   as a `ValueTy::TyConVar`, shadowing outer value binders.
3. Infer the body.
4. Generalize `s` into the argument lambda's `Scheme.vars`.
5. `runST` applies the polymorphic lambda: the result `a` cannot mention `s`
   because `s` is quantified — escaping is rejected automatically.

### 2.4 Integration with typeclass/Arrow/Kleisli

Class method signatures already use implicit quantification via the class
variable. `forall` makes it explicit and adds value-polymorphism. `Kleisli m a b`
and `Arrow (Kleisli m)` keep working. A polymorphic value function
(`id2 = fn x -> x;` used at two types) now instantiates fresh each use.

### 2.5 Compatibility risk

Full value-tyvar generalization may change how existing polymorphic value
functions type. If it regresses the suite, narrow to: only generalize
`TyConVar`-kind value vars (the phantom case) and keep ordinary `ValueTy::Var`
monomorphic — report as a deviation with the regression evidence.

---

## 3. Part B — Thunk unification

### 3.1 Rust `Thunk` trait

In `backend/interp.rs` (or a new `backend/exec.rs`):

```rust
trait Thunk {
    fn fragment(&self) -> u32;
    fn env(&self) -> ArenaRef;
    fn memo(&self) -> Option<ArenaRef>;   // None for Closure (eager)
}
impl Thunk for Value::Closure { ... }
impl Thunk for Value::Thunk { ... }
```

### 3.2 Single runner

Merge `run_fragment` + `run_thunk_body` into one `run_call` with a parameter
for where the value-args come from (caller registers vs env-record tail). Steps
0-6 are shared; only the args-source step differs.

### 3.3 `force()` stays Thunk-specific

`force()` continues to drive memo/black-hole/cycle for `Value::Thunk`. Closure
calls (`ValueCallFunc`) remain eager. The trait gives a common `fragment`/`env`
view without changing memo semantics.

---

## 4. Ordering and non-goals

- **Part A (rank-N) first**: type-system only, no runtime change. Unblocks the
  `runST` phantom and the `Buffer`/ST Stage B tasks.
- **Part B (Thunk) second**: runtime refactor only, no type-system change.
- **Non-goals:** no `(arg..) -> IO`; no stream I/O; no `Buffer` typeclass
  methods in this sub-plan (they come in the parent Stage B tasks).

---

## 5. Testing

- **Part A:** `forall` parses/renders; a polymorphic value function instantiates
  fresh at two types; `runST (fn s -> 1.0)` runs to `1.0`; the two escape
  bypasses from the one-point approach are rejected (chain through higher-id
  var; outer value binder named `s`).
- **Part B:** the full `rill-lang` suite passes unchanged after the merge;
  `lang_chiptune`/`record_mic` still compile.

---

## 6. Verification

- `cargo test --workspace`, `cargo clippy --all-features --workspace`,
  `cargo fmt` — all green, 0 warnings.