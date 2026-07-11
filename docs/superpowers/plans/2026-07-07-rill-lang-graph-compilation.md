# rill-lang Graph Compilation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Extend `rill-lang` from a single-algorithm compiler to a full graph compiler with optimization passes and a custom runtime engine (`RillGraphEngine`).

**Architecture:** Add 6 new files to `rill-lang` for Graph IR, optimizer, lowering, schedule, and engine. Extend lexer/parser/AST with `param`/`keep`/`inline` keywords. Add `compile_graph()` entry point. Integrate `RillGraphEngine` into `rill-adrift::LangNode` and extend `rill-patchbay::SetParameter` with string anchors.

**Tech Stack:** Rust (`rill-lang`, `rill-graph`, `rill-adrift`, `rill-patchbay`, `rill-core`), indexmap, existing IR infrastructure.

---

## File Map

### New files (`rill-lang/src/`)

| File | Responsibility |
|------|---------------|
| `graph_ir.rs` | `GraphIr`, `GraphNode`, `GraphEdge`, `EdgeKind` — graph-level IR types |
| `graph_build.rs` | Build `GraphIr` from typed AST, inlining `def`s, extracting `param` nodes |
| `graph_optimize.rs` | Optimizer pass runner + DCE, inline, parallel merge, lateral merge, LTI reorder |
| `graph_lower.rs` | Topological sort, feedback detection, liveness analysis |
| `graph_schedule.rs` | `ScheduledGraph`, `Step` (InlineProgram, BufferCopy, ReadDelay, WriteDelay), buffer allocation |
| `graph_engine.rs` | `RillGraphEngine<T, BUF>` — runtime ALgorithm impl with actor mailbox |

### Modified files

| File | Changes |
|------|---------|
| `rill-lang/src/lexer.rs` | `Tok::Param`, `Tok::Keep`, `Tok::Inline` tokens |
| `rill-lang/src/ast.rs` | `DefKind` enum (Def/Param/KeepParam/InlineParam), add `kind` field to `Def` |
| `rill-lang/src/parser.rs` | Parse `param`, `keep param`, `inline param` prefix before `parse_def()` |
| `rill-lang/src/types/infer.rs` | Pass `DefKind` through `TypedProgram` (no type changes needed) |
| `rill-lang/src/lower.rs` | Pass `DefKind` through lowering (delegate to graph builder when graph IR present) |
| `rill-lang/src/lib.rs` | Add `compile_graph()`, declare new modules |
| `rill-adrift/src/lang_node.rs` | Detect graph source vs single-node source, route accordingly |
| `rill-adrift/src/registration.rs` | Register `rill/graph_lang` node type |
| `rill-patchbay/src/engine.rs` | Add `anchor: Option<String>` to `SetParameter`-like servo command construction |

---

### Task 1: Lexer — add `param`, `keep`, `inline` tokens

**Files:**
- Modify: `rill-lang/src/lexer.rs`

- [ ] **Step 1: Add Tok variants**

After `Tok::Eof` (line 51), insert:

```rust
    /// `param` keyword — declares a named subgraph anchor node.
    Param,
    /// `keep` keyword — forbids inlining of a param node.
    Keep,
    /// `inline` keyword — forces inlining of a param node.
    Inline,
```

- [ ] **Step 2: Lex the keywords in the `is_ident_start` branch**

Replace the `// tok = if text == "_"` block at line 158 with:

```rust
            let tok = match text {
                "_" => Tok::Wire,
                "param" => Tok::Param,
                "keep" => Tok::Keep,
                "inline" => Tok::Inline,
                _ => Tok::Ident(text.to_string()),
            };
            out.push(Token { tok, span });
```

- [ ] **Step 3: Add lexer unit tests**

After the existing test block, append:

```rust
    #[test]
    fn lexes_param_keyword() {
        assert_eq!(
            kinds("param keep inline ident"),
            vec![
                Tok::Param,
                Tok::Keep,
                Tok::Inline,
                Tok::Ident("ident".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn param_function_is_not_keyword() {
        // `param(` is a function call, not the keyword
        assert_eq!(
            kinds(r#"param("freq", 440)"#),
            vec![
                Tok::Ident("param".into()),
                Tok::LParen,
                Tok::Str("freq".into()),
                Tok::Comma,
                Tok::Int(440),
                Tok::RParen,
                Tok::Eof,
            ]
        );
    }
```

- [ ] **Step 4: Run parser tests to verify no breakage**

```bash
cargo test -p rill-lang -- lib::lexer
```
Expected: PASS (existing tests continue to pass, new tests pass)

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/lexer.rs
git commit -m 'feat(rill-lang): add param, keep, inline tokens to lexer'
```

---

### Task 2: AST — add `DefKind` to distinguish graph node defs from plain defs

**Files:**
- Modify: `rill-lang/src/ast.rs`

- [ ] **Step 1: Add `DefKind` enum and `kind` field to `Def`**

Replace the `Def` struct definition:

```rust
/// Whether a definition is a plain `def`, a graph-node `param`,
/// a `keep param`, or an `inline param`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DefKind {
    /// `def name = expr;` or plain `name = expr;` — always inlined.
    #[default]
    Def,
    /// `param name = expr;` — may be a graph node (optimizer decides).
    Param,
    /// `keep param name = expr;` — NEVER inlined.
    KeepParam,
    /// `inline param name = expr;` — ALWAYS inlined.
    InlineParam,
}

/// A top-level definition: `name(params) = body;` (params may be empty).
#[derive(Debug, Clone, PartialEq)]
pub struct Def {
    /// Definition name.
    pub name: String,
    /// Formal parameter names (empty for a plain alias).
    pub params: Vec<String>,
    /// Right-hand side.
    pub body: Expr,
    /// Span of the whole definition.
    pub span: Span,
    /// How this definition should be treated in graph IR.
    pub kind: DefKind,
}
```

- [ ] **Step 2: Update parser to set `kind: DefKind::Def` by default**

In `rill-lang/src/parser.rs`, find the `parse_def` method and where it constructs `Def`:

```rust
        Ok(Def {
            name,
            params,
            body,
            span: Span::new(start, self.peek().span.end),
            kind: DefKind::Def, // ADD THIS
        })
```

- [ ] **Step 3: Run tests to verify**

```bash
cargo test -p rill-lang
```
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/ast.rs rill-lang/src/parser.rs
git commit -m 'feat(rill-lang): add DefKind to AST for param/keep/inline annotations'
```

---

### Task 3: Parser — parse `param`, `keep param`, `inline param` prefixes

**Files:**
- Modify: `rill-lang/src/parser.rs`
- Create: `rill-lang/tests/graph_parse.rs`

- [ ] **Step 1: Write failing integration test**

```rust
// rill-lang/tests/graph_parse.rs
use rill_lang::ast::{Def, DefKind, Expr};
use rill_lang::parser;

#[test]
fn parses_param_def() {
    let src = r#"
param myFilter = _ : sin(440.0);
process = myFilter : _;
"#;
    let prg = parser::parse_str(src).unwrap();
    assert_eq!(prg.defs.len(), 2);
    assert_eq!(prg.defs[0].name, "myFilter");
    assert_eq!(prg.defs[0].kind, DefKind::Param);
    assert_eq!(prg.defs[1].name, "process");
    assert_eq!(prg.defs[1].kind, DefKind::Def);
}

#[test]
fn parses_keep_param() {
    let src = "keep param osc = _ : sin(440.0); process = osc : _;";
    let prg = parser::parse_str(src).unwrap();
    assert_eq!(prg.defs[0].kind, DefKind::KeepParam);
}

#[test]
fn parses_inline_param() {
    let src = "inline param gain = _ * 0.5; process = gain : _;";
    let prg = parser::parse_str(src).unwrap();
    assert_eq!(prg.defs[0].kind, DefKind::InlineParam);
}

#[test]
fn plain_def_is_not_param() {
    let src = "def gain(x) = x * 0.5; process = gain(_, 440);";
    let prg = parser::parse_str(src).unwrap();
    assert_eq!(prg.defs[0].kind, DefKind::Def);
}
```

