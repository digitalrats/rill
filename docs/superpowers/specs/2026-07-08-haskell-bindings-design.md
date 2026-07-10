# Haskell-style Bindings for rill-lang

**Date**: 2026-07-08
**Status**: design

## Motivation

Current rill-lang has a single entry-point grammar: `main = body where { defs }`. Definitions in `where` have sequential scoping (def N sees defs 0..N−1). No `let` expressions exist.

Goal: bring binding semantics to Haskell rules:
- Mutual recursion in all binding groups (`let`, `where`, top-level)
- `let` expressions — mutual group visible in a scoped expression
- Top-level definitions — program is a list of mutually recursive definitions, `main` is the designated entry

## Design

### 1. Grammar

```
program ::= top_def (';' top_def)* ';'? EOF

top_def ::= name params? '=' body ('where' where_block)?

params ::= name+

body ::= expr

where_block ::= '{' (def ';'?)* '}'          -- explicit braces
              | <newline> (def <newline>)+      -- layout (indent ≥ layout_col)
              | <newline> '{' (def ';'?)* '}'   -- newline, then braces

def ::= name params? '=' body
```

**`let` expressions:**
```
expr ::= ... | 'let' where_block 'in' expr
```

Examples:
```haskell
-- Top-level mutual recursion
sq(x) = x * x;
main = sq(_);

-- Top-level with where (mutually recursive where block)
main = osc : filt where
    osc(freq) = sine(freq, 0.5, 0.0)
    filt = _ : lowpass(cutoff, 0.7)
    cutoff = 1000.0

-- Expression-level let
main = let g = _ * 0.5 in g : _
```

**Lexically**: `let` and `in` are new keywords. `let` tokenizes as `KwLet`, `in` as `KwIn`. Both revert to identifiers when followed by `(` (function calls `let(` and `in(` are legal identifiers).

**Syntax**: function vs. constant binding distinguished only by presence of parameters left of `=` — identical to Haskell.

### 2. Scoping rules

| Binding form | Visibility | Mutual recursion |
|---|---|---|
| **Top-level defs** | All top-level definitions in the program | Yes |
| **`where` block** | Only within the function/definition it's attached to | Yes, within the block |
| **`let` expression** | Only within the `in` body expression | Yes, within the block |

Shadowing: inner scopes shadow outer names. `where` and `let` block names
shadow top-level names; nested `let`/`where` blocks shadow outer blocks.

### 3. Mutual recursion semantics

For any binding group (top-level, `where` block, `let` block):

1. **All names are visible to all bodies** — including self-reference.
2. **Type inference**: collect all definitions, create placeholder type schemes, infer each body with the full mutual environment, then unify and generalize.
3. **Lowering**: definitions in a mutually recursive group must be scheduled carefully — the SCC analysis in the scheduler already handles this.

**Occurs check**: self-reference in type inference requires an occurs check during unification to reject genuinely circular types (e.g. `x = x` where the type would be infinitely recursive). The unifier already has occurs-check machinery for unification variables.

**Runtime semantics**: mutual recursion at the signal-diagram level means definitions can reference each other's diagrams. The scheduler already handles this correctly (it builds an SCC-based schedule from the data-dependency graph).

### 3. Implementation steps

#### 3.1 Lexer (`lexer.rs`)
- Add `Tok::KwLet`, `Tok::KwIn`
- Lex `let` → `KwLet` when next char is not `(`
- Lex `in` → `KwIn` when next char is not `(`
- Update `keyword_token()` helper

#### 3.2 Parser (`parser.rs`)
- Refactor `parse_program` → parse list of top-level definitions
- Extract `parse_where_block()` for reuse in `let` and `where`
- Add `let ... in ...` expression parsing
- `main` is required as one of the top-level definitions (validation in `build_program` or parser)
- Each top-level def may have an optional `where` block

#### 3.3 AST (`ast.rs`)
- Add `Expr::Let { defs: Vec<Def>, body: Box<Expr>, span: Span }`
- No changes to `Def` — same structure for all binding contexts
- `Program` changes: remove `params` (params are now part of `main` def), add `top_defs: Vec<Def>`

#### 3.4 Type inference (`types/infer.rs`)
- `infer_program`: 
  1. First pass: collect all top-level names, insert fresh placeholder schemes into `defs`
  2. Second pass: infer each body with full mutual environment
  3. Validate exactly one `main` exists
- `infer_expr` for `Expr::Let`:
  1. Collect names, insert fresh placeholders into `defs` (shadowing outer names)
  2. Infer each def body with mutual env
  3. Infer `in` body
  4. Restore outer `defs` (scope exits)
- The unifier's occurs check handles genuinely circular types

#### 3.5 Lowering (`lower.rs`)
- Handle `Expr::Let`: lower each def, then lower the body
- Lowering semantics: `let { defs } in body` → inline all def references in body
- Existing lowering for `where` defs works similarly

#### 3.6 Tests
- `let` expression: `let g = _ * 0.5 in g` produces half input
- Mutual recursion: `even = ...; odd = ...; main = even(_)` 
- Top-level multi-def: `gain = _ * 0.5; main = gain`
- `where` mutual: definitions reference each other regardless of order
- Self-reference: `f = f(_)` — should produce occurs-check error in inference
- Layout-based `let`: indentation-based let block
- `let` inside `where` and vice versa

#### 3.7 Documentation
- Update `rill-lang.md` with new grammar, binding rules, examples

### 4. Risk analysis

| Risk | Mitigation |
|------|-----------|
| Occurs check may reject valid signal programs | Start with conservative occurs check; refine if false positives found |
| Mutual recursion in `let` may conflict with block scheduling | The SCC scheduler already handles mutual dependencies correctly |
| Breaking change for existing DSL programs (`main` is now a regular def) | Backward compat: `main = body where { defs }` is still valid syntax |
| Deep mutual groups may cause inference complexity | HM inference with let-polymorphism is polynomial; practical programs are shallow |

### 5. Out of scope

- Type signatures (`sq :: Int -> Int`)
- User-defined data types
- Pattern matching in parameters
- Guards
- Where clauses on individual cases (rill-lang has no case/pattern matching)
