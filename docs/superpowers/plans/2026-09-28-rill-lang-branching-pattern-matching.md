# rill-lang: Branching and Pattern Matching — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `if cond then a else b` and a generalized `match` (ctor/literal/wildcard/var/nested patterns + guards) with runtime control flow to the rill-lang value track, executed on a block-CFG with a trampoline interpreter.

**Architecture:** Restructure `Ir::value_instrs` (flat `Vec<ValueInstr>`) into a control-flow graph of `ValueBlock`s walked by a data-driven trampoline (`while cur != HALT`). `if`/`match` lower to `ValueTerm::Branch`/`BranchCtor` chains with a shared `out` register joined via `ValueMove`. Static fast-paths are preserved for statically-known conditions/scrutinees.

**Tech Stack:** Rust workspace crate `rill-lang` (path `rill/rill-lang`). No new dependencies. Test runner: `cargo test -p rill-lang`. Lint: `cargo clippy -p rill-lang`. Format: `cargo fmt`. All work on branch `feature/rill-lang-logic`.

**Spec:** `docs/superpowers/specs/2026-09-28-rill-lang-branching-pattern-matching-design.md`

---

## File structure

| File | Responsibility | Change |
|---|---|---|
| `src/ir.rs` | Value IR: `ValueInstr`, `ValueTerm`, `ValueBlock`, `Ir`, `FragmentIr` | Add `ValueBlock`, `ValueTerm`, `ValueMove`; replace `value_instrs` with `value_blocks`+`value_entry` |
| `src/ast.rs` | AST: `Expr`, `Pattern`, `MatchArm` | Add `Expr::If`, `Pattern`, `MatchArm`; change `Expr::Match` arm type |
| `src/lexer.rs` | Tokeniser | Add `KwIf`, `KwThen`, `KwElse` |
| `src/parser.rs` | Parser | Parse `if`; `parse_pattern`; guards; match arms |
| `src/render.rs` | AST pretty-printer (debug round-trip) | Render `if`, patterns, guards |
| `src/reduce.rs` | β-reduction / CAF substitution | Walk `If` and new `Match` shape |
| `src/types/infer.rs` | Type inference | Infer `If`; generalize `Match` typing + exhaustiveness; walk new nodes in `collect_static_calls` |
| `src/lower.rs` | IR lowering | Block emission; lower `if`/`match`; static fast-paths; remove "statically resolvable" error |
| `src/backend/interp.rs` | Runtime interpreter | Trampoline loop; `Branch`/`BranchCtor`/`ValueMove`; `remap_value_term`; fix test fixtures |
| `src/program.rs` | `RillProgram` | `max_drops` over blocks |
| `tests/branching.rs` | E2E `if` tests | New |
| `tests/match_patterns.rs` | E2E `match`/pattern/guard tests | New |
| `tests/collections_list.rs` | Repurpose static-only test | Edit `match_over_non_analyzable_scrutinee_is_compile_error` → runtime dispatch |
| `README.md`, `CHANGELOG.md` | Docs | Document branching + patterns |

---

## Task 1: Block-CFG IR refactor (mechanical, tests stay green)

Refactor the value track from a flat instruction list to blocks. **No behavior change** — every existing program becomes a single block ending in `Halt`.

**Files:**
- Modify: `src/ir.rs`
- Modify: `src/lower.rs` (emission only)
- Modify: `src/backend/interp.rs` (trampoline loop; fix test fixtures)
- Modify: `src/program.rs` (`max_drops`)
- Modify: `src/render.rs` (any `value_instrs` references — none expected; skip if clean)

- [ ] **Step 1: Add IR types**

In `src/ir.rs`, add before `pub enum ValueInstr`:

```rust
/// A straight-line run of value instructions ending in a terminator.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ValueBlock {
    /// Instructions executed in order.
    pub instrs: Vec<ValueInstr>,
    /// How the block ends.
    pub term: ValueTerm,
}

impl Default for ValueBlock {
    fn default() -> Self {
        ValueBlock {
            instrs: Vec::new(),
            term: ValueTerm::Halt,
        }
    }
}

/// The data-driven successor(s) of a value block. Control flow lives in these
/// ids (data), not in the Rust call stack.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ValueTerm {
    /// Run block `0`'s id.
    Fallthrough(usize),
    /// Branch on a Bool value register.
    Branch { cond: usize, then: usize, els: usize },
    /// Branch on a sum value's constructor tag.
    BranchCtor { slot: usize, ctor: u32, then: usize, els: usize },
    /// End of the value track (or fragment).
    Halt,
}
```

Add to `ValueInstr` (inside the enum, before `ValueCallBuiltin`):

```rust
    /// Move an arena ref between registers (`dst = src; src = None`) — an
    /// ownership transfer used at control-flow join points.
    ValueMove { dst: usize, src: usize },
```

Replace in `Ir` (`src/ir.rs:648-651`):

```rust
    /// Value-track blocks (per-tick), see `run_value_track`.
    pub value_blocks: Vec<ValueBlock>,
    /// Entry block of the value track.
    pub value_entry: usize,
```

Replace in `FragmentIr` (`src/ir.rs:549-555`):

```rust
    pub value_blocks: Vec<ValueBlock>,
    /// Entry block of the fragment's value track.
    pub entry: usize,
```

- [ ] **Step 2: Fix `Ir`/`FragmentIr` construction sites in tests and interp**

In `src/backend/interp.rs` (unit-test fixtures at ~1955, ~1977, ~2006-2021), replace every `value_instrs: vec![...]` with:

```rust
value_blocks: vec![ValueBlock {
    instrs: vec![ /* the same instructions */ ],
    term: ValueTerm::Halt,
}],
value_entry: 0,
```

and every `value_instrs: Vec::new()` with `value_blocks: Vec::new(), value_entry: 0,`. Update the `FragmentIr` literal at ~1976 to `value_blocks: vec![ValueBlock { instrs: vec![ValueInstr::ValueConstInt { dst: 0, value: 7 }], term: ValueTerm::Halt }], entry: 0,`. Update the helper struct at ~2006 and its `new` at ~2020 to carry `value_blocks`/`value_entry` instead of `value_instrs`.

- [ ] **Step 3: Update `max_drops` in `program.rs`**

Replace `fn max_drops` (`src/program.rs:388-397`):

```rust
    fn max_drops(ir: &Ir) -> usize {
        let instrs = |b: &ValueBlock| b.instrs.len();
        ir.value_blocks.iter().map(instrs).sum::<usize>()
            + ir.fragments
                .iter()
                .map(|f| f.value_blocks.iter().map(instrs).sum::<usize>())
                .sum::<usize>()
    }
```

Add `use crate::ir::ValueBlock;` to the imports. Also fix the other `value_instrs` references at `program.rs:392,395` (they are inside `max_drops`), and the `vec![]` constructors at `program.rs:621,649` → `value_blocks: Vec::new(), value_entry: 0,`.

- [ ] **Step 4: Rewire lowering emission to blocks**

In `src/lower.rs`:
- Replace field `value_instrs: Vec<ValueInstr>` (line 131) with:

```rust
    /// Value-track blocks, executed once per tick (see `run_value_track`).
    value_blocks: Vec<ValueBlock>,
    /// The block currently being appended to.
    cur_value_block: usize,
    /// Entry block of the program's value track (always 0).
    value_entry: usize,
```

- Replace `fn emit_value` (lines 209-211):

```rust
    fn emit_value(&mut self, i: ValueInstr) {
        self.value_blocks[self.cur_value_block].instrs.push(i);
    }

    fn new_value_block(&mut self) -> usize {
        self.value_blocks.push(ValueBlock::default());
        self.value_blocks.len() - 1
    }

    fn set_value_term(&mut self, block: usize, term: ValueTerm) {
        self.value_blocks[block].term = term;
    }
```

- In `Lowerer::new` (or wherever the struct is initialized, ~3220), initialize:

```rust
            value_blocks: vec![ValueBlock::default()],
            cur_value_block: 0,
            value_entry: 0,
```

- **Fragment compilation swap:** every `std::mem::take(&mut self.value_instrs)` must become a 3-field swap. Pattern for `compile_lambda_body` (~1455-1500) and `compile_fragment_body` (~1544-1590):

```rust
        let saved_blocks = std::mem::take(&mut self.value_blocks);
        let saved_cur = self.cur_value_block;
        self.value_blocks.push(ValueBlock::default());
        self.cur_value_block = 0;
        // ... lower the body ...
        let frag = FragmentIr {
            value_blocks: std::mem::take(&mut self.value_blocks),
            entry: 0,
            // ... other fields unchanged ...
        };
        self.value_blocks = saved_blocks;
        self.cur_value_block = saved_cur;
```

  Repeat for the third swap site (~1920-1927).

- **Final Ir assembly** (~3370-3376): replace `value_instrs: lw.value_instrs,` with:

