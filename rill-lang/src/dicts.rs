//! Dictionary **planning**: which `(class, type)` pairs a program needs, in
//! which order, and how each method slot is provided.
//!
//! Lowering collects the pairs the program mentions
//! ([`TypedProgram::dict_selections`](crate::types::infer::TypedProgram::dict_selections)),
//! and this module turns them into an ordered plan. Lowering then compiles the
//! one code fragment each [`SlotKind::Body`] slot needs and emits the IR's
//! descriptor list ([`DictDesc`](crate::ir::DictDesc)), which
//! [`WorldState::seat`](crate::program::WorldState::seat) materialises into
//! [`Value::Dictionary`](crate::arena::Value::Dictionary) values.
//!
//! Three rules make the result usable by seat in one forward pass:
//!
//! 1. **Transitive closure.** A dictionary's superclasses are themselves
//!    planned, so a *mentioned* `Num a` also yields `Eq a` and `Ord a` (System
//!    FC: the subclass dictionary holds the superclass ones). Every superclass
//!    describes the SAME type — the class variable is shared across the chain.
//! 2. **Topological order.** A superclass pair is planned before every pair
//!    that references it, so a `supers` index always names an already-built
//!    slot.
//! 3. **No silent truncation.** A superclass cycle is a hard error naming the
//!    cycle. A pair that cannot be expressed yet becomes a
//!    [`DictGap`](crate::ir::DictGap) carrying the reason, and no dictionary is
//!    built for it *or* for a pair that depends on it.
//!
//! `ty_key` always comes from [`instance_key`], the registry's own identity
//! (the full [`value_type_key`](crate::types::ty::value_type_key) when an
//! instance is registered under it, else the head name a partial or derived
//! instance is keyed by). A dictionary and its instance therefore cannot
//! disagree about which type they describe.

use std::collections::HashMap;

use crate::arena::KernelId;
use crate::error::{CompileError, Span};
use crate::ir::DictGap;
use crate::kernels;
use crate::types::ty::{InstanceInfo, TypeEnv, ValueTy};

/// How one method slot of a planned dictionary is provided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotKind {
    /// The instance is primitive and a compiler kernel implements the method.
    /// Chosen by [`kernels::kernel_for_method`] — the ONE `(class, method)`
    /// table, which lowering's primitive-call path also consults.
    Kernel(KernelId),
    /// A source body: the instance's equation, or the class default when the
    /// instance omits the method. Lowering compiles it into one fragment.
    Body,
}

/// One dictionary to build, with every slot already classified.
#[derive(Debug, Clone)]
pub(crate) struct DictPlan {
    /// The class whose vtable this is.
    pub(crate) class: String,
    /// The concrete type the dictionary describes, and how the class variable
    /// must be substituted when the body is typed.
    pub(crate) ty: ValueTy,
    /// The class's kind arity of the class variable (`0` for `Eq a`, `1` for
    /// `Functor f`).
    ///
    /// It selects the substitution the body is typed under, mirroring
    /// [`method_dispatch_key`](crate::types::infer::method_dispatch_key):
    /// an arity-0 class keys by the solved full type, a constructor class by
    /// the head constructor. The two cannot be mixed — a `Functor IO` body
    /// typed with the evidence's nominal `App("IO", [])` instead of the
    /// registry's `Data("IO", [])` would see a non-record where a field is
    /// projected.
    pub(crate) class_arity: usize,
    /// The instance-registry key of `ty` for `class` (see [`instance_key`]).
    pub(crate) ty_key: String,
    /// Whether `ty_key` names a buffer type. A kernel's availability can depend
    /// on the operand kind (see [`kernels::has_instruction`]), so the kind
    /// travels with the plan instead of being re-derived from a spelling.
    pub(crate) is_buffer: bool,
    /// The class's DIRECT superclasses as `(class, ty_key)` pairs, in the order
    /// the class declares them. Each is planned earlier than this pair, so its
    /// descriptor index is already known when this one is materialised.
    pub(crate) supers: Vec<(String, String)>,
    /// The class's declared type variable, needed to type a body's parameters.
    pub(crate) class_var: String,
    /// Method names in the class's declaration order, parallel to
    /// [`Self::slots`].
    pub(crate) method_names: Vec<String>,
    /// How each method is provided, parallel to [`Self::method_names`].
    pub(crate) slots: Vec<SlotKind>,
}

/// The planning result: the ordered plan and the pairs that could not be
/// described.
#[derive(Debug, Clone, Default)]
pub(crate) struct Plan {
    /// Dictionaries in topological order.
    pub(crate) dicts: Vec<DictPlan>,
    /// Pairs the program mentions that this stage cannot express.
    pub(crate) gaps: Vec<DictGap>,
}