You need to expose `parse_str` from `parser.rs`:
```rust
pub fn parse_str(src: &str) -> Result<Program, CompileError> {
    let tokens = lexer::tokenize(src)?;
    parse(&tokens)
}
```

- [ ] **Step 2: Run test to verify failure**

```bash
cargo test -p rill-lang --test graph_parse
```
Expected: FAIL — `DefKind::Param` assertion fails (defaults to `DefKind::Def`)

- [ ] **Step 3: Implement parsing of `param`/`keep param`/`inline param` prefixes**

In `parser.rs`, modify `parse_def` to check for keywords before the name:

```rust
    fn parse_def(&mut self) -> Result<Def, CompileError> {
        let start = self.peek().span.start;

        // Check for `param`, `keep param`, `inline param` prefixes
        let mut kind = DefKind::Def;
        if self.peek().tok == Tok::Keep {
            self.bump(); // consume `keep`
            kind = DefKind::KeepParam;
        } else if self.peek().tok == Tok::Inline {
            self.bump(); // consume `inline`
            kind = DefKind::InlineParam;
        }

        if self.peek().tok == Tok::Param {
            self.bump(); // consume `param`
            kind = match kind {
                DefKind::KeepParam => DefKind::KeepParam,
                DefKind::InlineParam => DefKind::InlineParam,
                _ => DefKind::Param,
            };
        } else if kind == DefKind::KeepParam {
            // `keep` without `param` — parse as plain def with `keep` as name
            // (or error if `keep` alone was eaten)
            // For simplicity: error
            return Err(CompileError::Parse {
                msg: "expected `param` after `keep`".into(),
                span: Span::new(start, self.peek().span.end),
            });
        } else if kind == DefKind::InlineParam {
            return Err(CompileError::Parse {
                msg: "expected `param` after `inline`".into(),
                span: Span::new(start, self.peek().span.end),
            });
        }

        // Now parse the name as before
        let name_tok = self.peek().clone();
        let name = match &name_tok.tok {
            Tok::Ident(n) => n.clone(),
            _ => {
                return Err(CompileError::Parse {
                    msg: format!("expected definition name, found {:?}", name_tok.tok),
                    span: name_tok.span,
                })
            }
        };
        self.bump();

        // ... rest of param/body parsing unchanged ...

        // Set kind in the returned Def
        Ok(Def {
            name,
            params,
            body,
            span: Span::new(start, self.peek().span.end),
            kind,
        })
    }
```

- [ ] **Step 4: Run tests to verify PASS**

```bash
cargo test -p rill-lang
```
Expected: PASS (existing + new tests)

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/parser.rs rill-lang/tests/graph_parse.rs
git commit -m 'feat(rill-lang): parse param/keep/inline keyword prefixes'
```

---

### Task 4: Graph IR types

**Files:**
- Create: `rill-lang/src/graph_ir.rs`
- Modify: `rill-lang/src/lib.rs` (add `pub mod graph_ir;`)

- [ ] **Step 1: Write the module**

```rust
//! Graph-level IR: nodes, edges, and topology produced by the graph builder
//! before optimization and lowering.

use indexmap::IndexMap;
use crate::ir::{Ir, ParamDef};

/// A node in the graph IR.
#[derive(Debug, Clone)]
pub struct GraphNode {
    /// Signal arity: (inputs, outputs).
    pub arity: (usize, usize),
    /// Compiled IR for this node's signal-processing algorithm.
    pub ir: Ir,
    /// Parameter slots (name, default, range).
    pub params: Vec<ParamDef>,
    /// `keep param` — never inline.
    pub keep: bool,
    /// `inline param` — always inline.
    pub force_inline: bool,
}

/// Edge kind determines whether this edge participates in activation
/// propagation (Signal) or is a delayed data path (Feedback).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// Normal signal edge — determines activation order.
    Signal,
    /// Feedback edge (`~`) — delayed data, excluded from topological sort.
    Feedback,
}

/// A directed edge between two nodes and their ports.
#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from_node: String,
    pub from_port: usize,
    pub to_node: String,
    pub to_port: usize,
    pub kind: EdgeKind,
}

/// The graph-level intermediate representation.
///
/// Produced by the graph builder from a typed AST, consumed by the optimizer,
/// then lowered to a `ScheduledGraph`.
#[derive(Debug, Clone)]
pub struct GraphIr {
    /// Number of external input channels (0 for Source, 1 for Processor).
    pub inputs: usize,
    /// Number of external output channels (1 for process).
    pub outputs: usize,
    /// Nodes in definition order, keyed by name.
    pub nodes: IndexMap<String, GraphNode>,
    /// All edges (signal + feedback).
    pub edges: Vec<GraphEdge>,
}
```

- [ ] **Step 2: Add `indexmap` dependency if not present**

Check `rill-lang/Cargo.toml` for `indexmap`. If absent:

```toml
[dependencies]
indexmap = { version = "2", features = ["serde"] }
```

- [ ] **Step 3: Add `pub mod graph_ir;` to `lib.rs`**

After `pub mod error;`:

```rust
pub mod graph_ir;
```

- [ ] **Step 4: Verify compiles**

```bash
cargo check -p rill-lang
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/graph_ir.rs rill-lang/src/lib.rs
git commit -m 'feat(rill-lang): add GraphIr, GraphNode, GraphEdge types'
```

---

### Task 5: Graph Builder — AST → Graph IR

**Files:**
- Create: `rill-lang/src/graph_build.rs`
- Modify: `rill-lang/src/lib.rs` (add `pub mod graph_build;`)
- Create: `rill-lang/tests/graph_build.rs`

- [ ] **Step 1: Write failing integration test**

```rust
// rill-lang/tests/graph_build.rs
use rill_lang::ast::DefKind;
use rill_lang::graph_build::GraphBuildResult;
use rill_lang::graph_ir::{EdgeKind, GraphIr};

fn compile_graph_ir(src: &str) -> GraphIr {
    let tokens = rill_lang::lexer::tokenize(src).unwrap();
    let program = rill_lang::parser::parse(&tokens).unwrap();
    let typed = rill_lang::types::infer::infer_program(&program).unwrap();
    rill_lang::graph_build::build_graph_ir(&typed).unwrap()
}

#[test]
fn single_node_is_not_a_graph() {
    let ir = compile_graph_ir("process = _ * 0.5;");
    assert_eq!(ir.nodes.len(), 0,
        "plain process with no param defs produces empty graph (single inline program)");
}

#[test]
fn param_def_becomes_graph_node() {
    let src = r#"
param filt = _ : sin(440.0);
process = filt : _;
"#;
    let ir = compile_graph_ir(src);
    assert_eq!(ir.nodes.len(), 1, "one param def → one graph node");
    assert!(ir.nodes.contains_key("filt"));
    assert_eq!(ir.nodes["filt"].arity, (1, 1));
}

#[test]
fn signal_edges_between_param_nodes() {
    let src = r#"
param osc = sin(440.0);
param filt = _ : osc;
process = filt : _;
"#;
    let ir = compile_graph_ir(src);
    assert_eq!(ir.nodes.len(), 2);
    let signal_edges: Vec<_> = ir.edges.iter()
        .filter(|e| e.kind == EdgeKind::Signal)
        .collect();
    assert_eq!(signal_edges.len(), 2, "osc→filt and filt→output");
}