```rust
        value_blocks: std::mem::take(&mut lw.value_blocks),
        value_entry: lw.value_entry,
```

  and before it, terminate the current block:

```rust
    lw.set_value_term(lw.cur_value_block, ValueTerm::Halt);
```

- The `let mut lw = lw(&env, &empty);`-style unit tests in `lower.rs` that read `lw.value_instrs` (e.g. ~3776-3841) must switch to `lw.value_blocks[i].instrs` (inspect `[0]`). At each such test, replace `.value_instrs` with `.value_blocks[0].instrs`.

- [ ] **Step 5: Trampoline in `run_value_track`**

Replace the loop in `run_value_track` (`src/backend/interp.rs:143-147`) with:

```rust
    let blocks = std::mem::take(&mut prog.ir.value_blocks);
    let mut cur = prog.ir.value_entry;
    while let Some(b) = blocks.get(cur) {
        for i in &b.instrs {
            exec_value_instr(prog, i, &mut drops);
        }
        cur = match b.term {
            ValueTerm::Fallthrough(n) => n,
            ValueTerm::Branch { cond, then, els } => {
                if value_reg_is_true(prog, cond) {
                    then
                } else {
                    els
                }
            }
            ValueTerm::BranchCtor { slot, ctor, then, els } => {
                if value_reg_ctor(prog, slot) == Some(ctor) {
                    then
                } else {
                    els
                }
            }
            ValueTerm::Halt => break,
        };
    }
    prog.ir.value_blocks = blocks;
```

Add module-level helpers (near `run_value_track`):

```rust
/// Whether a value register holds `Bool(true)`. Non-Bool/`None` is `false`
/// (defensive; static typing guarantees a Bool here).
fn value_reg_is_true<T: Transcendental, const BUF: usize>(
    prog: &RillProgram<T, BUF>,
    reg: usize,
) -> bool {
    matches!(
        prog.value_regs.get(reg).copied().flatten().and_then(|r| prog.arena.get(r)),
        Some(Value::Bool(true))
    )
}

/// The constructor tag of a value register's sum value, if any.
fn value_reg_ctor<T: Transcendental, const BUF: usize>(
    prog: &RillProgram<T, BUF>,
    reg: usize,
) -> Option<u32> {
    match prog.value_regs.get(reg).copied().flatten().and_then(|r| prog.arena.get(r)) {
        Some(Value::Sum(c, _)) => Some(*c),
        _ => None,
    }
}
```

Add `ValueInstr::ValueMove` to `exec_value_instr` (next to `ValueCopy`, ~757):

```rust
        ValueInstr::ValueMove { dst, src } => {
            prog.value_regs[*dst] = prog.value_regs[*src].take();
        }
```

Add `ValueMove` to `remap_value_instr` (~1648 area, mirroring `ValueCopy`):

```rust
        ValueInstr::ValueMove { dst, src } => ValueInstr::ValueMove {
            dst: dst + base,
            src: src + base,
        },
```

- [ ] **Step 6: Trampoline in `run_fragment`**

Replace the fragment loop (`src/backend/interp.rs:1506-1510`) with:

```rust
    let mut cur = frag.entry;
    while let Some(b) = frag.value_blocks.get(cur) {
        for i in &b.instrs {
            let remapped = remap_value_instr(i, base);
            exec_value_instr(prog, &remapped, drops);
        }
        cur = match b.term {
            ValueTerm::Fallthrough(n) => n,
            ValueTerm::Branch { cond, then, els } => {
                if value_reg_is_true(prog, cond + base) {
                    then
                } else {
                    els
                }
            }
            ValueTerm::BranchCtor { slot, ctor, then, els } => {
                if value_reg_ctor(prog, slot + base) == Some(ctor) {
                    then
                } else {
                    els
                }
            }
            ValueTerm::Halt => break,
        };
    }
```

`fallthrough`/branch targets are fragment-local block ids — never offset.

- [ ] **Step 7: Add a trampoline unit test (branch + move + ctor dispatch)**

In `src/backend/interp.rs` (the existing `#[cfg(test)]` module), add a test that builds a 3-block program directly and runs it:

```rust
    #[test]
    fn trampoline_branch_and_move() {
        // Blocks: 0: const 1.0, const 0.0, ValueMove into out reg (slot 2) of
        // the then-path; 1 (then): halt. Branch cond = Bool(true).
        let ir = Ir {
            num_main_cells: 0,
            value_blocks: vec![
                ValueBlock {
                    instrs: vec![
                        ValueInstr::ValueConstFloat { dst: 0, value: 1.0 },
                        ValueInstr::ValueConstFloat { dst: 1, value: 0.0 },
                        ValueInstr::ValueMove { dst: 2, src: 0 },
                    ],
                    term: ValueTerm::Fallthrough(1),
                },
                ValueBlock {
                    instrs: vec![],
                    term: ValueTerm::Halt,
                },
            ],
            value_entry: 0,
            ..test_ir_base(2)
        };
        // ... build a program, run one tick, assert value_regs[2] is Float(1.0)
    }
```

Use the existing fixture helpers in the interp test module (`test_program` / `make_program` patterns) to run the tick; assert the arena value in register 2.

- [ ] **Step 8: Run the full suite**

Run: `cargo test -p rill-lang`
Expected: all existing tests PASS (this is a pure refactor). Fix any remaining `value_instrs` compile errors (search: `rg "value_instrs" src/`).

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m 'refactor(rill-lang): value track as block CFG with trampoline execution'
```

---

## Task 2: Lexer keywords `if`/`then`/`else`

**Files:**
- Modify: `src/lexer.rs`

- [ ] **Step 1: Add token variants**

Add to `enum Tok` in `src/lexer.rs` (near `KwOf`, ~line 60):

```rust
    /// `if` keyword — conditional expression.
    KwIf,
    /// `then` keyword — `if` branch separator.
    KwThen,
    /// `else` keyword — `if` branch separator.
    KwElse,
```

- [ ] **Step 2: Map keywords**

In the keyword `match text` block (`src/lexer.rs:301-319`), add:

```rust
                "if" => Tok::KwIf,
                "then" => Tok::KwThen,
                "else" => Tok::KwElse,
```

Mirror the existing `if !followed_by_paren` guard style for all three (a user `else(...)` builtin must not lex as the keyword).

- [ ] **Step 3: Update keyword-match exhaustiveness**

The lexer has `match &t.tok` blocks at ~537 and ~560 that list keywords (used in tests or classification). Add `Tok::KwIf | Tok::KwThen | Tok::KwElse` wherever other keywords are grouped.

- [ ] **Step 4: Run**

Run: `cargo test -p rill-lang`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m 'feat(rill-lang): lexer tokens for if/then/else'
```

---

## Task 3: AST — `Pattern`, `MatchArm`, `Expr::If`, new `Expr::Match`

**Files:**
- Modify: `src/ast.rs`

- [ ] **Step 1: Add `Pattern` and `MatchArm`**

Add before `pub enum Expr` (`src/ast.rs:70`):

```rust
/// A match pattern. The case convention: an uppercase-initial identifier is a
/// constructor, a lowercase-initial identifier is a variable binding, `_` is a
/// wildcard.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Pattern {
    /// Binds the whole matched value to a name (`x`).
    Var(String),
    /// Matches anything, binds nothing (`_`).
    Wild,
    /// Integer literal.
    LitInt(i64),
    /// Float literal.
    LitFloat(f64),
    /// Boolean literal.
    LitBool(bool),
    /// String literal.
    LitStr(String),
    /// Constructor application with (possibly nested) argument patterns.
    Ctor(String, Vec<Pattern>),
}

/// One `match` arm: a pattern, then a sequence of `(guard, body)` alternatives.
/// The first alternative's guard is `true` (a bare `=> body`).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MatchArm {
    /// The pattern tested against the scrutinee.
    pub pattern: Pattern,
    /// `(guard_expr, body_expr)` pairs, evaluated in order; the first that is
    /// true runs its body.
    pub guards: Vec<(Expr, Expr)>,
    /// Span of the whole arm (for diagnostics).
    pub span: Span,
}
```

- [ ] **Step 2: Change `Expr::Match` and add `Expr::If`**

Replace the `Match` variant (`src/ast.rs:164-172`):

```rust
    /// Pattern matching over a value (a sum or a scalar).
    Match {
        /// Scrutinee expression.
        scrutinee: Box<Expr>,
        /// Arms.
        arms: Vec<MatchArm>,
        /// Span.
        span: Span,
    },
```

Add a new variant (after `Match`):

```rust
    /// Conditional expression `if cond then a else b`.
    If {
        /// Condition (must be a Bool value).
        cond: Box<Expr>,
        /// Taken when the condition is true.
        then: Box<Expr>,
        /// Taken when the condition is false.
        els: Box<Expr>,
        /// Span.
        span: Span,
    },
```