/// The instance-registry key of `ty` for `class`.
///
/// The registry's own identity, in the registry's own order: the full
/// [`value_type_key`](crate::types::ty::value_type_key) when an instance is
/// registered under it (`Pair Int Int`, `FixedBuffer Double`), otherwise the
/// head name a partial or derived instance is keyed by (`Kleisli`, `ST`).
///
/// When neither exists, the pair has no registry identity at all — an abstract
/// given-constraint witness (`C g`) or a type no instance declares. `None` then,
/// so the caller reports a gap instead of inventing a key.
pub(crate) fn instance_key(env: &TypeEnv, class: &str, ty: &ValueTy) -> Option<String> {
    let by_ty = env.instances.get(class);
    let registered = |key: &str| by_ty.map(|m| m.contains_key(key)).unwrap_or(false);
    let full = crate::types::ty::value_type_key(env, ty);
    if let Some(key) = &full {
        if registered(key) {
            return Some(key.clone());
        }
    }
    if let Some(key) = env.type_name_of_vty(ty) {
        if registered(&key) {
            return Some(key);
        }
    }
    None
}

/// Plan every dictionary the program needs from the pairs it mentions.
///
/// `mentioned` is the evidence
/// [`dict_selections`](crate::types::infer::TypedProgram::dict_selections)
/// recorded: `(class, concrete type)` for a call site, or `(class,
/// ValueTy::Dict(g))` under a given constraint. Many call sites, one dictionary
/// — duplicate pairs collapse to a single descriptor.
pub(crate) fn plan(env: &TypeEnv, mentioned: &[(String, ValueTy)]) -> Result<Plan, CompileError> {
    // A deterministic starting order: the evidence arrives from a hash map, so
    // its iteration order cannot decide the IR's dictionary layout.
    let mut seeds: Vec<(String, ValueTy)> = mentioned.to_vec();
    seeds.sort_by_key(|(class, ty)| format!("{class}\u{1}{ty:?}"));
    seeds.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    let mut plan = Plan::default();
    let mut order: Vec<(String, ValueTy)> = Vec::new();
    // The registry key already planned, so two spellings of one type (`App`
    // vs `Data` flavour) still yield exactly ONE dictionary.
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut visiting: Vec<String> = Vec::new();
    for (class, ty) in &seeds {
        visit(
            env,
            class,
            ty,
            &mut order,
            &mut seen,
            &mut visiting,
            &mut plan.gaps,
        )?;
    }

    // Materialise in plan order. A pair whose superclass produced no
    // descriptor cannot be built either: a dictionary must never hold a
    // dangling superclass ref.
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for (class, ty) in &order {
        let Some(ty_key) = instance_key(env, class, ty) else {
            continue;
        };
        let Some(info) = env
            .instances
            .get(class.as_str())
            .and_then(|m| m.get(ty_key.as_str()))
            .cloned()
        else {
            continue;
        };
        let Some(class_info) = env.typeclasses.get(class.as_str()) else {
            continue;
        };
        if let Some(reason) = unbuildable_reason(env, class, &ty_key, &info) {
            plan.gaps.push(DictGap {
                class: class.clone(),
                ty_key: Some(ty_key),
                reason,
            });
            continue;
        }
        // Every superclass dictionary must already be planned; the plan order
        // guarantees that when nothing was dropped.
        let mut supers = Vec::with_capacity(class_info.superclasses.len());
        let mut missing_super = None;
        for (sup, _var) in &class_info.superclasses {
            match instance_key(env, sup, ty) {
                Some(sup_key) if index.contains_key(&(sup.clone(), sup_key.clone())) => {
                    supers.push((sup.clone(), sup_key));
                }
                _ => {
                    missing_super = Some(sup.clone());
                    break;
                }
            }
        }
        if let Some(sup) = missing_super {
            plan.gaps.push(DictGap {
                class: class.clone(),
                ty_key: Some(ty_key),
                reason: format!(
                    "superclass `{sup}` has no dictionary for this type in this build \
                     (design §3.10: a dictionary that is a function of another \
                     dictionary is not expressible yet)"
                ),
            });
            continue;
        }
        let method_names: Vec<String> = class_info.methods.iter().map(|(m, _)| m.clone()).collect();
        let slots: Vec<SlotKind> = method_names
            .iter()
            .map(|m| slot_kind(class, m, &info))
            .collect();
        index.insert((class.clone(), ty_key.clone()), plan.dicts.len());
        plan.dicts.push(DictPlan {
            class: class.clone(),
            ty: ty.clone(),
            is_buffer: env.instance_key_is_buffer(&ty_key),
            ty_key,
            supers,
            class_var: class_info.var.clone(),
            class_arity: class_info.arity,
            method_names,
            slots,
        });
    }
    Ok(plan)
}