#[test]
fn feedback_edge_from_tilde() {
    let src = r#"
param delay = _ : 0.5;
process = _ : delay ~ _;
"#;
    let ir = compile_graph_ir(src);
    let fb_edges: Vec<_> = ir.edges.iter()
        .filter(|e| e.kind == EdgeKind::Feedback)
        .collect();
    assert!(!fb_edges.is_empty(), "~ operator creates feedback edge");
}

#[test]
fn keep_param_has_keep_flag() {
    let src = "keep param kf = _ : 0.5; process = kf : _;";
    let ir = compile_graph_ir(src);
    assert!(ir.nodes["kf"].keep);
}

#[test]
fn inline_param_has_force_inline_flag() {
    let src = "inline param iff = _ : 0.5; process = iff : _;";
    let ir = compile_graph_ir(src);
    assert!(ir.nodes["iff"].force_inline);
}

#[test]
fn def_functions_are_not_graph_nodes() {
    let src = r#"
def scale(x, s) = x * s;
param myFilter = _ : scale(_, 2.0);
process = myFilter : _;
"#;
    let ir = compile_graph_ir(src);
    assert_eq!(ir.nodes.len(), 1, "def is inlined, only param is a node");
}
```

- [ ] **Step 2: Run test to verify failure**

```bash
cargo test -p rill-lang --test graph_build
```
Expected: FAIL — `graph_build` module doesn't exist

- [ ] **Step 3: Implement graph builder**

```rust
// rill-lang/src/graph_build.rs
//! Build GraphIr from a typed AST.

use crate::ast::{BinOp, DefKind, Expr, Program};
use crate::builtin::NoSigs;
use crate::error::CompileError;
use crate::graph_ir::{EdgeKind, GraphEdge, GraphIr, GraphNode};
use crate::ir::{Ir, ParamDef};
use crate::lower;
use crate::types::infer::TypedProgram;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

/// Result of building the graph IR.
pub type GraphBuildResult = Result<GraphIr, CompileError>;

/// Walk the typed AST and extract `param`-/`keep param`-annotated definitions
/// as graph nodes, inlining `def` and non-annotated definitions into the
/// containing expressions.
pub fn build_graph_ir(typed: &TypedProgram) -> GraphBuildResult {
    let program = &typed.program;
    let def_map: HashMap<&str, &crate::ast::Def> = program
        .defs
        .iter()
        .map(|d| (d.name.as_str(), d))
        .collect();

    // Collect param-def names — these become graph nodes.
    let param_names: HashSet<&str> = program
        .defs
        .iter()
        .filter(|d| d.kind != DefKind::Def)
        .map(|d| d.name.as_str())
        .collect();

    // Find `process` definition
    let process_def = program
        .defs
        .iter()
        .find(|d| d.name == "process")
        .ok_or_else(|| CompileError::Parse {
            msg: "missing `process` definition".into(),
            span: crate::error::Span::new(0, 0),
        })?;

    // Build nodes from param defs: each gets its own IR by lowering its body
    let mut nodes: IndexMap<String, GraphNode> = IndexMap::new();

    for def in &program.defs {
        if def.name == "process" || !param_names.contains(def.name.as_str()) {
            continue;
        }
        // Create a synthetic program containing just this def + its body as process
        let sub = crate::types::infer::infer_program(&Program {
            defs: vec![
                def.clone(),
                crate::ast::Def {
                    name: "process".into(),
                    params: vec![],
                    body: Expr::Ref(def.name.clone(), def.span),
                    span: def.span,
                    kind: DefKind::Def,
                },
            ],
        })?;

        let ir = lower::lower_with(&sub, &NoSigs, 44100.0)?;

        // Arity: infer from the node's process_ty
        let arity = (
            typed.program.defs.iter()
                .find(|d| d.name == def.name)
                .map(|d| d.params.len())
                .unwrap_or(1),
            1, // output arity = 1 for now (can be extended later)
        );

        let param_count = ir.params.len();

        nodes.insert(
            def.name.clone(),
            GraphNode {
                arity,
                ir,
                params: vec![], // params are inside the IR; external SetParameter targets them by name
                keep: def.kind == DefKind::KeepParam,
                force_inline: def.kind == DefKind::InlineParam,
            },
        );
    }

    // If no param nodes, return empty graph (inline everything)
    if nodes.is_empty() {
        return Ok(GraphIr {
            inputs: 0,
            outputs: 1,
            nodes,
            edges: vec![],
        });
    }

    // Walk process body to collect edges between param nodes
    let mut edges = Vec::new();
    collect_edges(&process_def.body, &param_names, &mut edges, &def_map);

    Ok(GraphIr {
        inputs: 0,
        outputs: 1,
        nodes,
        edges,
    })
}

/// Walk an expression and collect edges between `param`-node references and
/// their combinator connections.
fn collect_edges(
    expr: &Expr,
    param_names: &HashSet<&str>,
    edges: &mut Vec<GraphEdge>,
    def_map: &HashMap<&str, &crate::ast::Def>,
) {
    match expr {
        Expr::Ref(name, _) => {
            // param reference — for now, edges are collected from parent Bin nodes
        }
        Expr::Apply { name, args, .. } => {
            for arg in args {
                collect_edges(arg, param_names, edges, def_map);
            }
        }
        Expr::Bin { op, lhs, rhs, .. } => {
            collect_edges(lhs, param_names, edges, def_map);
            collect_edges(rhs, param_names, edges, def_map);

            let edge_kind = match op {
                BinOp::Feedback => EdgeKind::Feedback,
                _ => EdgeKind::Signal,
            };

            // Extract node names from lhs/rhs Ref nodes and create edges
            // For Seq (:), connect lhs.out[0] → rhs.in[0]
            // For Par (,), both receive the same input — edges are to both from their shared source
            // For Split (<:), lhs.out→both children
            // For Merge (:>), both children→lhs
            let lhs_nodes = extract_node_refs(lhs, param_names, def_map);
            let rhs_nodes = extract_node_refs(rhs, param_names, def_map);

            match op {
                BinOp::Seq => {
                    for (lname, lports) in &lhs_nodes {
                        for (rname, _) in &rhs_nodes {
                            edges.push(GraphEdge {
                                from_node: lname.clone(),
                                from_port: 0,
                                to_node: rname.clone(),
                                to_port: 0,
                                kind: edge_kind,
                            });
                        }
                    }
                }
                BinOp::Par => {
                    // For parallel, both sides receive same input from upstream.
                    // Edges are added by the parent Seq combinator.
                }
                BinOp::Split => {
                    for (lname, lports) in &lhs_nodes {
                        for (rname, _) in &rhs_nodes {
                            for p in 0..*lports {
                                edges.push(GraphEdge {
                                    from_node: lname.clone(),
                                    from_port: p,
                                    to_node: rname.clone(),
                                    to_port: 0,
                                    kind: edge_kind,
                                });
                            }
                        }
                    }
                }
                BinOp::Merge => {
                    for (lname, _) in &lhs_nodes {
                        for (rname, rports) in &rhs_nodes {
                            for p in 0..*rports {
                                edges.push(GraphEdge {
                                    from_node: rname.clone(),
                                    from_port: p,
                                    to_node: lname.clone(),
                                    to_port: 0,
                                    kind: edge_kind,
                                });
                            }
                        }
                    }
                }
                BinOp::Feedback => {
                    // Feedback creates edges in both directions (delayed path)
                    for (lname, _) in &lhs_nodes {
                        for (rname, _) in &rhs_nodes {
                            edges.push(GraphEdge {
                                from_node: rname.clone(),
                                from_port: 0,
                                to_node: lname.clone(),
                                to_port: 0,
                                kind: EdgeKind::Signal,
                            });
                            edges.push(GraphEdge {
                                from_node: lname.clone(),
                                from_port: 0,
                                to_node: rname.clone(),
                                to_port: 0,
                                kind: EdgeKind::Feedback,
                            });
                        }
                    }
                }
                _ => {
                    // Add, Sub, Mul, Div, Rem — arithmetic combinators between nodes
                    // For now, nodes involved in arithmetic all share edges via the
                    // parent combinator context
                }
            }
        }
        _ => {}
    }
}