- [ ] **Step 3: Update `Expr::span()`**

Add `| Expr::If { span, .. }` to the span match (both the positional arm list is unchanged — `Match` is already there).

- [ ] **Step 4: Run**

Run: `cargo test -p rill-lang`
Expected: compile errors in parser/reduce/render/infer/lower (all `Expr::Match` consumers) — this task only adds AST types; the crate will not compile until Tasks 4-5 fix consumers. If you prefer to keep green, do Task 4's parser change in the same session before running tests. (Recommendation: proceed to Task 4 before running.)

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m 'feat(rill-lang): AST for if and match patterns/guards'
```

---

## Task 4: Parser — `if`, patterns, guards

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Parse `if`**

In `parse_prefix` (`src/parser.rs:653`, before the `Tok::KwMatch` arm), add:

```rust
            Tok::KwIf => {
                self.bump();
                let cond = self.parse_expr(0, false)?;
                self.eat(&Tok::KwThen)?;
                let then = self.parse_expr(0, true)?;
                self.eat(&Tok::KwElse)?;
                let els = self.parse_expr(0, true)?;
                let span = t.span.merge(els.span());
                Ok(Expr::If {
                    cond: Box::new(cond),
                    then: Box::new(then),
                    els: Box::new(els),
                    span,
                })
            }
```

(`then`/`else` are parsed with `no_comma = true` so a following `,` inside a record literal is not swallowed — same rule as match arm bodies.)

- [ ] **Step 2: Add `parse_pattern` and `is_pattern_start`**

Add a module-level helper next to `is_atom_start`:

```rust
/// Check if a token can begin a (sub)pattern.
fn is_pattern_start(tok: &Tok) -> bool {
    matches!(
        tok,
        Tok::Ident(_)
            | Tok::Int(_)
            | Tok::Float(_)
            | Tok::Str(_)
            | Tok::KwTrue
            | Tok::KwFalse
            | Tok::Wire
            | Tok::LParen
    )
}
```

Add a method on `Parser` (next to `expect_ident`):

```rust
    fn parse_pattern(&mut self) -> Result<Pattern, CompileError> {
        let t = self.peek().clone();
        match &t.tok {
            Tok::Wire => {
                self.bump();
                Ok(Pattern::Wild)
            }
            Tok::Int(v) => {
                self.bump();
                Ok(Pattern::LitInt(*v))
            }
            Tok::Float(v) => {
                self.bump();
                Ok(Pattern::LitFloat(*v))
            }
            Tok::Str(s) => {
                self.bump();
                Ok(Pattern::LitStr(s.clone()))
            }
            Tok::KwTrue => {
                self.bump();
                Ok(Pattern::LitBool(true))
            }
            Tok::KwFalse => {
                self.bump();
                Ok(Pattern::LitBool(false))
            }
            Tok::LParen => {
                self.bump();
                let p = self.parse_pattern()?;
                self.eat(&Tok::RParen)?;
                Ok(p)
            }
            Tok::Ident(name) => {
                let is_ctor = name.chars().next().map_or(false, |c| c.is_uppercase());
                self.bump();
                if is_ctor {
                    let mut args = Vec::new();
                    while is_pattern_start(&self.peek().tok) {
                        args.push(self.parse_pattern()?);
                    }
                    Ok(Pattern::Ctor(name.clone(), args))
                } else {
                    Ok(Pattern::Var(name.clone()))
                }
            }
            _ => Err(CompileError::Parse {
                msg: format!("expected a pattern, found {:?}", t.tok),
                span: t.span,
            }),
        }
    }
```

- [ ] **Step 3: Rewrite the `match` arm loop**

Replace the arm-parsing loop in the `Tok::KwMatch` arm (`src/parser.rs:658-675`):

```rust
                let mut arms = Vec::new();
                while self.peek().tok != Tok::RBrace {
                    let pat_span = self.peek().span;
                    let pattern = self.parse_pattern()?;
                    self.eat(&Tok::FatArrow)?;
                    let first = self.parse_expr(0, true)?;
                    let mut guards = vec![(Expr::Bool(true, pat_span), first)];
                    while self.peek().tok == Tok::Pipe {
                        self.bump();
                        let g = self.parse_expr(0, true)?;
                        self.eat(&Tok::FatArrow)?;
                        let b = self.parse_expr(0, true)?;
                        guards.push((g, b));
                    }
                    arms.push(MatchArm {
                        pattern,
                        guards,
                        span: t.span.merge(guards.last().unwrap().1.span()),
                    });
                    if self.peek().tok == Tok::Semi {
                        self.bump();
                    }
                }
```

- [ ] **Step 4: Add parser round-trip tests**

In `src/parser.rs` `#[cfg(test)]`, add:

```rust
    #[test]
    fn parse_if_expression() {
        let p = parse("main = if true then 1.0 else 2.0;");
        let p = p.unwrap();
        assert!(matches!(p.body(), Expr::If { .. }));
    }

    #[test]
    fn parse_match_patterns_and_guards() {
        let p = parse(
            "data Shape = Circle Float | Rect Float Float; \
             main = match s of { Circle r => r; 0 => 0.0; _ => 1.0; n | n > 0 => n; };",
        )
        .unwrap();
        // The parser accepts mixed ctor/literal/wildcard arms and guards.
        let _ = p;
    }
```

  Add a render round-trip in `src/render.rs` `#[cfg(test)]` after Task 5 implements rendering.

- [ ] **Step 5: Run**

Run: `cargo test -p rill-lang`
Expected: parser tests pass; remaining compile errors are only the un-updated `Expr::Match` consumers (`reduce.rs`, `render.rs`, `infer.rs`, `lower.rs`) — fix those in Task 5 before running the full suite, or run only `cargo test -p rill-lang --bin rill-lang` if it isolates parser tests.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m 'feat(rill-lang): parse if and match patterns/guards'
```

---

## Task 5: Render + reduce walkers for the new AST

**Files:**
- Modify: `src/render.rs`
- Modify: `src/reduce.rs`
- Modify: `src/types/infer.rs` (`collect_static_calls` only)

- [ ] **Step 1: Render `if`, patterns, guards**

Replace the `Expr::Match` arm in `render_expr` (`src/render.rs:257-273`):

```rust
        Expr::Match {
            scrutinee, arms, ..
        } => {
            write!(buf, "match ").ok();
            render_expr(scrutinee, buf, 0);
            write!(buf, " of {{ ").ok();
            for arm in arms {
                render_pattern(&arm.pattern, buf);
                for (idx, (g, body)) in arm.guards.iter().enumerate() {
                    if idx > 0 {
                        write!(buf, " | ").ok();
                        render_expr(g, buf, 0);
                    }
                    write!(buf, " => ").ok();
                    render_expr(body, buf, 0);
                }
                write!(buf, "; ").ok();
            }
            write!(buf, "}}").ok();
        }
        Expr::If { cond, then, els, .. } => {
            write!(buf, "if ").ok();
            render_expr(cond, buf, 0);
            write!(buf, " then ").ok();
            render_expr(then, buf, 0);
            write!(buf, " else ").ok();
            render_expr(els, buf, 0);
        }
```

Add a helper (module-level):

```rust
fn render_pattern(p: &Pattern, buf: &mut String) {
    match p {
        Pattern::Wild => write!(buf, "_").ok(),
        Pattern::Var(n) => write!(buf, "{n}").ok(),
        Pattern::LitInt(v) => write!(buf, "{v}").ok(),
        Pattern::LitFloat(v) => write!(buf, "{v}").ok(),
        Pattern::LitBool(v) => write!(buf, "{v}").ok(),
        Pattern::LitStr(s) => write!(buf, "\"{s}\"").ok(),
        Pattern::Ctor(n, args) => {
            write!(buf, "{n}").ok();
            for a in args {
                write!(buf, " ").ok();
                render_pattern(a, buf);
            }
        }
    }
}
```

Note: the round-trip render drops the bare-`=>`/guard distinction only in the *first* alternative (rendered as `=>` without a guard), which re-parses as guard `true` — round-trip is preserved.

- [ ] **Step 2: `reduce.rs` substitution for `Match`/`If`**

Replace the `Expr::Match` arm in `substitute` (`src/reduce.rs:90-112`):

```rust
        Expr::Match {
            scrutinee,
            arms,
            span,
        } => {
            let reduced_arms: Vec<MatchArm> = arms
                .iter()
                .map(|arm| {
                    let mut inner = subst.clone();
                    for v in pattern_vars(&arm.pattern) {
                        inner.remove(&v);
                    }
                    MatchArm {
                        pattern: arm.pattern.clone(),
                        guards: arm
                            .guards
                            .iter()
                            .map(|(g, b)| (substitute(g, &inner), substitute(b, &inner)))
                            .collect(),
                        span: arm.span,
                    }
                })
                .collect();
            Expr::Match {
                scrutinee: Box::new(substitute(scrutinee, subst)),
                arms: reduced_arms,
                span: *span,
            }
        }
        Expr::If { cond, then, els, span } => Expr::If {
            cond: Box::new(substitute(cond, subst)),
            then: Box::new(substitute(then, subst)),
            els: Box::new(substitute(els, subst)),
            span: *span,
        },
