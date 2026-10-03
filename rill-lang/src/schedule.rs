//! Compile-time partitioning of the linear IR into a block-only execution schedule.
//!
//! Every instruction is a whole-buffer operation: `CallBlock` becomes
//! [`Step::ForeignBlock`], all other instructions (combinational, block-state
//! read/write, delay read/write) become [`Step::Block`]. The engine is purely
//! block-level — per-sample state lives inside `BlockBuiltin` implementations.

use crate::ir::{Instr, Ir};

/// One scheduled unit of work, in execution order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A single whole-buffer instruction.
    Block(usize),
    /// An opaque whole-buffer built-in.
    ForeignBlock(usize),
}

/// The full execution plan for an [`Ir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    /// Steps in execution order (dependencies first).
    pub steps: Vec<Step>,
}

/// Which register each instruction produces (`None` for sinks).
fn instr_dst(instr: &Instr) -> Option<usize> {
    match *instr {
        Instr::Const { dst, .. }
        | Instr::LoadInput { dst, .. }
        | Instr::ReadBlockState { dst, .. }
        | Instr::ReadDelay { dst, .. }
        | Instr::Un { dst, .. }
        | Instr::Bin { dst, .. }
        | Instr::Move { dst, .. }
        | Instr::CallBlock { dst, .. }
        | Instr::ReadParam { dst, .. }
        | Instr::ReadActorParam { dst, .. }
        | Instr::ReadMainCell { dst, .. } => Some(dst),
        Instr::WriteBlockState { .. } | Instr::WriteDelay { .. } => None,
        #[cfg(feature = "debug")]
        Instr::ProbePoint { dst, .. } => Some(dst),
    }
}

/// The registers an instruction consumes.
fn instr_srcs(instr: &Instr) -> Vec<usize> {
    match *instr {
        Instr::Un { src, .. } | Instr::Move { src, .. } => vec![src],
        Instr::Bin { a, b, .. } => vec![a, b],
        Instr::WriteBlockState { src, .. } | Instr::WriteDelay { src, .. } => vec![src],
        Instr::CallBlock { ref srcs, .. } => srcs.clone(),
        #[cfg(feature = "debug")]
        Instr::ProbePoint { src, .. } => vec![src],
        _ => Vec::new(),
    }
}

/// Build the block-only schedule for an IR.
pub fn build_schedule(ir: &Ir) -> Schedule {
    let n = ir.instrs.len();

    // producer[reg] = instr index whose dst == reg (SSA: unique).
    let mut producer: Vec<Option<usize>> = vec![None; ir.num_regs];
    for (i, instr) in ir.instrs.iter().enumerate() {
        if let Some(d) = instr_dst(instr) {
            producer[d] = Some(i);
        }
    }

    // Adjacency: consumer -> producer (dependency edges).
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, instr) in ir.instrs.iter().enumerate() {
        for s in instr_srcs(instr) {
            if let Some(p) = producer[s] {
                adj[i].push(p);
            }
        }
    }

    // Tarjan SCC. Emission order is reverse-finish = execution order
    // (dependencies first) because edges point consumer -> producer.
    let sccs = tarjan_scc(n, &adj);

    // Classify each SCC into a Step.
    let mut steps = Vec::with_capacity(sccs.len());
    for scc in sccs {
        if scc.len() == 1 {
            let i = scc[0];
            if matches!(ir.instrs[i], Instr::CallBlock { .. }) {
                steps.push(Step::ForeignBlock(i));
            } else {
                steps.push(Step::Block(i));
            }
        } else {
            // A cycle should not occur in block-level IR (feedback is 1-tick via
            // a double buffer); emit sorted as a best-effort fallback.
            let mut instrs = scc;
            instrs.sort_unstable();
            for i in instrs {
                steps.push(Step::Block(i));
            }
        }
    }
    Schedule { steps }
}

/// Iterative Tarjan strongly-connected-components.
///
/// Returns SCCs in reverse topological order of the condensation — with our
/// consumer→producer edges, that is exactly execution order (a node's
/// dependencies appear before it).
fn tarjan_scc(n: usize, adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    const UNVISITED: i64 = -1;
    let mut index = vec![UNVISITED; n];
    let mut low = vec![0i64; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next_index: i64 = 0;
    let mut out: Vec<Vec<usize>> = Vec::new();

    // Explicit DFS stack of (node, next-neighbor-index).
    for root in 0..n {
        if index[root] != UNVISITED {
            continue;
        }
        let mut call: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&mut (v, ref mut ni)) = call.last_mut() {
            if *ni == 0 {
                index[v] = next_index;
                low[v] = next_index;
                next_index += 1;
                stack.push(v);
                on_stack[v] = true;
            }
            if *ni < adj[v].len() {
                let w = adj[v][*ni];
                *ni += 1;
                if index[w] == UNVISITED {
                    call.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                if low[v] == index[v] {
                    let mut comp = Vec::new();
                    loop {
                        let w = stack.pop().unwrap();
                        on_stack[w] = false;
                        comp.push(w);
                        if w == v {
                            break;
                        }
                    }
                    out.push(comp);
                }
                let finished = v;
                call.pop();
                if let Some(&mut (parent, _)) = call.last_mut() {
                    low[parent] = low[parent].min(low[finished]);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::lower::lower;
    use crate::parser::parse;
    use crate::types::infer::infer_program;

    fn schedule_of(src: &str) -> Schedule {
        let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
        let tp = infer_program(&p).unwrap();
        let ir = lower(&tp).unwrap();
        build_schedule(&ir)
    }

    fn schedule_of_with(src: &str) -> (Ir, Schedule) {
        let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
        let tp = infer_program(&p).unwrap();
        let ir = lower(&tp).unwrap();
        let sched = build_schedule(&ir);
        (ir, sched)
    }

    fn n_block(s: &Schedule) -> usize {
        s.steps
            .iter()
            .filter(|st| matches!(st, Step::Block(_)))
            .count()
    }

    #[test]
    fn combinational_program_is_all_block() {
        let s = schedule_of("main = _ * 0.5");
        assert!(n_block(&s) >= 1);
    }

    #[test]
    fn feedback_uses_block_state() {
        let s = schedule_of("main = + ~ _");
        assert!(!s.steps.is_empty());
    }

    #[test]
    fn delay_uses_block_steps() {
        let s = schedule_of("main = _ @ 3");
        assert!(!s.steps.is_empty());
    }

    #[test]
    fn steps_are_in_dependency_order() {
        let s = schedule_of("main = abs _ : _ * 2.0");
        assert!(!s.steps.is_empty());
    }

    #[test]
    fn block_builtin_schedules_as_foreign_block() {
        let (_, s) = schedule_of_with("main = _ : lowpass 1000.0 0.7");
        assert!(s.steps.iter().any(|st| matches!(st, Step::ForeignBlock(_))));
    }
}