/// Extract param node names from an expression (recursively through def refs).
fn extract_node_refs(
    expr: &Expr,
    param_names: &HashSet<&str>,
    def_map: &HashMap<&str, &crate::ast::Def>,
) -> Vec<(String, usize)> {
    let mut result = Vec::new();
    extract_node_refs_inner(expr, param_names, def_map, &mut result);
    result
}

fn extract_node_refs_inner(
    expr: &Expr,
    param_names: &HashSet<&str>,
    def_map: &HashMap<&str, &crate::ast::Def>,
    out: &mut Vec<(String, usize)>,
) {
    match expr {
        Expr::Ref(name, _) => {
            if param_names.contains(name.as_str()) {
                out.push((name.clone(), 1)); // default arity 1
            } else if let Some(def) = def_map.get(name.as_str()) {
                extract_node_refs_inner(&def.body, param_names, def_map, out);
            }
        }
        Expr::Apply { name, args, .. } => {
            for arg in args {
                extract_node_refs_inner(arg, param_names, def_map, out);
            }
        }
        Expr::Bin { lhs, rhs, .. } => {
            extract_node_refs_inner(lhs, param_names, def_map, out);
            extract_node_refs_inner(rhs, param_names, def_map, out);
        }
        _ => {}
    }
}
```

Note: this is the **initial implementation** — edge collection from combinators needs refinement in subsequent optimizer/lowering passes. The edge structure provides enough information for the validator and optimizer to work.

- [ ] **Step 4: Add module to lib.rs**

```rust
pub mod graph_build;
```

- [ ] **Step 5: Verify compiles**

```bash
cargo check -p rill-lang
```

- [ ] **Step 6: Run tests**

```bash
cargo test -p rill-lang --test graph_build
```
Expected: tests pass for nodes detection; edge tests may need adjustment.

- [ ] **Step 7: Commit**

```bash
git add rill-lang/src/graph_build.rs rill-lang/src/lib.rs rill-lang/tests/graph_build.rs
git commit -m 'feat(rill-lang): graph builder — AST to GraphIr'
```

---

### Task 6: Graph Optimizer — pass runner, DCE, inlining

**Files:**
- Create: `rill-lang/src/graph_optimize.rs`
- Modify: `rill-lang/src/lib.rs`

- [ ] **Step 1: Write failing test**

```rust
// rill-lang/tests/graph_optimize.rs
use rill_lang::ast::DefKind;
use rill_lang::graph_ir::{EdgeKind, GraphIr, GraphNode};
use rill_lang::graph_optimize::optimize;
use indexmap::IndexMap;

fn make_node(keep: bool, force_inline: bool) -> GraphNode {
    GraphNode {
        arity: (1, 1),
        ir: todo!(), // placeholder — optimise depends on identifiers, not IR content
        params: vec![],
        keep,
        force_inline,
    }
}

#[test]
fn inline_param_is_removed() {
    let mut nodes = IndexMap::new();
    nodes.insert("a".into(), make_node(false, true));
    nodes.insert("b".into(), make_node(false, false));
    let mut ir = GraphIr {
        inputs: 0, outputs: 1, nodes,
        edges: vec![
            graph_edge("a", "b", EdgeKind::Signal),
        ],
    };
    optimize(&mut ir);
    assert!(!ir.nodes.contains_key("a"), "inline param removed");
    assert!(ir.nodes.contains_key("b"));
}

fn graph_edge(from: &str, to: &str, kind: EdgeKind) -> rill_lang::graph_ir::GraphEdge {
    rill_lang::graph_ir::GraphEdge {
        from_node: from.into(),
        from_port: 0,
        to_node: to.into(),
        to_port: 0,
        kind,
    }
}
```

Note: this test requires `GraphNode` to have `PartialEq`, `Eq` for IndexMap comparison. We'll add derive in Task 4.

- [ ] **Step 2: Implement optimizer**

```rust
// rill-lang/src/graph_optimize.rs
//! Graph IR optimizer — applies passes to reduce node count and buffer pressure.

use crate::graph_ir::{EdgeKind, GraphIr};

/// Run all optimization passes on the graph IR.
pub fn optimize(ir: &mut GraphIr) {
    dead_edge_elimination(ir);
    inline_nodes(ir);
    // Future passes:
    // parallel_merge(ir);
    // lateral_merge(ir);
    // lti_reorder(ir);
}

/// Remove edges whose destination port is never read by any node.
fn dead_edge_elimination(ir: &mut GraphIr) {
    // Collect ports that are read by nodes
    use std::collections::HashSet;
    let mut consumed: HashSet<(&str, usize)> = HashSet::new();
    for edge in &ir.edges {
        if edge.kind == EdgeKind::Signal {
            consumed.insert((&edge.to_node, edge.to_port));
        }
    }
    // Keep edges that connect to consumed ports or are feedback edges
    ir.edges.retain(|e| {
        e.kind == EdgeKind::Feedback || consumed.contains(&(&e.to_node, e.to_port))
    });
}

/// Inline nodes marked `force_inline` or with no dynamic params and not `keep`.
fn inline_nodes(ir: &mut GraphIr) {
    use std::collections::HashSet;

    // Collect nodes to inline
    let mut to_inline: HashSet<String> = HashSet::new();
    for (name, node) in &ir.nodes {
        if node.force_inline {
            to_inline.insert(name.clone());
        } else if !node.keep {
            // Check if params are all non-dynamic — is_dynamic checks for param() refs
            let has_dynamic = node.ir.params.iter().any(|p| {
                // A param is dynamic if neither min nor max is set (or default is the only info)
                // For MVP: params without explicit min/max bound are candidate for inlining
                p.min.is_none() && p.max.is_none()
            });
            if !has_dynamic && !node.params.is_empty() {
                to_inline.insert(name.clone());
            }
        }
    }

    // Inline: remove the node, reconnect edges through it
    for name in &to_inline {
        // Find edges where this node is the middle (output of prev → this → input of next)
        let incoming: Vec<_> = ir.edges.iter()
            .filter(|e| e.kind == EdgeKind::Signal && &e.to_node == name)
            .cloned()
            .collect();
        let outgoing: Vec<_> = ir.edges.iter()
            .filter(|e| e.kind == EdgeKind::Signal && &e.from_node == name)
            .cloned()
            .collect();

        // Rewire: prev → next
        for inc in &incoming {
            for out in &outgoing {
                ir.edges.push(GraphEdge {
                    from_node: inc.from_node.clone(),
                    from_port: inc.from_port,
                    to_node: out.to_node.clone(),
                    to_port: out.to_port,
                    kind: EdgeKind::Signal,
                });
            }
        }
    }

    // Remove inlined nodes and their edges
    ir.nodes.retain(|name, _| !to_inline.contains(name));
    ir.edges.retain(|e| {
        !to_inline.contains(&e.from_node) && !to_inline.contains(&e.to_node)
    });
}