```

Add a module-level helper in `reduce.rs`:

```rust
/// All variable bindings introduced by a pattern (for shadowing-aware
/// substitution).
fn pattern_vars(p: &Pattern) -> Vec<String> {
    let mut out = Vec::new();
    match p {
        Pattern::Var(n) => out.push(n.clone()),
        Pattern::Ctor(_, args) => {
            for a in args {
                out.extend(pattern_vars(a));
            }
        }
        _ => {}
    }
    out
}
```

Add `use crate::ast::{MatchArm, Pattern};` to `reduce.rs` imports.

- [ ] **Step 3: `collect_static_calls` in `infer.rs`**

Replace the `Expr::Match` arm in `collect_static_calls` (`src/types/infer.rs:351-362`) and add `If`:

```rust
        Expr::Match {
            scrutinee, arms, ..
        } => {
            collect_static_calls(scrutinee, src, bound, nodes, out);
            for arm in arms {
                let mut inner = bound.clone();
                for v in pattern_vars(&arm.pattern) {
                    inner.insert(v);
                }
                for (g, b) in &arm.guards {
                    collect_static_calls(g, src, &inner, nodes, out);
                    collect_static_calls(b, src, &inner, nodes, out);
                }
            }
        }
        Expr::If { cond, then, els, .. } => {
            collect_static_calls(cond, src, bound, nodes, out);
            collect_static_calls(then, src, bound, nodes, out);
            collect_static_calls(els, src, bound, nodes, out);
        }
```

Add a `pattern_vars` helper in `infer.rs` (or import from `reduce.rs` if `pub(crate)`):

```rust
fn pattern_vars(p: &Pattern) -> Vec<String> {
    let mut out = Vec::new();
    match p {
        Pattern::Var(n) => out.push(n.clone()),
        Pattern::Ctor(_, args) => {
            for a in args {
                out.extend(pattern_vars(a));
            }
        }
        _ => {}
    }
    out
}
```

- [ ] **Step 4: Run**

Run: `cargo test -p rill-lang`
Expected: compiles; any tests still failing are `infer.rs`/`lower.rs` match tests — Task 6/7 fix them. Run `cargo test -p rill-lang --lib` to gate on unit tests only if needed.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m 'refactor(rill-lang): render and reduce walk new match/if AST'
```

---

## Task 6: Inference — `if` and generalized `match` typing + exhaustiveness

**Files:**
- Modify: `src/types/infer.rs`

- [ ] **Step 1: Infer `Expr::If`**

Add to `infer_expr` (next to the `Expr::Match` arm):

```rust
        Expr::If { cond, then, els, span } => {
            let ct = infer_expr(ctx, cond)?;
            if ct.arity_out() != 1 || ct.outs[0].rate != Rate::Value {
                return Err(CompileError::Type {
                    msg: "if condition must be a Bool value".into(),
                    span: cond.span(),
                });
            }
            if !matches!(ctx.subst.resolve_value(&ct.outs[0].vty), ValueTy::Bool) {
                return Err(CompileError::Type {
                    msg: "if condition must be a Bool value".into(),
                    span: cond.span(),
                });
            }
            let tt = infer_expr(ctx, then)?;
            let et = infer_expr(ctx, els)?;
            let tv = arm_result_vty(ctx, tt, then, *span)?;
            let ev = arm_result_vty(ctx, et, els, *span)?;
            unify_value(&tv, &ev, &mut ctx.subst, *span)?;
            Ok(ArrowTy::value_channel(tv))
        }
```

- [ ] **Step 2: Add `arm_result_vty` helper**

The existing match arm logic (bare `Int`/`Float` literals are signal-rate but value-compatible) is reused. Add a helper and use it in BOTH `if` and `match`:

```rust
/// Resolve an arm/branch body's result value type: a value channel directly,
/// or a bare `Int`/`Float` literal (signal-rate in v1 but value-compatible in
/// value positions).
fn arm_result_vty(
    ctx: &mut InferCtx,
    bt: ArrowTy,
    body: &Expr,
    span: Span,
) -> Result<ValueTy, CompileError> {
    if bt.arity_in() != 0 || bt.arity_out() != 1 {
        return Err(CompileError::Type {
            msg: "branch must be a value expression (0\u{2192}1 value channel)".into(),
            span,
        });
    }
    match bt.outs[0].rate {
        Rate::Value => Ok(bt.outs[0].vty.clone()),
        Rate::Signal => match body {
            Expr::Int(_, _) => Ok(ValueTy::Int),
            Expr::Float(_, _) => Ok(ValueTy::Float),
            _ => Err(CompileError::Type {
                msg: "branch must be a value expression (0\u{2192}1 value channel)".into(),
                span,
            }),
        },
    }
}
```

Refactor the existing match arm body handling (`infer.rs:1436-1457`) to call `arm_result_vty`.

- [ ] **Step 3: Rewrite `Expr::Match` inference**

Replace the whole `Expr::Match` arm (`infer.rs:1288-1474`) with the generalized version:

```rust
        Expr::Match {
            scrutinee,
            arms,
            span,
        } => {
            if arms.is_empty() {
                return Err(CompileError::Type {
                    msg: "match requires at least one arm".into(),
                    span: *span,
                });
            }
            let st = match scrutinee.as_ref() {
                Expr::Wire(_) => {
                    let v = ctx.next;
                    ctx.next += 1;
                    ArrowTy::value_channel(ValueTy::Var(v))
                }
                _ => infer_expr(ctx, scrutinee)?,
            };
            if st.arity_out() != 1 || st.outs[0].rate != Rate::Value {
                return Err(CompileError::Type {
                    msg: "match scrutinee must be a value".into(),
                    span: *span,
                });
            }
            let scrutinee_vty = st.outs[0].vty.clone();
            // Sum or scalar? Resolve the scrutinee type enough to decide.
            let resolved = ctx.subst.resolve_value(&scrutinee_vty);
            let scrutinee_sum = match &resolved {
                ValueTy::Data(name, _) => Some(name.clone()),
                ValueTy::App(name, _)
                    if matches!(
                        ctx.env.data_types.get(name.as_str()),
                        Some(DataInfo::Sum(_))
                    ) =>
                {
                    Some(name.clone())
                }
                _ => None,
            };
            // If any arm uses a constructor pattern, the scrutinee must be a sum.
            let has_ctor_pattern = arms.iter().any(|a| matches!(a.pattern, Pattern::Ctor(_, _)));
            if has_ctor_pattern {
                let sum_name = match &scrutinee_sum {
                    Some(n) => n.clone(),
                    None => {
                        // Try to derive the sum from the ctor patterns (existing
                        // intersection logic), else error.
                        let mut candidates: Option<Vec<String>> = None;
                        for a in &arms {
                            if let Pattern::Ctor(name, _) = &a.pattern {
                                let mut per = sum_types_with_ctor(ctx, name);
                                if let Some(sn) = &scrutinee_sum {
                                    per.retain(|n| n == sn);
                                }
                                if per.is_empty() {
                                    return Err(CompileError::Type {
                                        msg: format!("unknown constructor `{name}`"),
                                        span: a.span,
                                    });
                                }
                                candidates = Some(match candidates {
                                    None => per,
                                    Some(acc) => acc
                                        .into_iter()
                                        .filter(|n| per.contains(n))
                                        .collect(),
                                });
                            }
                        }
                        match candidates {
                            Some(v) if v.len() == 1 => v[0].clone(),
                            _ => {
                                return Err(CompileError::Type {
                                    msg: "match arms use constructors of ambiguous or different sum types"
                                        .into(),
                                    span: *span,
                                });
                            }
                        }
                    }
                };
                // Pin the scrutinee to the sum type (existing logic) ...
                let pin_ty = match ctx.env.ctor_arity(&sum_name) {
                    Some(arity) => {
                        let mut args: Vec<ValueTy> = match &scrutinee_vty {
                            ValueTy::App(_, a) | ValueTy::Data(_, a) => a.clone(),
                            _ => vec![],
                        };
                        while args.len() < arity {
                            args.push(ctx.fresh_vty());
                        }
                        ValueTy::App(sum_name.clone(), args)
                    }
                    None => ValueTy::Data(sum_name.clone(), vec![]),
                };
                unify_value(&scrutinee_vty, &pin_ty, &mut ctx.subst, *span)?;
                check_match_arms(ctx, &sum_name, &pin_ty, arms, *span)?;
            } else {
                // Scalar (or unknown) match: literal/Var/Wild patterns only.
                // Pin literal patterns to the scrutinee type; require totality.
                let mut result: Option<ArrowTy> = None;
                for a in arms {
                    let saved = ctx.locals.clone();
                    check_scalar_pattern(ctx, &a.pattern, &scrutinee_vty, a.span)?;
                    for (g, body) in &a.guards {
                        if !matches!(g, Expr::Bool(true, _)) {
                            let gt = infer_expr(ctx, g)?;
                            if gt.arity_out() != 1
                                || gt.outs[0].rate != Rate::Value
                                || !matches!(ctx.subst.resolve_value(&gt.outs[0].vty), ValueTy::Bool)
                            {
                                return Err(CompileError::Type {
                                    msg: "match guard must be a Bool value".into(),
                                    span: g.span(),
                                });
                            }
                        }
                        let bt = infer_expr(ctx, body)?;
                        let bv = arm_result_vty(ctx, bt, body, body.span())?;
                        if let Some(ref acc) = result {
                            unify_value(&acc.outs[0].vty, &bv, &mut ctx.subst, body.span())?;
                        } else {
                            result = Some(ArrowTy::value_channel(bv));
                        }
                    }
                    ctx.locals = saved;
                }
                check_exhaustive_scalar(ctx, arms, &scrutinee_vty, *span)?;
                result.unwrap_or(ArrowTy::value_channel(ValueTy::Float))
            }
        }
```

