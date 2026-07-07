//! Build GraphIr from a typed AST.

use crate::ast::{BinOp, DefKind, Expr};
use crate::error::CompileError;
use crate::graph_ir::{EdgeKind, GraphEdge, GraphIr, GraphNode};
use crate::ir::StateLayout;
use crate::types::infer::TypedProgram;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

/// Build a `GraphIr` from a typed program.
///
/// If the program has no `param`-annotated definitions, returns a `GraphIr`
/// with zero nodes (caller should fall back to single-algorithm mode).
pub fn build_graph_ir(typed: &TypedProgram) -> Result<GraphIr, CompileError> {
    let program = &typed.program;

    let def_map: HashMap<&str, &crate::ast::Def> =
        program.defs.iter().map(|d| (d.name.as_str(), d)).collect();

    let param_names: HashSet<&str> = program
        .defs
        .iter()
        .filter(|d| d.kind != DefKind::Def)
        .map(|d| d.name.as_str())
        .collect();

    let process_def = program.defs.iter().find(|d| d.name == "process");
    if process_def.is_none() {
        return Ok(GraphIr {
            inputs: 0,
            outputs: 1,
            nodes: IndexMap::new(),
            edges: vec![],
        });
    }

    let mut nodes: IndexMap<String, GraphNode> = IndexMap::new();
    for def in &program.defs {
        if def.name == "process" || def.kind == DefKind::Def {
            continue;
        }
        let arity = (def.params.len().max(1), 1);
        nodes.insert(
            def.name.clone(),
            GraphNode {
                arity,
                ir: crate::ir::Ir {
                    instrs: vec![],
                    num_regs: 0,
                    output_reg: 0,
                    num_inputs: arity.0,
                    state: StateLayout::default(),
                    builtins: vec![],
                    params: vec![],
                },
                params: vec![],
                keep: def.kind == DefKind::KeepParam,
                force_inline: def.kind == DefKind::InlineParam,
            },
        );
    }

    if nodes.is_empty() {
        return Ok(GraphIr {
            inputs: 0,
            outputs: 1,
            nodes,
            edges: vec![],
        });
    }

    let mut edges = Vec::new();
    let exprs_as_map: HashMap<&str, &Expr> = def_map.iter().map(|(k, d)| (*k, &d.body)).collect();
    collect_edges(
        &process_def.unwrap().body,
        &param_names,
        &exprs_as_map,
        &def_map,
        &mut edges,
    );

    Ok(GraphIr {
        inputs: 0,
        outputs: 1,
        nodes,
        edges,
    })
}

fn collect_edges(
    expr: &Expr,
    param_names: &HashSet<&str>,
    exprs: &HashMap<&str, &Expr>,
    def_map: &HashMap<&str, &crate::ast::Def>,
    edges: &mut Vec<GraphEdge>,
) {
    match expr {
        Expr::Bin { op, lhs, rhs, .. } => {
            collect_edges(lhs, param_names, exprs, def_map, edges);
            collect_edges(rhs, param_names, exprs, def_map, edges);

            let edge_kind = match op {
                BinOp::Feedback => EdgeKind::Feedback,
                _ => EdgeKind::Signal,
            };

            let lhs_nodes = node_refs(lhs, param_names, exprs);
            let rhs_nodes = node_refs(rhs, param_names, exprs);

            match op {
                BinOp::Seq => {
                    for (ln, _) in &lhs_nodes {
                        for (rn, _) in &rhs_nodes {
                            edges.push(GraphEdge {
                                from_node: ln.clone(),
                                from_port: 0,
                                to_node: rn.clone(),
                                to_port: 0,
                                kind: edge_kind,
                            });
                        }
                    }
                }
                BinOp::Par => {}
                BinOp::Split => {
                    for (ln, _) in &lhs_nodes {
                        for (rn, _) in &rhs_nodes {
                            for p in 0..2 {
                                edges.push(GraphEdge {
                                    from_node: ln.clone(),
                                    from_port: p,
                                    to_node: rn.clone(),
                                    to_port: p,
                                    kind: edge_kind,
                                });
                            }
                        }
                    }
                }
                BinOp::Merge => {
                    for (ln, _) in &lhs_nodes {
                        for (rn, _) in &rhs_nodes {
                            for p in 0..2 {
                                edges.push(GraphEdge {
                                    from_node: rn.clone(),
                                    from_port: p,
                                    to_node: ln.clone(),
                                    to_port: p,
                                    kind: edge_kind,
                                });
                            }
                        }
                    }
                }
                BinOp::Feedback => {
                    if lhs_nodes.is_empty() && rhs_nodes.is_empty() {
                    } else if rhs_nodes.is_empty() {
                        for (ln, _) in &lhs_nodes {
                            edges.push(GraphEdge {
                                from_node: ln.clone(),
                                from_port: 0,
                                to_node: ln.clone(),
                                to_port: 0,
                                kind: EdgeKind::Feedback,
                            });
                        }
                    } else if lhs_nodes.is_empty() {
                        for (rn, _) in &rhs_nodes {
                            edges.push(GraphEdge {
                                from_node: rn.clone(),
                                from_port: 0,
                                to_node: rn.clone(),
                                to_port: 0,
                                kind: EdgeKind::Signal,
                            });
                        }
                    } else {
                        for (rn, _) in &rhs_nodes {
                            for (ln, _) in &lhs_nodes {
                                edges.push(GraphEdge {
                                    from_node: rn.clone(),
                                    from_port: 0,
                                    to_node: ln.clone(),
                                    to_port: 0,
                                    kind: EdgeKind::Signal,
                                });
                            }
                        }
                        for (ln, _) in &lhs_nodes {
                            for (rn, _) in &rhs_nodes {
                                edges.push(GraphEdge {
                                    from_node: ln.clone(),
                                    from_port: 0,
                                    to_node: rn.clone(),
                                    to_port: 0,
                                    kind: EdgeKind::Feedback,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Expr::Ref(name, _) if !param_names.contains(name.as_str()) => {
            if let Some(def) = def_map.get(name.as_str()) {
                if def.kind == DefKind::Def {
                    collect_edges(&def.body, param_names, exprs, def_map, edges);
                }
            }
        }
        Expr::Apply { args, .. } => {
            for arg in args {
                collect_edges(arg, param_names, exprs, def_map, edges);
            }
        }
        _ => {}
    }
}

fn node_refs<'a>(
    expr: &'a Expr,
    param_names: &HashSet<&str>,
    exprs: &'a HashMap<&str, &Expr>,
) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    node_refs_inner(expr, param_names, exprs, &mut out);
    out
}

fn node_refs_inner<'a>(
    expr: &'a Expr,
    param_names: &HashSet<&str>,
    exprs: &'a HashMap<&str, &Expr>,
    out: &mut Vec<(String, usize)>,
) {
    match expr {
        Expr::Ref(name, _) => {
            if param_names.contains(name.as_str()) {
                out.push((name.clone(), 1));
            } else if let Some(sub_expr) = exprs.get(name.as_str()) {
                node_refs_inner(sub_expr, param_names, exprs, out);
            }
        }
        Expr::Bin { lhs, rhs, .. } => {
            node_refs_inner(lhs, param_names, exprs, out);
            node_refs_inner(rhs, param_names, exprs, out);
        }
        Expr::Apply { args, .. } => {
            for arg in args {
                node_refs_inner(arg, param_names, exprs, out);
            }
        }
        _ => {}
    }
}