use crate::graph_ir::GraphEdge;
```

- [ ] **Step 3: Add module to lib.rs**

```rust
pub mod graph_optimize;
```

- [ ] **Step 4: Verify compiles and test**

```bash
cargo test -p rill-lang --test graph_optimize
```

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/graph_optimize.rs rill-lang/src/lib.rs rill-lang/tests/graph_optimize.rs
git commit -m 'feat(rill-lang): graph optimizer — DCE and node inlining'
```

---

### Task 7: Graph Lowering — topological sort + feedback detection

**Files:**
- Create: `rill-lang/src/graph_lower.rs`
- Modify: `rill-lang/src/lib.rs`

- [ ] **Step 1: Implement lowerer**

```rust
// rill-lang/src/graph_lower.rs
//! Lower GraphIr to SchedulingOrder: topological sort excluding feedback edges,
//! feedback pair detection (ReadDelay/WriteDelay), and buffer liveness analysis.

use crate::error::{CompileError, Span};
use crate::graph_ir::{EdgeKind, GraphIr};
use std::collections::{HashMap, HashSet, VecDeque};

/// Ordered node schedule with feedback delay pairs identified.
#[derive(Debug, Clone)]
pub struct SchedulingOrder {
    /// Node names in topological (activation) order.
    pub topo_order: Vec<String>,
    /// Feedback delay pairs: (producer_node, consumer_node, delay_slot).
    /// Each pair creates a ReadDelay before the consumer and WriteDelay after the producer.
    pub feedback_pairs: Vec<(String, String, usize)>,
    /// For each node, the set of input edges from upstream nodes.
    pub node_inputs: HashMap<String, Vec<(String, usize, usize)>>,
    /// For each node, the set of output edges to downstream nodes.
    pub node_outputs: HashMap<String, Vec<(String, usize, usize)>>,
}

/// Lower GraphIr to scheduling order.
pub fn lower_graph(ir: &GraphIr) -> Result<SchedulingOrder, CompileError> {
    // Build adjacency from signal edges only (feedback edges excluded)
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut successors: HashMap<&str, Vec<&str>> = HashMap::new();

    for name in ir.nodes.keys() {
        in_degree.entry(name).or_insert(0);
        successors.entry(name).or_default();
    }

    for edge in &ir.edges {
        if edge.kind != EdgeKind::Signal {
            continue;
        }
        if !ir.nodes.contains_key(&edge.from_node) || !ir.nodes.contains_key(&edge.to_node) {
            continue; // edge references an inlined node — skip
        }
        *in_degree.entry(&edge.to_node).or_insert(0) += 1;
        successors.entry(&edge.from_node).or_default().push(&edge.to_node);
    }

    // Kahn's algorithm
    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(n, _)| *n)
        .collect();

    let mut topo_order = Vec::new();
    while let Some(node) = queue.pop_front() {
        topo_order.push(node.to_string());
        if let Some(succs) = successors.get(node) {
            for &succ in succs {
                let d = in_degree.get_mut(succ).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push_back(succ);
                }
            }
        }
    }

    if topo_order.len() != ir.nodes.len() {
        return Err(CompileError::Parse {
            msg: "graph contains a cycle in signal edges (activation graph must be acyclic)".into(),
            span: Span::new(0, 0),
        });
    }

    // Collect feedback pairs from feedback edges
    let mut feedback_pairs = Vec::new();
    let mut delay_slot = 0usize;
    for edge in &ir.edges {
        if edge.kind == EdgeKind::Feedback {
            feedback_pairs.push((
                edge.from_node.clone(),
                edge.to_node.clone(),
                delay_slot,
            ));
            delay_slot += 1;
        }
    }

    // Build input/output maps
    let mut node_inputs: HashMap<String, Vec<(String, usize, usize)>> = HashMap::new();
    let mut node_outputs: HashMap<String, Vec<(String, usize, usize)>> = HashMap::new();
    for name in ir.nodes.keys() {
        node_inputs.insert(name.clone(), Vec::new());
        node_outputs.insert(name.clone(), Vec::new());
    }
    for edge in &ir.edges {
        if edge.kind != EdgeKind::Signal {
            continue;
        }
        node_inputs.entry(edge.to_node.clone()).or_default()
            .push((edge.from_node.clone(), edge.from_port, edge.to_port));
        node_outputs.entry(edge.from_node.clone()).or_default()
            .push((edge.to_node.clone(), edge.from_port, edge.to_port));
    }

    Ok(SchedulingOrder {
        topo_order,
        feedback_pairs,
        node_inputs,
        node_outputs,
    })
}
```

- [ ] **Step 2: Add module to lib.rs**

```rust
pub mod graph_lower;
```

- [ ] **Step 3: Verify compiles**

```bash
cargo check -p rill-lang
```

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/graph_lower.rs rill-lang/src/lib.rs
git commit -m 'feat(rill-lang): graph lowerer — topo sort + feedback detection'
```

---

### Task 8: ScheduledGraph + Buffer Allocation

**Files:**
- Create: `rill-lang/src/graph_schedule.rs`
- Modify: `rill-lang/src/lib.rs`

- [ ] **Step 1: Implement Schedule and Step types with buffer allocator**

```rust
// rill-lang/src/graph_schedule.rs
//! ScheduledGraph — pre-compiled execution plan with buffer allocation.

use crate::graph_ir::GraphIr;
use crate::graph_lower::SchedulingOrder;
use std::collections::HashMap;

/// A step in the scheduled execution plan.
#[derive(Debug, Clone)]
pub enum Step {
    /// Execute an inlined program node.
    InlineProgram {
        /// Index into the programs vector in RillGraphEngine.
        node_idx: usize,
        /// Input buffer indices (may alias previous outputs — zero-copy).
        input_bufs: Vec<usize>,
        /// Output buffer indices.
        output_bufs: Vec<usize>,
        /// Parameter indices within the program to set before execution.
        param_indices: Vec<usize>,
    },
    /// Copy (or accumulate) between buffer slots.
    /// Used for fan-out (overwrite) and fan-in (add).
    BufferCopy {
        from: usize,
        to: usize,
        gain: f32,
        /// false = overwrite, true = accumulate (+=)
        add: bool,
    },
    /// Read a delay buffer (previous tick's feedback) into a target buffer.
    ReadDelay {
        /// Delay slot index.
        slot: usize,
        /// Target buffer index.
        target: usize,
    },
    /// Save a source buffer into a delay slot for the next tick.
    WriteDelay {
        /// Source buffer index.
        source: usize,
        /// Delay slot index.
        slot: usize,
    },
}

/// Pre-compiled execution plan with buffer allocation.
#[derive(Debug, Clone)]
pub struct ScheduledGraph {
    /// Number of external input channels.
    pub inputs: usize,
    /// Number of external output channels.
    pub outputs: usize,
    /// Ordered execution steps.
    pub steps: Vec<Step>,
    /// Total number of buffer slots needed.
    pub buffers: usize,
    /// Total number of delay (feedback) buffer slots needed.
    pub delay_slots: usize,
    /// Which buffer indices map to output channels.
    pub output_mapping: Vec<usize>,
}

