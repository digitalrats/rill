//! Linear-scan register allocator for the block IR.
//!
//! Reuses dead registers to reduce the number of `block_regs` allocations.
//! Fewer registers = fewer `mem::take`/re-assign cycles and less memory.

use std::collections::HashMap;

use crate::ir::{Instr, Ir};

fn instr_dst(instr: &Instr) -> Option<usize> {
    match instr {
        Instr::Const { dst, .. }
        | Instr::LoadInput { dst, .. }
        | Instr::Move { dst, .. }
        | Instr::Un { dst, .. }
        | Instr::Bin { dst, .. }
        | Instr::ReadState { dst, .. }
        | Instr::ReadDelay { dst, .. }
        | Instr::CallSample { dst, .. }
        | Instr::CallBlock { dst, .. }
        | Instr::ReadParam { dst, .. } => Some(*dst),
        Instr::WriteState { .. } | Instr::WriteDelay { .. } => None,
    }
}

fn instr_srcs(instr: &Instr) -> Vec<usize> {
    match instr {
        Instr::Const { .. }
        | Instr::LoadInput { .. }
        | Instr::ReadState { .. }
        | Instr::ReadParam { .. }
        | Instr::ReadDelay { .. } => vec![],
        Instr::Move { src, .. } | Instr::Un { src, .. } => vec![*src],
        Instr::Bin { a, b, .. } => vec![*a, *b],
        Instr::WriteState { src, .. } | Instr::WriteDelay { src, .. } => vec![*src],
        Instr::CallSample { srcs, .. } => srcs.clone(),
        Instr::CallBlock { srcs, .. } => srcs.clone(),
    }
}

fn set_dst(instr: &mut Instr, phys: usize) {
    match instr {
        Instr::Const { dst, .. }
        | Instr::LoadInput { dst, .. }
        | Instr::Move { dst, .. }
        | Instr::Un { dst, .. }
        | Instr::Bin { dst, .. }
        | Instr::ReadState { dst, .. }
        | Instr::ReadDelay { dst, .. }
        | Instr::CallSample { dst, .. }
        | Instr::CallBlock { dst, .. }
        | Instr::ReadParam { dst, .. } => *dst = phys,
        _ => {}
    }
}

fn remap_srcs(instr: &mut Instr, remap: &HashMap<usize, usize>) {
    let m = |r: &mut usize| *r = remap.get(r).copied().unwrap_or(*r);
    match instr {
        Instr::Move { src, .. } | Instr::Un { src, .. } => m(src),
        Instr::Bin { a, b, .. } => {
            m(a);
            m(b);
        }
        Instr::WriteState { src, .. } | Instr::WriteDelay { src, .. } => m(src),
        Instr::CallSample { srcs, .. } => {
            for s in srcs {
                m(s);
            }
        }
        Instr::CallBlock { srcs, .. } => {
            for s in srcs {
                m(s);
            }
        }
        _ => {}
    }
}

/// Allocate physical registers by reusing dead virtual registers.
/// Reduces `num_regs` — the interpreter allocates one `Vec<T>` per register.
pub fn allocate(ir: &mut Ir) {
    let n = ir.instrs.len();

    // Step 1: compute last-use index for each virtual register
    let mut last_use: HashMap<usize, usize> = HashMap::new();
    for (i, instr) in ir.instrs.iter().enumerate() {
        for src in instr_srcs(instr) {
            last_use.insert(src, i);
        }
    }

    // Step 2: forward pass — allocate physical registers
    let mut vir2phys: HashMap<usize, usize> = HashMap::new();
    let mut phys2vir: HashMap<usize, usize> = HashMap::new();
    let mut free: Vec<usize> = Vec::new();
    let mut next_phys: usize = 0;
    let mut new_instrs: Vec<Instr> = Vec::with_capacity(n);

    for i in 0..n {
        let mut instr = ir.instrs[i].clone();

        // Sources that die at this instruction can have their physical registers freed
        for &src in &instr_srcs(&instr) {
            if last_use.get(&src).copied() == Some(i) {
                if let Some(&phys) = vir2phys.get(&src) {
                    free.push(phys);
                    phys2vir.remove(&phys);
                }
            }
        }

        // Remap sources
        remap_srcs(&mut instr, &vir2phys);

        // Allocate destination
        if let Some(vr) = instr_dst(&instr) {
            let phys = if let Some(reuse) = free.pop() {
                reuse
            } else {
                let p = next_phys;
                next_phys += 1;
                p
            };
            vir2phys.insert(vr, phys);
            phys2vir.insert(phys, vr);
            set_dst(&mut instr, phys);
        }
        new_instrs.push(instr);
    }

    ir.instrs = new_instrs;
    ir.output_reg = vir2phys
        .get(&ir.output_reg)
        .copied()
        .unwrap_or(ir.output_reg);
    ir.num_regs = next_phys;
}