This step restructures the match inference. To keep it tractable, define the following helpers (add them as free functions in `infer.rs`):

```rust
/// Bind a pattern's variables into `ctx.locals` and type-check the pattern
/// against `vty` (recursively for nested constructor patterns).
fn bind_pattern(
    ctx: &mut InferCtx,
    pattern: &Pattern,
    vty: &ValueTy,
    sum_name: &str,
    is_builtin: bool,
    scrutinee_args: &[ValueTy],
    arm_span: Span,
) -> Result<(), CompileError> {
    match pattern {
        Pattern::Var(name) => {
            ctx.locals
                .insert(name.clone(), ArrowTy::value_channel(vty.clone()));
            Ok(())
        }
        Pattern::Wild => Ok(()),
        Pattern::Ctor(name, args) => {
            let payload = match sum_ctor_payload(ctx, sum_name, name) {
                Some(p) => p,
                None => {
                    return Err(CompileError::Type {
                        msg: format!("unknown constructor `{name}` for `{sum_name}`"),
                        span: arm_span,
                    });
                }
            };
            if args.len() != payload.len() {
                return Err(CompileError::Type {
                    msg: format!("wrong number of patterns for constructor `{name}`"),
                    span: arm_span,
                });
            }
            for (arg, pt) in args.iter().zip(payload.iter()) {
                let resolved = if is_builtin {
                    match pt {
                        ValueTy::Var(k) => scrutinee_args
                            .get(k.saturating_sub(1) as usize)
                            .cloned()
                            .unwrap_or_else(|| ctx.fresh_vty()),
                        t => t.clone(),
                    }
                } else {
                    pt.clone()
                };
                bind_pattern(ctx, arg, &resolved, sum_name, is_builtin, scrutinee_args, arm_span)?;
            }
            Ok(())
        }
        Pattern::LitInt(_) | Pattern::LitFloat(_) | Pattern::LitBool(_) | Pattern::LitStr(_) => {
            Err(CompileError::Type {
                msg: "literal pattern requires a scalar scrutinee".into(),
                span: arm_span,
            })
        }
    }
}

/// Type-check every arm against the (already pinned) sum scrutinee: patterns,
/// guards (Bool), and unified bodies. `scrutinee_vty` is the pinned scrutinee
/// type (an `App(name, args)` for builtin sums, `Data(name, [])` for user sums).
fn check_match_arms(
    ctx: &mut InferCtx,
    sum_name: &str,
    scrutinee_vty: &ValueTy,
    arms: &[MatchArm],
    span: Span,
) -> Result<(), CompileError> {
    let scrutinee_args: Vec<ValueTy> = match &scrutinee_vty {
        ValueTy::App(_, a) | ValueTy::Data(_, a) => a.clone(),
        _ => vec![],
    };
    let is_builtin = ctx.env.ctor_arity(sum_name).is_some();
    let mut result: Option<ArrowTy> = None;
    for arm in arms {
        for (g, _) in &arm.guards {
            if !matches!(g, Expr::Bool(true, _)) {
                let gt = infer_expr(ctx, g)?;
                if gt.arity_out() != 1 || gt.outs[0].rate != Rate::Value {
                    return Err(CompileError::Type {
                        msg: "match guard must be a Bool value".into(),
                        span: g.span(),
                    });
                }
                if !matches!(ctx.subst.resolve_value(&gt.outs[0].vty), ValueTy::Bool) {
                    return Err(CompileError::Type {
                        msg: "match guard must be a Bool value".into(),
                        span: g.span(),
                    });
                }
            }
        }
        let saved = ctx.locals.clone();
        bind_pattern(ctx, &arm.pattern, scrutinee_vty, sum_name, is_builtin, &scrutinee_args, arm.span)?;
        for (_, body) in &arm.guards {
            let bt = infer_expr(ctx, body)?;
            let bv = arm_result_vty(ctx, bt, body, body.span())?;
            if let Some(ref acc) = result {
                unify_value(&acc.outs[0].vty, &bv, &mut ctx.subst, body.span())?;
            } else {
                result = Some(ArrowTy::value_channel(bv));
            }
        }
        ctx.locals = saved;
    }
    check_exhaustive_sum(ctx, sum_name, arms, span)?;
    Ok(())
}
```

The scalar path needs these helpers (add as free functions):

```rust
/// Pin a literal pattern's type to the scrutinee type (so a `Wire` scrutinee
/// infers `Int` from a `0` arm) and bind variable patterns.
fn check_scalar_pattern(
    ctx: &mut InferCtx,
    pattern: &Pattern,
    scrutinee_vty: &ValueTy,
    arm_span: Span,
) -> Result<(), CompileError> {
    match pattern {
        Pattern::Var(name) => {
            ctx.locals
                .insert(name.clone(), ArrowTy::value_channel(scrutinee_vty.clone()));
            Ok(())
        }
        Pattern::Wild => Ok(()),
        Pattern::LitInt(_) => unify_value(scrutinee_vty, &ValueTy::Int, &mut ctx.subst, arm_span)?,
        Pattern::LitFloat(_) => unify_value(scrutinee_vty, &ValueTy::Float, &mut ctx.subst, arm_span)?,
        Pattern::LitBool(_) => unify_value(scrutinee_vty, &ValueTy::Bool, &mut ctx.subst, arm_span)?,
        Pattern::LitStr(_) => unify_value(scrutinee_vty, &ValueTy::String, &mut ctx.subst, arm_span)?,
        Pattern::Ctor(_, _) => {
            return Err(CompileError::Type {
                msg: "constructor pattern requires a sum scrutinee".into(),
                span: arm_span,
            });
        }
    }
    Ok(())
}

/// Scalar totality: a `Wild`/`Var` arm, or (for Bool) both literals.
fn check_exhaustive_scalar(
    ctx: &InferCtx,
    arms: &[MatchArm],
    scrutinee_vty: &ValueTy,
    span: Span,
) -> Result<(), CompileError> {
    let is_bool = matches!(ctx.subst.resolve_value(scrutinee_vty), ValueTy::Bool);
    let mut has_wild = false;
    let mut has_true = false;
    let mut has_false = false;
    for a in arms {
        let guarded = a.guards.len() > 1
            || !matches!(a.guards.first(), Some((Expr::Bool(true, _), _)));
        if guarded {
            continue;
        }
        match &a.pattern {
            Pattern::Wild | Pattern::Var(_) => has_wild = true,
            Pattern::LitBool(b) => {
                if *b {
                    has_true = true;
                } else {
                    has_false = true;
                }
            }
            _ => {}
        }
    }
    let ok = has_wild || (is_bool && has_true && has_false);
    if ok {
        return Ok(());
    }
    Err(CompileError::Type {
        msg: "non-exhaustive match: a scalar match needs a `_` (or variable) arm"
            .into(),
        span,
    })
}
```

In the `Expr::Match` arm's scalar branch: save `ctx.locals`, loop `check_scalar_pattern` per arm, infer each guard (Bool) and body (`arm_result_vty` + unify across arms — mirror the sum path's body loop), restore `ctx.locals`, then call `check_exhaustive_scalar`. Bindings for the guards/body of a scalar arm come from `check_scalar_pattern` (`Var` arms).

- [ ] **Step 4: Exhaustiveness check (sums)**

Add a helper (called from `check_match_arms` after type-checking):