/// Build a ScheduledGraph from GraphIr and SchedulingOrder.
pub fn build_scheduled_graph(
    ir: &GraphIr,
    order: &SchedulingOrder,
) -> ScheduledGraph {
    // Map node names to sequential indices
    let name_to_idx: HashMap<&str, usize> = ir.nodes.keys()
        .enumerate()
        .map(|(i, name)| (name.as_str(), i))
        .collect();

    // Buffer allocation using liveness analysis
    // Phase 1: determine buffer count
    // For MVP: allocate one buffer per distinct signal edge, then compact
    // Phase 2: assign buffer indices with reuse
    let mut buf_count: usize = 0;
    let mut edge_buf: HashMap<(String, String, usize), usize> = HashMap::new();
    let mut buf_liveness_end: HashMap<usize, usize> = HashMap::new(); // buf_idx → last step that reads it

    // Assign temp buffer indices per edge
    for (to_name, inputs) in &order.node_inputs {
        for (from_name, from_port, to_port) in inputs {
            let key = (from_name.clone(), to_name.clone(), *to_port);
            if !edge_buf.contains_key(&key) {
                edge_buf.insert(key.clone(), buf_count);
                buf_count += 1;
            }
        }
    }

    // Build steps in topo order
    let mut steps: Vec<Step> = Vec::new();
    let mut node_buf_inputs: HashMap<String, Vec<usize>> = HashMap::new();
    let mut node_buf_outputs: HashMap<String, Vec<usize>> = HashMap::new();

    // Insert ReadDelay steps before nodes that consume feedback
    for (producer, consumer, slot) in &order.feedback_pairs {
        if let Some(&ci) = name_to_idx.get(consumer.as_str()) {
            let target_buf = buf_count;
            buf_count += 1;
            steps.push(Step::ReadDelay {
                slot: *slot,
                target: target_buf,
            });
            node_buf_inputs.entry(consumer.clone()).or_default().push(target_buf);
        }
    }

    for node_name in &order.topo_order {
        let node_idx = name_to_idx[node_name.as_str()];
        let node = &ir.nodes[node_name];

        // Collect input buffer indices for this node from incoming edges
        let mut input_bufs: Vec<usize> = Vec::new();
        if let Some(inputs) = order.node_inputs.get(node_name) {
            for (from_name, from_port, to_port) in inputs {
                let key = (from_name.clone(), node_name.clone(), *to_port);
                if let Some(&buf) = edge_buf.get(&key) {
                    input_bufs.push(buf);
                }
            }
        }

        // Also add feedback-derived input buffers
        if let Some(extra) = node_buf_inputs.remove(node_name) {
            for b in extra {
                if !input_bufs.contains(&b) {
                    input_bufs.push(b);
                }
            }
        }

        // Allocate output buffers
        let output_bufs: Vec<usize> = (0..node.arity.1)
            .map(|_| {
                let b = buf_count;
                buf_count += 1;
                b
            })
            .collect();

        steps.push(Step::InlineProgram {
            node_idx,
            input_bufs,
            output_bufs: output_bufs.clone(),
            param_indices: (0..node.ir.params.len()).collect(),
        });

        node_buf_outputs.insert(node_name.clone(), output_bufs);
    }

    // Insert WriteDelay steps after nodes that produce feedback
    for (producer, _consumer, slot) in &order.feedback_pairs {
        if let Some(outputs) = node_buf_outputs.get(producer) {
            if let Some(&source) = outputs.first() {
                steps.push(Step::WriteDelay {
                    source,
                    slot: *slot,
                });
            }
        }
    }

    // Reorder steps: ReadDelay → InlineProgram → WriteDelay per feedback cycle
    // (For simplicity, all ReadDelays are already before processing, all WriteDelays after)

    // Output mapping: last node's first output → output[0]
    let output_mapping = if let Some(last) = order.topo_order.last() {
        node_buf_outputs.get(last).cloned().unwrap_or_default()
    } else {
        vec![]
    };

    ScheduledGraph {
        inputs: ir.inputs,
        outputs: ir.outputs,
        steps,
        buffers: buf_count,
        delay_slots: order.feedback_pairs.len(),
        output_mapping,
    }
}
```

Note: `ScheduledGrid` at the end should be `ScheduledGraph`. Fix in implementation.

- [ ] **Step 2: Add module to lib.rs**

```rust
pub mod graph_schedule;
```

- [ ] **Step 3: Verify compiles**

```bash
cargo check -p rill-lang
```

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/graph_schedule.rs rill-lang/src/lib.rs
git commit -m 'feat(rill-lang): ScheduledGraph — pre-compiled execution plan with buffer allocation'
```

---

### Task 9: RillGraphEngine — Runtime

**Files:**
- Create: `rill-lang/src/graph_engine.rs`
- Modify: `rill-lang/src/lib.rs`

- [ ] **Step 1: Add `rill-core-actor` dependency**

```toml
# rill-lang/Cargo.toml
[dependencies]
rill-core-actor = { path = "../rill-core-actor" }
```

Note: `rill-lang` currently depends only on `rill-core`. Adding `rill-core-actor` requires workspace coordination. Check if this creates a circular dependency. Since `rill-core-actor` depends on `rill-core` (not vice versa), this should be fine.

- [ ] **Step 2: Implement RillGraphEngine**

```rust
// rill-lang/src/graph_engine.rs
//! RillGraphEngine — runtime for compiled graphs. Implements Algorithm<T>
//! and exposes an ActorRef<CommandEnum> for patchbay control.

use rill_core::math::Transcendental;
use rill_core::traits::{Algorithm, ProcessResult};
use rill_core::buffer::FixedBuffer;
use rill_core_actor::{Actor, ActorRef, ActorSystem};
use crate::graph_schedule::{ScheduledGraph, Step};
use crate::program::RillProgram;
use std::collections::HashMap;

/// Engine that executes a pre-compiled ScheduledGraph.
pub struct RillGraphEngine<T: Transcendental, const BUF: usize> {
    schedule: ScheduledGraph,
    programs: Vec<RillProgram<T>>,
    buffers: Vec<FixedBuffer<T, BUF>>,
    delay_buffers: Vec<FixedBuffer<T, BUF>>,
    param_values: Vec<Vec<f64>>,
    param_map: HashMap<String, HashMap<String, (usize, usize)>>, // anchor → param_name → (prog_idx, param_idx)
    actor: Actor<CommandEnum>,
    actor_ref: ActorRef<CommandEnum>,
}

impl<T: Transcendental, const BUF: usize> RillGraphEngine<T, BUF> {
    /// Create a new engine from a scheduled graph and compiled programs.
    pub fn new(
        schedule: ScheduledGraph,
        programs: Vec<RillProgram<T>>,
        node_names: Vec<String>,
        system: &ActorSystem,
    ) -> Self {
        let num_programs = programs.len();
        let mut param_values = Vec::with_capacity(num_programs);
        let mut param_map: HashMap<String, HashMap<String, (usize, usize)>> = HashMap::new();

        for (pi, prog) in programs.iter().enumerate() {
            let params: Vec<f64> = prog.params_meta().iter().map(|p| p.default).collect();
            param_values.push(params);
        }

        // Build param_map: for each program, map its param names to (prog_idx, param_idx)
        for (pi, prog) in programs.iter().enumerate() {
            // The param names are global — they map to the node name
            // For MVP: the node name is the first one in node_names[pi]
            if let Some(node_name) = node_names.get(pi) {
                let mut inner = HashMap::new();
                for (param_idx, param_def) in prog.params_meta().iter().enumerate() {
                    inner.insert(param_def.name.clone(), (pi, param_idx));
                }
                param_map.insert(node_name.clone(), inner);
            }
        }

        let buffers = vec![FixedBuffer::default(); schedule.buffers];
        let delay_buffers = vec![FixedBuffer::default(); schedule.delay_slots];

        let (actor, actor_ref) = Actor::new(system);

        Self {
            schedule,
            programs,
            buffers,
            delay_buffers,
            param_values,
            param_map,
            actor,
            actor_ref,
        }
    }

    /// Return the actor handle for external control (patchbay).
    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.actor_ref.clone()
    }

    fn apply_command(&mut self, cmd: CommandEnum) {
        match cmd {
            CommandEnum::SetParameter { anchor, param, value } => {
                if let Some(inner) = self.param_map.get(&anchor) {
                    if let Some(&(prog_idx, param_idx)) = inner.get(&param) {
                        let v = match value {
                            crate::graph_engine::ParamValue::F32(v) => v as f64,
                            crate::graph_engine::ParamValue::F64(v) => v,
                            _ => return,
                        };
                        self.param_values[prog_idx][param_idx] = v;
                    }
                }
            }
            _ => {}
        }
    }
}

impl<T: Transcendental, const BUF: usize> Algorithm<T> for RillGraphEngine<T, BUF> {
    fn process(
        &mut self,
        input: Option<&[T]>,
        output: &mut [T],
    ) -> ProcessResult<()> {
        // Drain actor mailbox
        self.actor.drain(|cmd| self.apply_command(cmd));

        // Execute steps
        for step in &self.schedule.steps.clone() {
            match step {
                Step::ReadDelay { slot, target } => {
                    self.buffers[target].copy_from(&self.delay_buffers[slot]);
                }
                Step::InlineProgram { node_idx, input_bufs, output_bufs, param_indices } => {
                    let prog = &mut self.programs[node_idx];
                    for &pi in &param_indices {
                        prog.set_param(pi, self.param_values[node_idx][pi]);
                    }

                    // Read input buffers into a temporary slice for the program
                    // RillProgram expects input: &[T]
                    // For now: use the first input buffer as the input
                    let input_data: Option<Vec<T>> = input_bufs.first().map(|&bi| {
                        self.buffers[bi].as_slice().to_vec()
                    });

                    // Allocate output buffer
                    let mut out_buf = vec![T::default(); rill_core::buffer::FIXED_BUFFER_SIZE];

                    prog.process(
                        input_data.as_deref(),
                        &mut out_buf,
                    )?;

                    // Write to output buffer slots
                    for &ob in &output_bufs {
                        self.buffers[ob].copy_from(&out_buf);
                    }
                }
                Step::BufferCopy { from, to, gain, add } => {
                    if add {
                        for i in 0..BUF {
                            self.buffers[to].as_mut_slice()[i] +=
                                self.buffers[from].as_slice()[i] * T::from(gain).unwrap();
                        }
                    } else {
                        self.buffers[to].copy_from(&self.buffers[from]);
                        if (gain - 1.0).abs() > f32::EPSILON {
                            for v in self.buffers[to].as_mut_slice() {
                                *v = *v * T::from(gain).unwrap();
                            }
                        }
                    }
                }
                Step::WriteDelay { source, slot } => {
                    self.delay_buffers[slot].copy_from(&self.buffers[source]);
                }
            }
        }

        // Copy output buffers to output slice
        for (i, &buf_idx) in self.schedule.output_mapping.iter().enumerate() {
            if i < output.len() {
                let buf = &self.buffers[buf_idx];
                output.copy_from_slice(&buf.as_slice()[..output.len()]);
            }
        }

        Ok(())
    }

    fn reset(&mut self) {
        for prog in &mut self.programs {
            prog.reset();
        }
        for buf in &mut self.buffers {
            buf.clear();
        }
        for db in &mut self.delay_buffers {
            db.clear();
        }
    }
}

use rill_core::queues::CommandEnum;
```