/// Depth-first visit of one `(class, type)` pair: plan its superclasses first,
/// then itself.
///
/// `visiting` is the DFS stack of class names, so re-entering one means the
/// program's superclass declarations form a cycle. That is a hard error naming
/// the loop — the design forbids a silent truncation, and a truncated table
/// would leave a dictionary without its superclass.
fn visit(
    env: &TypeEnv,
    class: &str,
    ty: &ValueTy,
    order: &mut Vec<(String, ValueTy)>,
    seen: &mut std::collections::HashSet<(String, String)>,
    visiting: &mut Vec<String>,
    gaps: &mut Vec<DictGap>,
) -> Result<(), CompileError> {
    // The cycle check comes FIRST: an already-planned pair short-circuits, but a
    // pair that is still on the DFS stack means the superclass declarations
    // themselves loop.
    if let Some(start) = visiting.iter().position(|c| c == class) {
        let mut cycle: Vec<String> = visiting[start..].to_vec();
        cycle.push(class.to_string());
        return Err(CompileError::Type {
            msg: format!(
                "superclass cycle in dictionary closure: {}",
                cycle.join(" -> ")
            ),
            span: Span::new(0, 0),
        });
    }
    let key = instance_key(env, class, ty);
    if let Some(key) = &key {
        if !seen.insert((class.to_string(), key.clone())) {
            return Ok(());
        }
    }
    let Some(ty_key) = key else {
        // No registry identity: an abstract given-constraint witness (`C g`) or
        // a type no instance declares. It cannot be ordered or built, and the
        // reason has to be visible.
        gaps.push(DictGap {
            class: class.to_string(),
            ty_key: crate::types::ty::value_type_key(env, ty),
            reason: match ty {
                ValueTy::Dict(given) => format!(
                    "abstract witness of the given constraint `{given}`: the dictionary \
                     arrives as a parameter (design §3.7 step 3), not from this table"
                ),
                other => format!("no instance of `{class}` is registered for `{other:?}`"),
            },
        });
        return Ok(());
    };
    let Some(info) = env
        .instances
        .get(class)
        .and_then(|m| m.get(ty_key.as_str()))
        .cloned()
    else {
        return Ok(());
    };
    // A pair that cannot be built is still ORDERED, so its dependents are
    // rejected with a reason instead of silently getting an incomplete table.
    let unbuildable = unbuildable_reason(env, class, &ty_key, &info);
    visiting.push(class.to_string());
    if unbuildable.is_none() {
        for (sup, _var) in env
            .typeclasses
            .get(class)
            .map(|c| c.superclasses.clone())
            .unwrap_or_default()
        {
            // A superclass constraint shares the class variable, so it
            // describes the SAME type as the subclass dictionary.
            visit(env, &sup, ty, order, seen, visiting, gaps)?;
        }
    }
    visiting.pop();
    // The gap itself is recorded when the pair is materialised, so a pair that
    // is both unbuildable AND a broken dependent reports one reason, not two.
    order.push((class.to_string(), ty.clone()));
    Ok(())
}

/// Why the pair has no descriptor, or `None` when it can be built.
///
/// Every clause is a real limitation of a flat vtable, and each one becomes the
/// verbatim [`DictGap::reason`] so the report never claims more than is true.
fn unbuildable_reason(
    env: &TypeEnv,
    class: &str,
    ty_key: &str,
    info: &InstanceInfo,
) -> Option<String> {
    // Design §3.10: a constraint-parameterised instance's dictionary is a
    // FUNCTION of other dictionaries; `supers: Vec<ref>` cannot express it.
    if !info.constraints.is_empty() {
        let context: Vec<String> = info
            .constraints
            .iter()
            .map(|(c, v)| format!("{c} {v}"))
            .collect();
        return Some(format!(
            "constraint-parameterised instance (`{}`): its dictionary is a function of \
             another dictionary (design §3.10)",
            context.join(", ")
        ));
    }
    let missing: Vec<String> = env
        .typeclasses
        .get(class)
        .map(|c| {
            c.methods
                .iter()
                .filter(|(method, _)| {
                    slot_kind(class, method, info) == SlotKind::Body
                        && body_of(env, class, ty_key, method).is_none()
                })
                .map(|(method, _)| method.clone())
                .collect()
        })
        .unwrap_or_default();
    if !missing.is_empty() {
        return Some(format!(
            "the instance provides no body and the class declares no default for method(s) {}",
            missing.join(", ")
        ));
    }
    None
}

/// How `method` is provided for a resolved instance.
///
/// A primitive instance's methods are compiler kernels — but only where
/// [`kernels::kernel_for_method`] names one. `Ord`'s `min`/`max` are ordinary
/// default bodies even on a primitive instance, so they fall through to
/// [`SlotKind::Body`] and are compiled like any other equation. This is the same
/// decision lowering's primitive-call path makes, from the same table.
fn slot_kind(class: &str, method: &str, info: &InstanceInfo) -> SlotKind {
    if info.primitive {
        if let Some(kernel) = kernels::kernel_for_method(class, method) {
            return SlotKind::Kernel(kernel);
        }
    }
    SlotKind::Body
}

/// The source body that provides `method` for the pair, if any: the instance's
/// own equation first, then the class default (the same precedence
/// `instantiate_method` applies).
pub(crate) fn body_of(
    env: &TypeEnv,
    class: &str,
    ty_key: &str,
    method: &str,
) -> Option<(Vec<String>, crate::ast::Expr)> {
    if let Some(found) = env
        .instances
        .get(class)
        .and_then(|m| m.get(ty_key))
        .and_then(|i| i.methods.get(method))
        .cloned()
    {
        return Some(found);
    }
    env.typeclasses
        .get(class)
        .and_then(|c| c.defaults.get(method).cloned())
}
