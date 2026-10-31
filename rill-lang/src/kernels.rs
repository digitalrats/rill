//! The ONE `(class, method) → kernel` table.
//!
//! A primitive instance (`Num Int`, `Ord Double`, the structural `Eq`/`Ord`, a
//! buffer arithmetic instance) has no source equations: its methods are compiler
//! kernels. Two consumers need that decision and **must** agree:
//!
//! * lowering, when it inlines a primitive method call
//!   ([`lower_primitive_method`](crate::lower)), and
//! * dictionary construction, when it fills
//!   [`DictSlot::Kernel`](crate::arena::DictSlot).
//!
//! Keeping two copies of the mapping is exactly the defect class the dictionary
//! work removes, so [`kernel_for_method`] is the only place a
//! `(class, method)` pair becomes a [`KernelId`]. The kernel → *instruction*
//! mapping is a different (and unavoidable) relation: one instruction serves a
//! whole kernel, and it lives where the instruction is emitted.
//!
//! Design §3.9's six missing kernels (`Compare`, `Negate`, scalar `Abs`,
//! `Signum`, `FromInteger`, `Recip`) now all have IR instructions. The
//! report machinery is kept because a *future* kernel may again be partial —
//! [`has_instruction`] and [`missing_kernels`] surface such a gap instead of
//! silently dropping the slot.

use crate::arena::KernelId;
use crate::ir::Ir;

/// The kernel that implements `class`'s `method`, if any.
///
/// This is the single `(class, method) → KernelId` decision in the compiler.
/// Every method declared by a *primitive* instance must resolve here (or, when
/// the class declares a default body, fall back to that body — `Ord`'s
/// `min`/`max` are ordinary definitions, not kernels).
///
/// It is deliberately keyed on `(class, method)`, not on `(class, type,
/// method)`: the kernel is an *operation*. `(+)` selects
/// [`KernelId::Add`] for `Int`, for `Double` and for a buffer alike; the kernel
/// implementation dispatches on its operand kinds. That is why adding a class
/// does not grow [`KernelId`].
pub fn kernel_for_method(class: &str, method: &str) -> Option<KernelId> {
    Some(match (class, method) {
        // `Eq`: structural equality and inequality.
        ("Eq", "==") => KernelId::Eq,
        ("Eq", "/=") => KernelId::Ne,
        // `Ord`: the comparison predicates plus the three-way order.
        ("Ord", "compare") => KernelId::Compare,
        ("Ord", "<") => KernelId::Lt,
        ("Ord", ">") => KernelId::Gt,
        ("Ord", "<=") => KernelId::Le,
        ("Ord", ">=") => KernelId::Ge,
        // `Num`: ring arithmetic and the unary numeric operations.
        ("Num", "+") => KernelId::Add,
        ("Num", "-") => KernelId::Sub,
        ("Num", "*") => KernelId::Mul,
        ("Num", "negate") => KernelId::Negate,
        ("Num", "abs") => KernelId::Abs,
        ("Num", "signum") => KernelId::Signum,
        ("Num", "fromInteger") => KernelId::FromInteger,
        // `Enum` (Report §6.3.4): the two minimal methods. The five
        // `enumFrom*` defaults are written in the PRELUDE in terms of these,
        // exactly as §9 gives them, so they need no kernel.
        ("Enum", "toEnum") => KernelId::ToEnum,
        ("Enum", "fromEnum") => KernelId::FromEnum,
        // `Fractional`: division and reciprocal.
        ("Fractional", "/") => KernelId::Div,
        ("Fractional", "recip") => KernelId::Recip,
        ("Fractional", "fromRational") => KernelId::FromRational,
        _ => return None,
    })
}

/// Whether `kernel` can be executed for an instance of a scalar or a buffer
/// type.
///
/// Every kernel is implemented for both operand kinds, and every one of them
/// has an IR instruction — design §3.9's gap list is **empty**. The `is_buffer`
/// parameter is kept because the report's shape still passes the instance kind
/// through ([`MissingKernel`]), and because a future kernel may again be
/// partial; today no argument changes the answer.
pub fn has_instruction(kernel: KernelId, _is_buffer: bool) -> bool {
    // Exhaustive so a new `KernelId` cannot be added without deciding whether
    // it is executable: adding a variant here fails to compile until it is
    // classified.
    match kernel {
        KernelId::Eq
        | KernelId::Ne
        | KernelId::Lt
        | KernelId::Gt
        | KernelId::Le
        | KernelId::Ge
        | KernelId::Compare
        | KernelId::Add
        | KernelId::Sub
        | KernelId::Mul
        | KernelId::Div
        | KernelId::Negate
        | KernelId::Abs
        | KernelId::Signum
        | KernelId::FromInteger
        | KernelId::FromRational
        | KernelId::Recip
        | KernelId::ToEnum
        | KernelId::FromEnum => true,
    }
}