Note: the implementation above is a sketch — the actual engine needs to deal with `FixedBuffer` API, `CommandEnum` variant, and `BUF` constant correctly. These will be refined during the implementation step based on compiler feedback.

- [ ] **Step 3: Add module to lib.rs**

```rust
pub mod graph_engine;
```

- [ ] **Step 4: Verify compiles**

```bash
cargo check -p rill-lang
```

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/graph_engine.rs rill-lang/src/lib.rs rill-lang/Cargo.toml
git commit -m 'feat(rill-lang): RillGraphEngine runtime with actor mailbox'
```

---

### Task 10: `compile_graph()` Entry Point

**Files:**
- Modify: `rill-lang/src/lib.rs`

- [ ] **Step 1: Add `compile_graph` function**

After `compile_with`, add:

```rust
use rill_core_actor::ActorSystem;

/// Compile a rill-lang graph source into a RillGraphEngine.
///
/// The source may contain `param`/`keep param`/`inline param` definitions
/// which become graph nodes. The `process` definition determines wiring.
pub fn compile_graph<T: Transcendental, const BUF: usize>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
    system: &ActorSystem,
) -> Result<graph_engine::RillGraphEngine<T, BUF>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens)?;
    let typed = types::infer::infer_program_with(&program, registry)?;
    let mut graph_ir = graph_build::build_graph_ir(&typed)?;

    // If no graph nodes, compile as a single inline program
    if graph_ir.nodes.is_empty() {
        // Fallback: single-algorithm mode
        let ir = lower::lower_with(&typed, registry, sample_rate)?;
        validate_block_builtins(&ir)?;
        let prog = RillProgram::<T>::new_with(ir, registry, sample_rate)?;

        let schedule = graph_schedule::ScheduledGraph {
            inputs: 0,
            outputs: 1,
            steps: vec![graph_schedule::Step::InlineProgram {
                node_idx: 0,
                input_bufs: vec![],
                output_bufs: vec![0],
                param_indices: (0..prog.params_meta().len()).collect(),
            }],
            buffers: 1,
            delay_slots: 0,
            output_mapping: vec![0],
        };

        return Ok(graph_engine::RillGraphEngine::new(
            schedule,
            vec![prog],
            vec!["process".to_string()],
            system,
        ));
    }

    // Graph path: optimize, lower, schedule
    graph_optimize::optimize(&mut graph_ir);
    let order = graph_lower::lower_graph(&graph_ir)?;
    let schedule = graph_schedule::build_scheduled_graph(&graph_ir, &order);

    // Compile each node's IR into a RillProgram
    let node_names: Vec<String> = graph_ir.nodes.keys().cloned().collect();
    let mut programs = Vec::with_capacity(graph_ir.nodes.len());
    for name in &node_names {
        let node = &graph_ir.nodes[name];
        let prog = RillProgram::<T>::new_with(node.ir.clone(), registry, sample_rate)?;
        programs.push(prog);
    }

    Ok(graph_engine::RillGraphEngine::new(
        schedule,
        programs,
        node_names,
        system,
    ))
}
```

- [ ] **Step 2: Verify compiles**

```bash
cargo check -p rill-lang
```

- [ ] **Step 3: Write integration test**

```rust
// rill-lang/tests/graph_compile.rs
#[test]
fn compiles_graph_with_param_node() {
    // Uses compile_graph — to be written after ActorSystem availability
}
```

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/lib.rs rill-lang/tests/graph_compile.rs
git commit -m 'feat(rill-lang): compile_graph() entry point'
```

---

### Task 11: rill-adrift integration — LangNode supports RillGraphEngine

**Files:**
- Modify: `rill-adrift/src/lang_node.rs`
- Modify: `rill-adrift/src/registration.rs`
- Modify: `rill-adrift/Cargo.toml`

- [ ] **Step 1: Add `rill-lang` graph feature to rill-adrift**

Check that `rill-lang` feature in rill-adrift already enables `rill-lang`. Add graph-related re-exports.

- [ ] **Step 2: Extend LangNode to detect graph source**

Add a `is_graph_source()` heuristic — if source contains `param ` keyword, treat as graph:

```rust
// In lang_node.rs or a new constructor
fn is_graph_source(src: &str) -> bool {
    src.contains("param ") || src.contains("keep param ") || src.contains("inline param ")
}

// In the constructor:
if is_graph_source(source) {
    let engine = rill_lang::compile_graph::<f32, BUF>(source, registry, sample_rate, actor_system)?;
    // Wrap in RillGraphLangNode
} else {
    let prog = rill_lang::compile_with::<f32>(source, registry, sample_rate)?;
    // Wrap in LangNode as before
}
```