```rust
/// Compile-time totality: every constructor is covered by an unguarded arm, or
/// an unguarded Wild/Var arm exists.
fn check_exhaustive_sum(
    ctx: &InferCtx,
    sum_name: &str,
    arms: &[MatchArm],
    span: Span,
) -> Result<(), CompileError> {
    let ctors: Vec<String> = match ctx.env.data_types.get(sum_name) {
        Some(DataInfo::Sum(cs)) => cs.iter().map(|(n, _)| n.clone()).collect(),
        _ => return Ok(()),
    };
    let mut covered: HashSet<&str> = HashSet::new();
    let mut has_wild = false;
    for a in arms {
        let guarded = a.guards.len() > 1 || !matches!(a.guards.first(), Some((Expr::Bool(true, _), _)));
        if guarded {
            continue;
        }
        match &a.pattern {
            Pattern::Ctor(name, _) => {
                covered.insert(name.as_str());
            }
            Pattern::Wild | Pattern::Var(_) => has_wild = true,
            _ => {}
        }
    }
    if has_wild {
        return Ok(());
    }
    let missing: Vec<String> = ctors.into_iter().filter(|c| !covered.contains(c.as_str())).collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(CompileError::Type {
        msg: format!("non-exhaustive match: missing constructor(s) {}", missing.join(", ")),
        span,
    })
}
```

Note: an arm is "guarded" if its first alternative is not the bare `=> body` form (guard `true`).

- [ ] **Step 5: Run**

Run: `cargo test -p rill-lang`
Expected: unit/integration tests pass except the two intentionally-rewritten tests in Task 9 and any lower.rs match tests (Task 7/8). Verify at minimum `cargo test -p rill-lang --lib`.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m 'feat(rill-lang): infer if/match with patterns, guards, exhaustiveness'
```

---

## Task 7: Lower `if` (branch + static fast path)

**Files:**
- Modify: `src/lower.rs`

- [ ] **Step 1: Write the failing unit test**

In `src/lower.rs` `#[cfg(test)]` (near the existing static-match tests ~3819), add:

```rust
    #[test]
    fn lower_if_emits_branch() {
        let p = parse(&tokenize("main = if true then 1.0 else 2.0;").unwrap(), b"main = if true then 1.0 else 2.0;")
            .unwrap();
        let tp = infer_program(&p).unwrap();
        let ir = lower(&tp).unwrap();
        assert!(
            ir.value_blocks
                .iter()
                .any(|b| matches!(b.term, ValueTerm::Branch { .. })),
            "a non-constant if condition must lower to a Branch"
        );
    }
```

  (This asserts the runtime path when the condition is not a literal Bool; the static fast path is exercised by `if false then ... else ...` → single block, no Branch — add a second assertion in Step 3.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rill-lang --lib lower_if_emits_branch`
Expected: FAIL (no `if` lowering yet).

- [ ] **Step 3: Implement `if` lowering**

Add to `lower_value` in `src/lower.rs` (next to the `Expr::Match` arm):

```rust
            Expr::If { cond, then, els, span } => {
                // Static fast path: a literal Bool condition selects one branch
                // at compile time (single block, no Branch term).
                if let Expr::Bool(b, _) = cond.as_ref() {
                    let (reg, ty) = if *b {
                        self.lower_value(then)?
                    } else {
                        self.lower_value(els)?
                    };
                    return Ok((reg, ty));
                }
                let (cond_reg, cond_ty) = self.lower_value(cond)?;
                if cond_ty != ValueTy::Bool {
                    return Err(CompileError::Type {
                        msg: "if condition must be a Bool value".into(),
                        span: *span,
                    });
                }
                let then_b = self.new_value_block();
                let els_b = self.new_value_block();
                let join = self.new_value_block();
                let out = self.fresh_value_reg();
                self.set_value_term(
                    self.cur_value_block,
                    ValueTerm::Branch {
                        cond: cond_reg,
                        then: then_b,
                        els: els_b,
                    },
                );
                self.cur_value_block = then_b;
                let (t_reg, t_ty) = self.lower_value(then)?;
                self.emit_value(ValueInstr::ValueMove { dst: out, src: t_reg });
                self.set_value_term(self.cur_value_block, ValueTerm::Fallthrough(join));
                self.cur_value_block = els_b;
                let (e_reg, e_ty) = self.lower_value(els)?;
                self.emit_value(ValueInstr::ValueMove { dst: out, src: e_reg });
                self.set_value_term(self.cur_value_block, ValueTerm::Fallthrough(join));
                // unify branch types (inference already guarantees equality)
                if t_ty != e_ty {
                    return Err(CompileError::Type {
                        msg: "if branches must have the same type".into(),
                        span: *span,
                    });
                }
                self.cur_value_block = join;
                Ok((out, t_ty))
            }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p rill-lang`
Expected: `lower_if_emits_branch` PASSes; existing suites still PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m 'feat(rill-lang): lower if to branch blocks with static fast path'
```

---

## Task 8: Lower `match` — patterns, guards, runtime dispatch

**Files:**
- Modify: `src/lower.rs`

- [ ] **Step 1: Write the failing unit tests**

In `src/lower.rs` `#[cfg(test)]`:

```rust
    #[test]
    fn lower_match_runtime_dispatch_emits_branch_ctor() {
        let src = "data Shape = Circle Float | Rect Float Float; \
                   main = match _ of { Circle r => r; Rect w h => w; };";
        let toks = tokenize(src).unwrap();
        let p = parse(&toks, src.as_bytes()).unwrap();
        let tp = infer_program(&p).unwrap();
        let ir = lower(&tp).unwrap();
        assert!(
            ir.value_blocks.iter().any(|b| matches!(b.term, ValueTerm::BranchCtor { .. })),
            "a non-static match must lower to BranchCtor dispatch"
        );
    }

    #[test]
    fn lower_match_literal_and_wildcard() {
        let src = "main = match 0 of { 0 => 1.0; _ => 2.0; };";
        let toks = tokenize(src).unwrap();
        let p = parse(&toks, src.as_bytes()).unwrap();
        let tp = infer_program(&p).unwrap();
        let ir = lower(&tp).unwrap();
        assert!(
            ir.value_blocks.iter().any(|b| matches!(b.term, ValueTerm::Branch { .. })),
            "a literal match must lower to a Branch"
        );
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --lib lower_match_runtime_dispatch_emits_branch_ctor lower_match_literal_and_wildcard`
Expected: FAIL (match still static-only).

- [ ] **Step 3: Implement runtime match lowering**

Replace the static-only body of the `Expr::Match` arm in `lower_value` (`src/lower.rs:336-459`). Keep the existing scrutinee lowering, sum resolution (`resolve_match_sum`), ctor table, builtin-sum handling, and `static_scrutinee_ctor` fast path (only when the match has **no guarded arms**), but replace the "not statically resolvable" error with a runtime dispatch chain:

```rust
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => {
                let (scrutinee_reg, scrutinee_vty) = self.lower_value(scrutinee)?;
                let has_guards = arms.iter().any(|a| a.guards.len() > 1
                    || !matches!(a.guards.first(), Some((Expr::Bool(true, _), _))));
                // Static fast path (guard-free, statically-known ctor): lower
                // only the matching arm — preserves exact-capacity behavior.
                if !has_guards {
                    if let Some(cname) = self.static_scrutinee_ctor(scrutinee.as_ref()) {
                        let sum_name = match &scrutinee_vty {
                            ValueTy::Data(n, _) => n.clone(),
                            _ => self.resolve_match_sum(arms, *span)?,
                        };
                        return self.lower_static_match_arm(scrutinee_reg, &scrutinee_vty, &sum_name, arms, &cname, *span);
                    }
                }
                // Runtime dispatch: sequential test chain ending in a fail block.
                let out = self.fresh_value_reg();
                let join = self.new_value_block();
                let fail = self.new_value_block();
                // fail block: latch ProcessError, then halt.
                self.emit_value(ValueInstr::ValueSetError); // new instruction, Step 4
                self.set_value_term(fail, ValueTerm::Halt);
                let n = arms.len();
                // All arm-entry blocks are created up front so the `else`
                // target of every arm is known before any body is lowered.
                let entries: Vec<usize> = (0..n).map(|_| self.new_value_block()).collect();
                let mut out_ty: Option<ValueTy> = None;
                for i in 0..n {
                    let else_t = if i + 1 < n { entries[i + 1] } else { fail };
                    let arm = &arms[i];
                    self.cur_value_block = entries[i];
                    let body_b = self.new_value_block();
                    let (_, body_ty) = self.lower_pattern_test(
                        &arm.pattern,
                        scrutinee_reg,
                        &scrutinee_vty,
                        body_b,
                        else_t,
                        arm,
                        &mut out_ty,
                    )?;
                    // Guard + body blocks for this arm (its scope is pushed
                    // inside lower_pattern_test / lower_match_guards_and_body).
                    self.cur_value_block = body_b;
                    self.lower_match_guards_and_body(arm, &out, join, else_t, &mut out_ty)?;
                }
                self.cur_value_block = join;
                Ok((out, out_ty.unwrap_or(ValueTy::Float)))
            }
```

Implement these `Lowerer` helpers (add after `resolve_match_sum`):

- `lower_static_match_arm(scrutinee_reg, scrutinee_vty, sum_name, arms, cname, span) -> (usize, ValueTy)` — port the existing static-selection code (`lower.rs:387-454`) verbatim: find the arm whose pattern's constructor == `cname`, `ValueMatch` payload extraction, bind `Pattern::Var` leaves, lower body, return `(body_reg, ty)`. For a `Var`/`Wild`/literal arm under a static ctor, bind appropriately (Var binds the scrutinee; Wild ignores; literal can only match if the static ctor's payload equals it — treat a literal arm as never-matching unless the scrutinee is that literal).

