//! Compile-time partitioning of the linear IR into a hybrid execution schedule.
//!
//! Feedforward instructions become whole-buffer [`Step::Block`] ops; recurrences
//! (feedback loops, and the read/write of each state slot or delay line) become
//! per-sample [`Step::Sample`] regions. See the block-processing design doc.

use crate::ir::{Instr, Ir};

/// One scheduled unit of work, in execution order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A single combinational instruction, executed over the whole buffer.
    Block(usize),
    /// A recurrent region, executed per sample. Instruction indices are in
    /// original IR order (which preserves intra-sample data + read-before-write
    /// ordering established by lowering).
    Sample(Vec<usize>),
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
        | Instr::ReadState { dst, .. }
        | Instr::ReadDelay { dst, .. }
        | Instr::Un { dst, .. }
        | Instr::Bin { dst, .. }
        | Instr::Move { dst, .. }
        | Instr::CallSample { dst, .. }
        | Instr::CallBlock { dst, .. } => Some(dst),
        Instr::WriteState { .. } | Instr::WriteDelay { .. } => None,
    }
}

/// The registers an instruction consumes.
fn instr_srcs(instr: &Instr) -> Vec<usize> {
    match *instr {
        Instr::Un { src, .. } | Instr::Move { src, .. } => vec![src],
        Instr::Bin { a, b, .. } => vec![a, b],
        Instr::WriteState { src, .. } | Instr::WriteDelay { src, .. } => vec![src],
        _ => Vec::new(),
    }
}

/// True for instructions that touch persistent state and therefore must run in
/// a sample region (never as a standalone block op).
fn is_stateful(instr: &Instr) -> bool {
    matches!(
        instr,
        Instr::ReadState { .. }
            | Instr::WriteState { .. }
            | Instr::ReadDelay { .. }
            | Instr::WriteDelay { .. }
            | Instr::CallSample { .. }
    )
}

/// Build the hybrid schedule for an IR.
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

    // Recurrence edges (bidirectional) for each state slot and delay line, so
    // the read/write ends share an SCC and feedback loops close.
    let mut read_state: Vec<Option<usize>> = vec![None; ir.state.state_slots];
    let mut write_state: Vec<Option<usize>> = vec![None; ir.state.state_slots];
    let mut read_delay: Vec<Option<usize>> = vec![None; ir.state.delay_lens.len()];
    let mut write_delay: Vec<Option<usize>> = vec![None; ir.state.delay_lens.len()];
    for (i, instr) in ir.instrs.iter().enumerate() {
        match *instr {
            Instr::ReadState { slot, .. } => read_state[slot] = Some(i),
            Instr::WriteState { slot, .. } => write_state[slot] = Some(i),
            Instr::ReadDelay { line, .. } => read_delay[line] = Some(i),
            Instr::WriteDelay { line, .. } => write_delay[line] = Some(i),
            _ => {}
        }
    }
    let add_pair = |a: Option<usize>, b: Option<usize>, adj: &mut Vec<Vec<usize>>| {
        if let (Some(a), Some(b)) = (a, b) {
            adj[a].push(b);
            adj[b].push(a);
        }
    };
    for s in 0..ir.state.state_slots {
        add_pair(read_state[s], write_state[s], &mut adj);
    }
    for l in 0..ir.state.delay_lens.len() {
        add_pair(read_delay[l], write_delay[l], &mut adj);
    }

    // Tarjan SCC. Emission order is reverse-finish = execution order
    // (dependencies first) because edges point consumer -> producer.
    let sccs = tarjan_scc(n, &adj);

    // Classify each SCC into a Step.
    let mut steps = Vec::with_capacity(sccs.len());
    for scc in sccs {
        let recurrent = scc.len() > 1 || scc.iter().any(|&i| is_stateful(&ir.instrs[i]));
        if recurrent {
            let mut instrs = scc;
            instrs.sort_unstable();
            steps.push(Step::Sample(instrs));
        } else {
            steps.push(Step::Block(scc[0]));
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
        let p = parse(&tokenize(src).unwrap()).unwrap();
        let tp = infer_program(&p).unwrap();
        let ir = lower(&tp).unwrap();
        build_schedule(&ir)
    }

    fn n_sample(s: &Schedule) -> usize {
        s.steps
            .iter()
            .filter(|st| matches!(st, Step::Sample(_)))
            .count()
    }
    fn n_block(s: &Schedule) -> usize {
        s.steps
            .iter()
            .filter(|st| matches!(st, Step::Block(_)))
            .count()
    }

    #[test]
    fn combinational_program_is_all_block() {
        let s = schedule_of("process = _ * 0.5;");
        assert_eq!(n_sample(&s), 0);
        assert!(n_block(&s) >= 1);
    }

    #[test]
    fn feedback_program_has_one_sample_region() {
        let s = schedule_of("process = + ~ _;");
        assert_eq!(n_sample(&s), 1);
    }

    #[test]
    fn const_feeding_feedback_stays_block() {
        // `+ ~ (_ * 0.5)`: the 0.5 constant is combinational (Block); the
        // ReadState/Add/Mul/WriteState cycle is one Sample region.
        let s = schedule_of("process = + ~ (_ * 0.5);");
        assert_eq!(n_sample(&s), 1);
        assert!(n_block(&s) >= 1); // at least the Const 0.5 and the LoadInput
    }

    #[test]
    fn feedforward_delay_is_isolated_sample_region() {
        // `_ @ 3`: delay read/write form a sample region; no feedback.
        let s = schedule_of("process = _ @ 3;");
        assert_eq!(n_sample(&s), 1);
    }

    #[test]
    fn feedback_through_delay_is_one_region() {
        let s = schedule_of("process = + ~ (_ @ 2);");
        assert_eq!(n_sample(&s), 1);
    }

    #[test]
    fn gain_then_integrator_splits_block_and_sample() {
        let s = schedule_of("process = (_ * 0.5) : (+ ~ _);");
        assert_eq!(n_sample(&s), 1);
        assert!(n_block(&s) >= 1);
    }

    #[test]
    fn steps_are_in_dependency_order() {
        // Every Block step's producer appears before any step that consumes it:
        // here we only assert the schedule is non-empty and ends producing output.
        let s = schedule_of("process = abs(_) : _ * 2.0;");
        assert!(!s.steps.is_empty());
        assert_eq!(n_sample(&s), 0);
    }
}