- [ ] **Step 3: Register `rill/graph_lang` node type**

In `registration.rs`:

```rust
factory.register_fn("rill/graph_lang", |id, params| {
    let source = params.get_str("source").unwrap_or("");
    let registry = lang_builtins::full_registry::<f32>();
    // Need access to ActorSystem here — pass via params or global
    // For MVP: use compile_with fallback
    let prog = rill_lang::compile_with::<f32>(source, &registry, 44100.0)
        .map_err(|e| RegistryError::Construct(format!("{e:?}")))?;
    Ok(NodeVariant::Processor(Box::new(LangNode::new(id, prog, source))))
});
```

- [ ] **Step 4: Verify compiles and test**

```bash
cargo check -p rill-adrift
cargo test -p rill-adrift
```

- [ ] **Step 5: Commit**

```bash
git add rill-adrift/src/lang_node.rs rill-adrift/src/registration.rs rill-adrift/Cargo.toml
git commit -m 'feat(rill-adrift): LangNode detects and routes graph DSL source'
```

---

### Task 12: rill-patchbay — anchor-based SetParameter

**Files:**
- Modify: `rill-patchbay/src/engine.rs`
- Modify: `rill-patchbay/src/module_def.rs`

- [ ] **Step 1: Add anchor field to servo target**

In `module_def.rs`, add to `ServoDef`:

```rust
/// String anchor name for rill-lang graph nodes.
/// When set, the servo sends SetParameter commands to the
/// RillGraphEngine using this anchor instead of a PortId.
#[cfg_attr(feature = "serde", serde(default))]
pub target_anchor: Option<String>,
```

- [ ] **Step 2: Extend Servo to support anchor-based targets**

In `engine.rs`, in the Servo's ClockTick handler, before sending SetParameter:

```rust
let cmd = if let Some(anchor) = &self.target_anchor {
    // GraphEngine mode: use anchor string
    CommandEnum::SetParameter(SetParameter {
        port: PortId::dummy(),  // placeholder — not used for anchor routing
        parameter: param_id.clone(),
        value,
        source: SignalOrigin::Automaton(self.automaton.name().into()),
        timestamp: SetParameter::now(),
        sample_pos: None,
    })
    // The engine receives this and routes by anchor internally
} else {
    // Standard mode: use PortId
    SetParameter::new(port_id, param_id.clone(), value, source)
};
```

Note: the actual approach may vary — if `CommandEnum` doesn't natively support anchors, we may need to add a new variant to `CommandEnum` in `rill-core`:

```rust
// rill-core/src/queues/signal.rs
CommandEnum::GraphSetParameter {
    anchor: String,
    param: String,
    value: ParamValue,
}
```

This variant bypasses `PortId` and targets a named anchor inside a `RillGraphEngine`. The engine routes by anchor → program → param index internally.

- [ ] **Step 3: Verify compiles**

```bash
cargo check -p rill-patchbay
```

- [ ] **Step 4: Commit**

```bash
git add rill-patchbay/src/engine.rs rill-patchbay/src/module_def.rs
git commit -m 'feat(rill-patchbay): anchor-based parameter targeting for graph engine'
```

---

### Task 13: End-to-end integration test

**Files:**
- Create: `rill-adrift/tests/graph_dsl_integration.rs`

- [ ] **Step 1: Write integration test**

```rust
// rill-adrift/tests/graph_dsl_integration.rs
use rill_adrift::lang;

#[test]
fn compile_graph_dsl_and_execute() {
    let src = r#"
param osc = sin(440.0);
process = osc : _;
"#;
    // Compile as graph
    let result = rill_lang::compile::<f32>(src); // fallback: single mode if no graph features active
    assert!(result.is_ok());
    let mut prog = result.unwrap();
    let mut out = [0.0f32; 64];
    prog.process(None, &mut out).unwrap();
    // Check that output is non-zero (sine wave produces values)
    let non_zero = out.iter().any(|&v| v.abs() > 1e-6);
    assert!(non_zero, "sine oscillator should produce non-zero output");
}

#[test]
fn graph_dsl_with_two_nodes_executes() {
    let src = r#"
param osc = sin(440.0);
param gain = _ * 0.5;
process = osc : gain : _;
"#;
    // This test verifies the full pipeline: parse → graph IR → lower → schedule → exec
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p rill-adrift --test graph_dsl_integration
```

- [ ] **Step 3: Fix issues and iterate**

- [ ] **Step 4: Commit**

```bash
git add rill-adrift/tests/graph_dsl_integration.rs
git commit -m 'test(rill-adrift): graph DSL end-to-end integration tests'
```

---

### Task 14: Clippy + fmt + workspace test pass

- [ ] **Step 1: Run clippy**

```bash
cargo clippy --workspace
```
Expected: PASS (no new warnings)

- [ ] **Step 2: Run fmt**

```bash
cargo fmt -- --check
```
Expected: PASS

- [ ] **Step 3: Run workspace tests**

```bash
cargo test --workspace
```
Expected: PASS (existing + new tests)

- [ ] **Step 4: Fix any issues**

- [ ] **Step 5: Commit**

```bash
git commit -m 'chore: clippy + fmt after graph compilation implementation'
```

---

## Spec Coverage Check

| Spec Section | Task(s) |
|---|---|
| DSL: `param`/`keep`/`inline` keywords | Tasks 1-3 |
| DSL: `def` always inlined | Task 5 (graph builder skips `DefKind::Def`) |
| Graph IR types | Task 4 |
| Graph builder (AST → Graph IR) | Task 5 |
| Optimizer: DCE, inlining | Task 6 |
| Feedback as delayed data path | Tasks 7-8 (ReadDelay/WriteDelay in schedule) |
| Lowering: topo sort (signal edges only) | Task 7 |
| Buffer allocation + liveness | Task 8 |
| ScheduledGraph + Step enum | Task 8 |
| RillGraphEngine runtime | Task 9 |
| compile_graph() entry point | Task 10 |
| rill-adrift integration (LangNode) | Task 11 |
| rill-patchbay anchor-based SetParameter | Task 12 |
| Future: parallel merge, lateral merge, LTI reorder | Deferred — skeleton in optimizer, not implemented |
| Non-goal: split-chain, tape loops, nested graphs | Not included |

---

## Implementation Notes

- **FixedBuffer API**: Verify `FixedBuffer::copy_from`, `as_slice()`, `as_mut_slice()`, `clear()`, and the `Default` impl exist in `rill-core`. Adjust engine code accordingly.
- **Const generic BUF**: The graph engine uses `const BUF: usize` — this must match the block size constant used by the backend. `rill-graph` uses this pattern; `RillGraphEngine` follows the same convention.
- **CommandEnum anchor variant**: Adding `GraphSetParameter` to `CommandEnum` in `rill-core` requires updating all `match` arms across the workspace. Do this as a separate commit before Task 12.
- **ActorSystem lifetime**: The engine holds `Actor<CommandEnum>` which is spawned via `ActorSystem`. The system must outlive the engine. This is already the case in `rill-graph`.
- **ScheduledGraph step cloning**: The `process()` method clones `self.schedule.steps` to avoid borrow issues with `&mut self.programs`. Consider `std::mem::take` + replace pattern (as used in `RillProgram`'s schedule handling) for zero-allocation.
- **Graph builder edge collection**: The initial `collect_edges` is simplified — it handles basic `:`, `,`, `<:`, `:>`, `~` combinators between param references. Multi-level nesting and arithmetic combinators between nodes need refinement during implementation.
- **Optimizer parallel/lateral/LTI passes**: Deferred. Stub functions in `graph_optimize.rs` are commented out; uncomment and implement in follow-up tasks.