- `lower_pattern_test(pattern, reg, vty, pass_b, fail_b, arm, out_ty: &mut Option<ValueTy>) -> Result<(usize, ValueTy)>` — emits the pattern's test into `self.cur_value_block`, jumping to `pass_b` on success and `fail_b` on mismatch; returns `(pass_b, <the type this arm's body will produce, from the first guard>)` and records it into `out_ty` if not yet set. Details:
  - `Wild` / `Var`: nothing (bindings applied later); fall through (the block's term is set by the caller's continuation).
  - `Lit(v)`: emit `ValueConst*` for `v` into a fresh reg + `ValueCompare { dst, op: Eq, a: reg, b: lit_reg }`, then `set_value_term(cur, Branch { cond: dst, then: pass_b, els: fail_b })` and `cur = pass_b`.
  - `Ctor(name, args)`: resolve `ctor_idx` from the sum table; `set_value_term(cur, BranchCtor { slot: reg, ctor, then: bind_b, els: fail_b })`; in `bind_b` emit `ValueMatch { dst: payload_regs, slot: reg, ctor }`; bind `Pattern::Var` leaves to payload regs in a new scope; recurse `lower_pattern_test` per arg against its payload reg (each nested test gets its own `then` chain). `cur = pass_b` after the last arg's test.

- `lower_match_guards_and_body(arm, out, join, else_t, out_ty: &mut Option<ValueTy>) -> Result<(), CompileError>` — for each `(g, body)` in `arm.guards`: if it's the first and g is `Bool(true)` (bare), just lower body into `cur`, `ValueMove { dst: out, src: body_reg }`, `set_value_term(cur, Fallthrough(join))`; else lower guard → `guard_reg` (Bool), `set_value_term(cur, Branch { cond: guard_reg, then: body_b, els: next })`, `cur = body_b`, lower body, move, fallthrough to join; the last guard's `els` = `else_t`. Records the body's lowered type into `out_ty` (first one wins).

- [ ] **Step 4: Add `ValueSetError` instruction**

In `src/ir.rs` `ValueInstr`, add:

```rust
    /// Latch a `ProcessError::Processing` for this tick (runtime match
    /// non-exhaustive). The tick fails at the end of `run_value_track`.
    ValueSetError,
```

In `exec_value_instr` (`interp.rs`, near `ValueNot`), add:

```rust
        ValueInstr::ValueSetError => {
            prog.value_error = Some(ProcessError::processing(
                "match is non-exhaustive at runtime (no arm matched)",
            ));
        }
```

Add `ValueSetError` to `remap_value_instr` (no fields — passthrough). `ProcessError::processing` is the constructor (`rill-core/src/traits/error.rs:88`, message becomes `Processing(...)`).

- [ ] **Step 5: Remove the static-only error + repurpose tests**

Delete the `"match scrutinee is not statically resolvable in v1"` arm in `lower_value` (`lower.rs:396-401`) and its doc comment. The two tests asserting it — `lower.rs:3861` (`match_wire_scrutinee_is_compile_error`) and `tests/collections_list.rs:234` — will be rewritten in Task 9. For now, delete/replace `match_wire_scrutinee_is_compile_error` with an assertion that lowering **succeeds** and emits a `BranchCtor` (mirroring `lower_match_runtime_dispatch_emits_branch_ctor`).

- [ ] **Step 6: Run**

Run: `cargo test -p rill-lang`
Expected: new lower tests PASS; the two old static-only tests updated in this task PASS; remaining failures are `tests/collections_list.rs:234` (rewrite in Task 9) and any infer tests fixed in Task 6.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m 'feat(rill-lang): lower match to runtime BranchCtor dispatch with guards'
```

---

## Task 9: Repurpose the two static-only tests + runtime E2E

**Files:**
- Modify: `tests/collections_list.rs`
- Create: `tests/match_patterns.rs`

- [ ] **Step 1: Rewrite `match_over_non_analyzable_scrutinee_is_compile_error`**

In `tests/collections_list.rs:233-251`, replace the whole test with:

```rust
#[test]
fn match_over_non_analyzable_scrutinee_dispatches_at_runtime() {
    // `head (filter ...)` is not statically analyzable — the runtime dispatch
    // must select `Just` and expose its payload (regression for the old
    // static-only behavior which rejected it).
    let mut prog = compile::<f32>(
        "main = match (head (filter (fn x -> x > 1.0) [1.0, 2.0, 3.0])) of { Nothing => 0.0; Just x => x; };",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(2.0)
    );
}
```

- [ ] **Step 2: Write `tests/match_patterns.rs` runtime tests**

Create `tests/match_patterns.rs`:

```rust
//! Runtime `match`: dispatch on non-static scrutinees, literal/wildcard/var
//! patterns, nested patterns, and guards.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

fn run_float(src: &str) -> f64 {
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(f) => *f,
        rill_lang::arena::Value::Int(i) => *i as f64,
        other => panic!("unexpected output {other:?}"),
    }
}

#[test]
fn runtime_match_on_head_of_filter() {
    assert_eq!(
        run_float("main = match (head (filter (fn x -> x > 1.0) [1.0, 2.0, 3.0])) of { Nothing => 0.0; Just x => x; };"),
        2.0
    );
}

#[test]
fn literal_int_patterns() {
    assert_eq!(
        run_float("main = match 0 of { 0 => 1.0; 1 => 2.0; _ => 3.0; };"),
        1.0
    );
    assert_eq!(
        run_float("main = match 5 of { 0 => 1.0; 1 => 2.0; _ => 3.0; };"),
        3.0
    );
}

#[test]
fn literal_bool_patterns_without_wildcard() {
    assert_eq!(
        run_float("main = match true of { true => 1.0; false => 0.0; };"),
        1.0
    );
    assert_eq!(
        run_float("main = match false of { true => 1.0; false => 0.0; };"),
        0.0
    );
}

#[test]
fn nested_pattern() {
    assert_eq!(
        run_float("main = match Just (Left 2.0) of { Just (Left x) => x; _ => 0.0; };"),
        2.0
    );
}

#[test]
fn var_pattern_binds_whole_value() {
    assert_eq!(run_float("main = match 7 of { v => v; };"), 7.0);
}

#[test]
fn guards_evaluate_in_order_with_fallthrough() {
    assert_eq!(
        run_float("main = match 3.0 of { n | n > 2.0 => 1.0; n | n > 1.0 => 2.0; _ => 3.0; };"),
        1.0
    );
    assert_eq!(
        run_float("main = match 1.5 of { n | n > 2.0 => 1.0; n | n > 1.0 => 2.0; _ => 3.0; };"),
        2.0
    );
    assert_eq!(
        run_float("main = match 0.5 of { n | n > 2.0 => 1.0; n | n > 1.0 => 2.0; _ => 3.0; };"),
        3.0
    );
}

#[test]
fn guarded_arm_fallthrough_to_nonmatching_arm_errors() {
    // Structurally total (Just guarded, Nothing unguarded); at runtime the
    // guard fails and Nothing does not match Just → ProcessError.
    let mut prog = compile::<f32>(
        "main = match Just 5.0 of { Just x | x > 10.0 => 1.0; Nothing => 2.0; };",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    assert!(MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).is_err());
}
```

- [ ] **Step 3: Run**

Run: `cargo test -p rill-lang --test match_patterns --test collections_list`
Expected: all PASS.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m 'test(rill-lang): runtime match dispatch, patterns, and guards'
```

---

## Task 10: E2E `if` — SetParameter switching and value-state

**Files:**
- Create: `tests/branching.rs`

- [ ] **Step 1: Write the tests**

Create `tests/branching.rs`:

```rust
//! `if` as a pure expression: static selection, result binding, and runtime
//! per-tick switching driven by SetParameter and value-state feedback.

use rill_core::traits::{MultichannelAlgorithm, ParamValue};
use rill_lang::compile;