/// One dictionary slot whose kernel has no IR instruction.
///
/// Empty today (design §3.9's six kernels all execute), and machine-readable on
/// purpose: a future partial kernel appears here, and stage 4 (slot projection)
/// must never return such a slot as if it worked.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MissingKernel {
    /// The class whose dictionary owns the slot.
    pub class: String,
    /// The instance's registry key (see
    /// [`DictDesc::ty_key`](crate::ir::DictDesc::ty_key)).
    pub ty_key: String,
    /// The method's name, at the slot's index in the class's method list.
    pub method: String,
    /// The method's positional slot index.
    pub slot: usize,
    /// The kernel the slot names.
    pub kernel: KernelId,
}

/// Every `(class, type, method)` in `ir`'s dictionary table whose slot names a
/// kernel with no IR instruction, in dictionary order and then slot order.
///
/// Empty for a program that mentions no class, and empty for a program whose
/// dictionaries only name implemented kernels.
pub fn missing_kernels(ir: &Ir) -> Vec<MissingKernel> {
    missing_kernels_in(&ir.dicts)
}

/// [`missing_kernels`] over a descriptor slice (a seated world keeps its own
/// copy and no longer has the `Ir`).
pub fn missing_kernels_in(descs: &[crate::ir::DictDesc]) -> Vec<MissingKernel> {
    let mut out = Vec::new();
    for desc in descs {
        for (slot_index, (slot, method)) in desc
            .methods
            .iter()
            .zip(desc.method_names.iter())
            .enumerate()
        {
            let crate::arena::DictSlot::Kernel(kernel) = slot else {
                continue;
            };
            if has_instruction(*kernel, desc.is_buffer) {
                continue;
            }
            out.push(MissingKernel {
                class: desc.class.clone(),
                ty_key: desc.ty_key.clone(),
                method: method.clone(),
                slot: slot_index,
                kernel: *kernel,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_covers_every_primitive_instance_method() {
        // Every method a primitive instance can be asked for resolves to exactly
        // one kernel: `Eq`'s two, `Ord`'s six on top of `Eq`, `Num`'s seven on
        // top of those, `Fractional`'s two. `Ord`'s `min`/`max` are absent by
        // design — they are ordinary default bodies, not kernels.
        let expected = [
            ("Eq", "==", KernelId::Eq),
            ("Eq", "/=", KernelId::Ne),
            ("Ord", "compare", KernelId::Compare),
            ("Ord", "<", KernelId::Lt),
            ("Ord", ">", KernelId::Gt),
            ("Ord", "<=", KernelId::Le),
            ("Ord", ">=", KernelId::Ge),
            ("Num", "+", KernelId::Add),
            ("Num", "-", KernelId::Sub),
            ("Num", "*", KernelId::Mul),
            ("Num", "negate", KernelId::Negate),
            ("Num", "abs", KernelId::Abs),
            ("Num", "signum", KernelId::Signum),
            ("Num", "fromInteger", KernelId::FromInteger),
            ("Enum", "toEnum", KernelId::ToEnum),
            ("Enum", "fromEnum", KernelId::FromEnum),
            ("Fractional", "/", KernelId::Div),
            ("Fractional", "recip", KernelId::Recip),
        ];
        for (class, method, kernel) in expected {
            assert_eq!(
                kernel_for_method(class, method),
                Some(kernel),
                "`{class}.{method}` must select {kernel:?}"
            );
        }
        // `Ord`'s default-body methods and unrelated names have no kernel: a
        // mapping for them would be a second dispatch path.
        for (class, method) in [
            ("Ord", "min"),
            ("Ord", "max"),
            ("Eq", "eq"),
            ("Num", "negate2"),
        ] {
            assert_eq!(
                kernel_for_method(class, method),
                None,
                "`{class}.{method}` must not select a kernel"
            );
        }
    }

    #[test]
    fn every_kernel_now_has_an_instruction() {
        // Design §3.9's six gaps are CLOSED: `Compare`, `Negate`, scalar `Abs`,
        // `Signum`, `FromInteger` and `Recip` all execute. The list is pinned
        // (rather than asserting `is_empty` on a filtered slice) so a kernel
        // added to `KernelId` without an instruction shows up here by name.
        let gaps: Vec<KernelId> = [
            KernelId::Eq,
            KernelId::Ne,
            KernelId::Lt,
            KernelId::Gt,
            KernelId::Le,
            KernelId::Ge,
            KernelId::Compare,
            KernelId::Add,
            KernelId::Sub,
            KernelId::Mul,
            KernelId::Div,
            KernelId::Negate,
            KernelId::Abs,
            KernelId::Signum,
            KernelId::FromInteger,
            KernelId::Recip,
        ]
        .into_iter()
        .filter(|k| !has_instruction(*k, false) || !has_instruction(*k, true))
        .collect();
        assert_eq!(
            gaps,
            Vec::<KernelId>::new(),
            "design §3.9's list is empty: every kernel has an IR instruction"
        );
        // `abs` is no longer PARTIAL: the same instruction serves a scalar and a
        // buffer because it dispatches on the OPERAND, not on the name.
        assert!(has_instruction(KernelId::Abs, false));
        assert!(has_instruction(KernelId::Abs, true));
        // `negate` now exists for buffers too (`UnOp::Neg` through the same
        // operand dispatch).
        assert!(has_instruction(KernelId::Negate, true));
        assert!(has_instruction(KernelId::Negate, false));
    }
}