fn float_output(prog: &mut impl MultichannelAlgorithm<f32>, out: &mut [f32; 4]) -> f64 {
    MultichannelAlgorithm::process(prog, &[], &mut [out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(f) => *f,
        other => panic!("unexpected output {other:?}"),
    }
}

#[test]
fn if_static_branches() {
    let mut prog = compile::<f32>("main = if true then 1.0 else 2.0;").unwrap();
    let mut out = [0.0f32; 4];
    assert_eq!(float_output(&mut prog, &mut out), 1.0);
    let mut prog = compile::<f32>("main = if false then 1.0 else 2.0;").unwrap();
    assert_eq!(float_output(&mut prog, &mut out), 2.0);
}

#[test]
fn if_binds_result_and_nests() {
    let mut prog = compile::<f32>("x = if false then 1.0 else 2.0; main = 0.5 * x;").unwrap();
    let mut out = [0.0f32; 4];
    assert_eq!(float_output(&mut prog, &mut out), 1.0);
}

#[test]
fn if_switches_on_setparameter_between_ticks() {
    // SetParameter → main cell → per-tick re-evaluation → branch switches.
    let mut prog = compile::<f32>("main g = if g > 0.5 then 1.0 else 2.0;").unwrap();
    let mut out = [0.0f32; 4];
    // default g = 0 → else branch
    assert_eq!(float_output(&mut prog, &mut out), 2.0);
    let idx = prog.param_index("g").unwrap();
    prog.set_param(idx, ParamValue::Float(1.0));
    assert_eq!(float_output(&mut prog, &mut out), 1.0);
    prog.set_param(idx, ParamValue::Float(0.0));
    assert_eq!(float_output(&mut prog, &mut out), 2.0);
}

#[test]
fn if_over_value_state_feedback() {
    // acc = ~ (acc + 1.0) accumulates across ticks; the Bool flips at 2.5.
    let mut prog = compile::<f32>(
        "acc = ~ (acc + 1.0); main = if acc > 2.5 then 1.0 else 0.0;",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..4 {
        float_output(&mut prog, &mut out);
    }
    assert_eq!(float_output(&mut prog, &mut out), 1.0);
}

#[test]
fn if_and_runtime_match_combined() {
    // A SetParameter-driven Bool selects a sum at runtime; the outer match
    // dispatches on the runtime constructor.
    let mut prog = compile::<f32>(
        "main g = match (if g > 0.5 then Just 1.0 else Nothing) of { Just x => x; Nothing => 0.0; };",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    assert_eq!(float_output(&mut prog, &mut out), 0.0);
    prog.set_param(prog.param_index("g").unwrap(), ParamValue::Float(1.0));
    assert_eq!(float_output(&mut prog, &mut out), 1.0);
}
```

- [ ] **Step 2: Run**

Run: `cargo test -p rill-lang --test branching`
Expected: all PASS. If `if_over_value_state_feedback` fails because `~` on a self-referencing value isn't supported yet, replace the accumulator with the param-driven test only and note the limitation (spec §2.2 allows either source).

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m 'test(rill-lang): if branching E2E with SetParameter switching'
```

---

## Task 11: Compile-error tests

**Files:**
- Create: `tests/compile_errors.rs`

- [ ] **Step 1: Write the tests**

Create `tests/compile_errors.rs`:

```rust
//! Compile-time rejection: bad conditions, mismatched branches, and
//! non-exhaustive / ill-typed matches.

use rill_lang::compile;

#[test]
fn if_non_bool_cond() {
    assert!(compile::<f32>("main = if 1.0 then 1.0 else 2.0;").is_err());
}

#[test]
fn if_branch_type_mismatch() {
    assert!(compile::<f32>("main = if true then 1.0 else \"s\";").is_err());
}

#[test]
fn match_missing_ctor() {
    assert!(compile::<f32>(
        "data Shape = Circle Float | Rect Float Float; \
         main = match Circle 1.0 of { Circle r => r; };"
    )
    .is_err());
}

#[test]
fn match_scalar_without_wildcard() {
    assert!(compile::<f32>("main = match 0 of { 0 => 1.0; };").is_err());
}

#[test]
fn match_guarded_no_unguarded_fallback() {
    assert!(compile::<f32>(
        "data Shape = Circle Float | Rect Float Float; \
         main = match Circle 1.0 of { Circle r | r > 0.0 => r; Rect w h => w; };"
    )
    .is_err());
}

#[test]
fn match_ctor_arity_mismatch() {
    assert!(compile::<f32>(
        "main = match Just 1.0 of { Just x y => x; _ => 0.0; };"
    )
    .is_err());
}

#[test]
fn match_unknown_ctor() {
    assert!(compile::<f32>(
        "main = match Just 1.0 of { Nope x => x; _ => 0.0; };"
    )
    .is_err());
}

#[test]
fn match_signal_scrutinee() {
    assert!(compile::<f32>("main = match _ of { _ => 0.0; };").is_err());
}
```

Note: `match_signal_scrutinee` — `match _ of { _ => 0.0 }` has a `Wire` scrutinee (a fresh value channel, value rate) — if this compiles under the new rules, replace it with a genuinely signal-rate scrutinee case (e.g. match over a signal expression) and assert the "must be a value" error. Adjust in implementation based on the inference result.

- [ ] **Step 2: Run**

Run: `cargo test -p rill-lang --test compile_errors`
Expected: all PASS.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m 'test(rill-lang): compile-error coverage for if/match'
```

---

## Task 12: Documentation

**Files:**
- Modify: `README.md` (`rill/rill-lang/README.md`)
- Modify: `CHANGELOG.md` (`rill/CHANGELOG.md`)

- [ ] **Step 1: README — branching section**

Add after the "First-class functions and closures" section a short section:

```markdown
## Branching and pattern matching

`if cond then a else b` and `match` are pure expressions on the value track,
re-evaluated every tick. A runtime `Bool` — e.g. a main λ-parameter compared to
a threshold and written via `SetParameter` — switches branches from block to
block:

```faust
?gate = 1.0;
main = if gate > 0.5 then 1.0 else 0.0;
```

`match` supports constructor, literal (`0`, `1.5`, `true`, `"s"`), wildcard
`_`, variable, and nested patterns plus Haskell-style guards. An uppercase
initial is a constructor, a lowercase initial is a binding. Matches must be
exhaustive (every constructor covered, or a `_`/variable arm); a guarded arm
whose guard fails falls through to the next arm, and a residual non-match is a
runtime `ProcessError`.
```

Update the "Status" section: replace the static-only mention with "runtime
control flow (`if`/`match` on the value track)".

- [ ] **Step 2: CHANGELOG entry**

Add to `CHANGELOG.md` under an unreleased/`0.6.0-M2` heading:

```markdown
### rill-lang
- feat: `if cond then a else b` and generalized `match` (constructor/literal/
  wildcard/variable/nested patterns, guards) with runtime control flow on the
  value track.
- refactor: value track executes on a block CFG (`ValueBlock`/`ValueTerm`)
  driven by a trampoline interpreter; static fast-paths preserved.
```

- [ ] **Step 3: Final verification**

Run:
```bash
cargo test -p rill-lang
cargo clippy -p rill-lang --all-features
cargo fmt
```
Expected: all tests PASS; clippy has **zero warnings** (fix any by following the warnings policy: real fixes, no blanket `#[allow]`); `cargo fmt` reports no changes (or apply and re-run).

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m 'docs(rill-lang): document branching and pattern matching'
```

---

## Task 13: Full-suite regression + release branch check

**Files:** none (verification)

- [ ] **Step 1: Full workspace verification**

Run:
```bash
cargo test --workspace
cargo clippy --workspace --all-features
```
Expected: everything PASSes, zero clippy warnings in the workspace's code.

- [ ] **Step 2: Confirm the two repurposed tests behave as runtime dispatch**

Run: `cargo test -p rill-lang --test collections_list match_over_non_analyzable_scrutinee_dispatches_at_runtime`
Expected: PASS (asserts `Float(2.0)`).

- [ ] **Step 3: Commit any stragglers**

```bash
git status
git add -A && git commit -m 'chore(rill-lang): final regression fixes'   # only if there are changes
```

- [ ] **Step 4: Report**

Summarize for the user: what shipped, the block-CFG/trampoline structure, the tests added, and any deviations from the spec (e.g. value-state `~` feedback behavior, `match_signal_scrutinee` case).

---

## Self-review notes (for the implementer)

- **Spec coverage:** `if` (Task 7/10), patterns+guards (Task 4/8/9), typing + exhaustiveness (Task 6/11), block-CFG IR + trampoline (Task 1), runtime dispatch (Task 8/9), static fast-paths (Task 7/8), docs (Task 12), old-test repurposing (Task 9). All spec sections mapped.
- **Acyclic-invariant note:** the value track remains acyclic in this feature (no loops added); the block-CFG is what will host loops later. No `DispatchCtor` jump table — sequential `BranchCtor` chains per the spec.
- **Determinism:** ctor indices from declaration order; arm/guard evaluation in source order; first match wins. Do not use `HashMap` iteration anywhere in lowering/inference decisions.
