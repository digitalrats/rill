//! Arrow type model: a program is a block transform.
//!
//! Signal types are structured in three levels: the per-sample scalar type
//! (`Scalar`), one channel (`Channel`), and a block transform (`ArrowTy`,
//! n input channels → m output channels). Arities (channel counts) are
//! synthesized separately (see `infer.rs`) because `<:`/`:>` divisibility is
//! not expressible by unification.
//!
//! Channels carry a **rate**: [`Rate::Signal`] (a block of samples per tick,
//! the existing SIMD path) or [`Rate::Value`] (one arena value per tick).
//! [`Block`] is a back-compat alias for a signal-rate `Channel`.

use std::collections::{HashMap, HashSet};

use crate::ast::Expr;
use crate::error::{CompileError, Span};

/// A unification variable identifier.
pub type TypeVarId = u32;

/// Upper bound on `resolve_value` chain-following depth. Legitimate nested
/// function types (`Func([Func([...])], ...)`) stay well below this; a value
/// type deeper than this is either a cyclic binding or a pathological program,
/// and resolving it must degrade to the unresolved var — never a stack
/// overflow. The occurs-check in `unify_value` rejects cycles at the source.
pub(crate) const MAX_VALUE_RESOLVE_DEPTH: usize = 64;

/// The scalar (element) type of a sample.
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    /// Integer.
    Int,
    /// Floating point (the runtime `T`).
    Float,
    /// Unresolved unification variable.
    Var(TypeVarId),
}

/// Wire rate: block-rate signal vs per-block value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rate {
    /// A block of samples processed per tick (existing SIMD path).
    Signal,
    /// One arena value per tick.
    Value,
}

/// Type of an arena value (per-block value channel).
#[derive(Debug, Clone, PartialEq)]
pub enum ValueTy {
    /// Integer value.
    Int,
    /// Floating point value.
    Float,
    /// A named data record.
    Data(String),
    /// A newtype wrapping another value type.
    Newtype(String),
    /// Function type: value-argument types and value-result types. Signal-wire
    /// arguments are positional wire-captures at the call site, not part of the
    /// type.
    Func(Vec<ValueTy>, Vec<ValueTy>),
    /// Unresolved value-type unification variable.
    Var(TypeVarId),
}

/// A signal or value channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    /// Wire rate: signal (block of samples) or value (one per tick).
    pub rate: Rate,
    /// Scalar type of the samples (Signal rate only).
    pub elem: Scalar,
    /// Arena value type (Value rate only).
    pub vty: ValueTy,
}

impl Channel {
    /// A signal-rate channel whose samples have scalar type `elem`.
    pub fn signal(elem: Scalar) -> Self {
        Self {
            rate: Rate::Signal,
            elem,
            vty: ValueTy::Int,
        }
    }
    /// A value-rate channel carrying arena values of type `vty`.
    pub fn value(vty: ValueTy) -> Self {
        Self {
            rate: Rate::Value,
            elem: Scalar::Int,
            vty,
        }
    }
    /// The scalar element type (Signal rate) — panics for Value rate.
    pub fn scalar(&self) -> &Scalar {
        debug_assert_eq!(self.rate, Rate::Signal);
        &self.elem
    }
    /// Back-compat constructor: a Signal-rate channel with scalar `elem`.
    pub fn new(elem: Scalar) -> Self {
        Self::signal(elem)
    }
}

/// Back-compat alias for [`Channel`] (a signal-rate channel).
pub type Block = Channel;

/// A block transform: n input channels → m output channels.
///
/// The vector *lengths* are the arities (channel counts). During inference we
/// usually know the arities as concrete integers; unification touches the
/// `Block::elem` scalars, and value-type unification (`unify_value`) lands with
/// value channels (Task 6+).
#[derive(Debug, Clone, PartialEq)]
pub struct ArrowTy {
    /// Scalar type of each input channel (len = input arity).
    pub ins: Vec<Block>,
    /// Scalar type of each output channel (len = output arity).
    pub outs: Vec<Block>,
}

impl ArrowTy {
    /// A (n_in → n_out) transform where every channel has the same scalar `s`.
    pub fn uniform(n_in: usize, n_out: usize, s: Scalar) -> ArrowTy {
        ArrowTy {
            ins: vec![Block::new(s.clone()); n_in],
            outs: vec![Block::new(s); n_out],
        }
    }
    /// A value channel (per-block value): 0 inputs → 1 output.
    pub fn value_channel(vty: ValueTy) -> ArrowTy {
        ArrowTy {
            ins: vec![],
            outs: vec![Channel::value(vty)],
        }
    }
    /// Input arity (channel count).
    pub fn arity_in(&self) -> usize {
        self.ins.len()
    }
    /// Output arity (channel count).
    pub fn arity_out(&self) -> usize {
        self.outs.len()
    }
}

/// A polymorphic type scheme `∀ vars. ty`.
///
/// `lam_count` is the number of λ-parameters for this definition
/// (user-defined function arguments — distinct from signal port inputs).
/// These appear as the **first** `lam_count` elements of `ty.ins`.
/// `ty.ins[lam_count..]` are signal port inputs (from `_` references in the body).
#[derive(Debug, Clone, PartialEq)]
pub struct Scheme {
    /// Number of λ-parameters.
    pub lam_count: usize,
    /// Quantified type variables.
    pub vars: Vec<TypeVarId>,
    /// The generalized diagram type.
    pub ty: ArrowTy,
}

/// A substitution mapping type variables to scalars (signal element types)
/// and value types (arena value types).
#[derive(Debug, Clone, Default)]
pub struct Subst {
    /// The scalar mapping (signal element types).
    pub map: HashMap<TypeVarId, Scalar>,
    /// The value-type mapping (arena value types). Value-type variables
    /// resolve structurally: unifying `Var(v)` with a concrete `ValueTy`
    /// (e.g. a `Func` signature for a lambda parameter used as a callee)
    /// records the binding here, so HOF parameter types stay concrete by the
    /// time lowering needs them (record projection needs the Data type, a
    /// function parameter needs its Func signature).
    pub value_map: HashMap<TypeVarId, ValueTy>,
}

/// Shape of a declared data type.
#[derive(Debug, Clone)]
pub enum DataInfo {
    /// A product type: field name → value type.
    Record(Vec<(String, ValueTy)>),
    /// A sum type: constructor name → payload value types.
    Sum(Vec<(String, Vec<ValueTy>)>),
}

/// A `typeclass` declaration: its type variable and method dictionary.
#[derive(Debug, Clone)]
pub struct TypeclassInfo {
    /// The class type variable (e.g. `a` in `typeclass Show a`).
    pub var: String,
    /// Method dictionary: method name → declared argument type name.
    pub methods: Vec<(String, String)>,
}

/// A concrete `instance` declaration: which class it implements, the concrete
/// type bound to the class variable, and the method bodies it provides.
#[derive(Debug, Clone)]
pub struct InstanceInfo {
    /// The class this instance implements.
    pub class: String,
    /// The concrete type name bound to the class variable.
    pub ty: String,
    /// Method bodies: method name → (optional parameter binding, body).
    pub methods: HashMap<String, (Option<String>, Expr)>,
}

/// The compile-time type environment: aliases, newtypes, data-type shapes, and
/// the typeclass/instance dictionary.
///
/// Built once during inference and carried to lowering so both phases resolve
/// type names identically (no order dependence, no duplicated resolution).
#[derive(Debug, Clone, Default)]
pub struct TypeEnv {
    /// Type synonyms (alias name → target name).
    pub type_aliases: HashMap<String, String>,
    /// Newtype wrappers (wrapper name → inner type name).
    pub newtypes: HashMap<String, String>,
    /// Data type declarations: name → shape.
    pub data_types: HashMap<String, DataInfo>,
    /// Typeclass declarations: class name → method dictionary.
    pub typeclasses: HashMap<String, TypeclassInfo>,
    /// Instances grouped by class, then by bound type name.
    pub instances: HashMap<String, HashMap<String, InstanceInfo>>,
}

impl TypeEnv {
    /// Resolve a DSL type name to a value type, following type synonyms and
    /// newtype wrappers. Alias chains resolve iteratively with a bounded loop
    /// (cycle-safe): each pass follows one link and there are at most
    /// `len(aliases)` distinct links to follow.
    pub fn vty_of_name(&self, name: &str) -> ValueTy {
        let mut cur = name.to_string();
        for _ in 0..=self.type_aliases.len() {
            match self.type_aliases.get(&cur) {
                Some(target) => cur = target.clone(),
                None => break,
            }
        }
        match cur.as_str() {
            "Float" => ValueTy::Float,
            "Int" => ValueTy::Int,
            n => {
                if self.newtypes.contains_key(n) {
                    ValueTy::Newtype(n.to_string())
                } else {
                    ValueTy::Data(n.to_string())
                }
            }
        }
    }

    /// Find the class whose method dictionary declares `method`. Returns
    /// `None` when no class declares it (the name may be a definition or
    /// builtin) and when several classes declare the same method name
    /// (ambiguous — the caller reports it as unresolvable).
    pub fn class_of_method(&self, method: &str) -> Option<String> {
        let mut found: Option<String> = None;
        for (cname, info) in &self.typeclasses {
            if info.methods.iter().any(|(m, _)| m == method) {
                if found.is_some() {
                    return None;
                }
                found = Some(cname.clone());
            }
        }
        found
    }

    /// The DSL type name of a concrete value type, used to look up instances
    /// (`Float`, `Int`, a data type name, a newtype name). `None` for
    /// unresolved type variables — a method call over such an argument cannot
    /// select an instance at compile time.
    pub fn type_name_of_vty(&self, v: &ValueTy) -> Option<String> {
        match v {
            ValueTy::Int => Some("Int".to_string()),
            ValueTy::Float => Some("Float".to_string()),
            ValueTy::Data(n) => Some(n.clone()),
            ValueTy::Newtype(n) => Some(n.clone()),
            _ => None,
        }
    }

    /// Validate that all data types and newtypes are acyclic. A data type that
    /// (transitively) references itself through a field, payload, or newtype
    /// inner is rejected: v1 guarantees acyclicity at compile time so the
    /// arena-capacity bound (`subtree_size`) is exact and RC is sound
    /// (spec §9.1, the `strict` contract).
    pub fn check_acyclic(&self) -> Result<(), CompileError> {
        let mut visiting = HashSet::new();
        for name in self.data_types.keys() {
            self.check_acyclic_name(name, &mut visiting)?;
        }
        visiting.clear();
        for name in self.newtypes.keys() {
            self.check_acyclic_newtype(name, &mut visiting)?;
        }
        Ok(())
    }

    fn check_acyclic_name(
        &self,
        name: &str,
        visiting: &mut HashSet<String>,
    ) -> Result<(), CompileError> {
        if !visiting.insert(name.to_string()) {
            return Err(CompileError::Type {
                msg: format!(
                    "recursive data type `{name}` is not supported in v1 (acyclicity is required)"
                ),
                span: Span::new(0, 0),
            });
        }
        match self.data_types.get(name) {
            Some(DataInfo::Record(fields)) => {
                for (_, t) in fields {
                    match t {
                        ValueTy::Data(inner) => self.check_acyclic_name(inner, visiting)?,
                        ValueTy::Newtype(inner) => self.check_acyclic_newtype(inner, visiting)?,
                        _ => {}
                    }
                }
            }
            Some(DataInfo::Sum(ctors)) => {
                for (_, payload) in ctors {
                    for t in payload {
                        match t {
                            ValueTy::Data(inner) => self.check_acyclic_name(inner, visiting)?,
                            ValueTy::Newtype(inner) => {
                                self.check_acyclic_newtype(inner, visiting)?
                            }
                            _ => {}
                        }
                    }
                }
            }
            None => {}
        }
        visiting.remove(name);
        Ok(())
    }

    fn check_acyclic_newtype(
        &self,
        name: &str,
        visiting: &mut HashSet<String>,
    ) -> Result<(), CompileError> {
        if !visiting.insert(name.to_string()) {
            return Err(CompileError::Type {
                msg: format!(
                    "recursive newtype `{name}` is not supported in v1 (acyclicity is required)"
                ),
                span: Span::new(0, 0),
            });
        }
        let res = match self.newtypes.get(name) {
            Some(inner) => match self.vty_of_name(inner) {
                ValueTy::Data(data_name) => self.check_acyclic_name(&data_name, visiting),
                ValueTy::Newtype(nw_name) => self.check_acyclic_newtype(&nw_name, visiting),
                _ => Ok(()),
            },
            None => Ok(()),
        };
        visiting.remove(name);
        res
    }

    /// Resolve a typeclass method call: the class declaring `method`, the
    /// concrete type name `ty_name`, and the matching instance's method body
    /// `(parameter binding, body)`. Returns `None` when no class declares
    /// `method` or no instance binds `ty_name`.
    pub fn resolve_method(
        &self,
        method: &str,
        ty_name: &str,
    ) -> Option<(String, Option<String>, Expr)> {
        if let Some(cname) = self.class_of_method(method) {
            if let Some(instance) = self
                .instances
                .get(cname.as_str())
                .and_then(|by_ty| by_ty.get(ty_name))
            {
                if let Some((param, body)) = instance.methods.get(method).cloned() {
                    return Some((cname, param, body));
                }
            }
        }
        None
    }
}

impl Subst {
    /// Follow the substitution chain for a single scalar to its representative.
    pub fn resolve_scalar(&self, s: &Scalar) -> Scalar {
        match s {
            Scalar::Var(v) => match self.map.get(v) {
                Some(inner) => self.resolve_scalar(inner),
                None => s.clone(),
            },
            _ => s.clone(),
        }
    }
    /// Follow the substitution chain for a single value type to its
    /// representative, resolving variables inside `Func` signatures too.
    ///
    /// Bounded by [`MAX_VALUE_RESOLVE_DEPTH`]: a (lowering-bug or
    /// already-rejected) cyclic binding degrades to the unresolved var rather
    /// than overflowing the stack. The occurs-check in `unify_value` rejects
    /// cycles at the source, so this is pure defense-in-depth.
    pub fn resolve_value(&self, t: &ValueTy) -> ValueTy {
        self.resolve_value_depth(t, 0)
    }

    /// Depth-bounded core of [`Self::resolve_value`].
    fn resolve_value_depth(&self, t: &ValueTy, depth: usize) -> ValueTy {
        if depth > MAX_VALUE_RESOLVE_DEPTH {
            return t.clone();
        }
        match t {
            ValueTy::Var(v) => match self.value_map.get(v) {
                Some(inner) => self.resolve_value_depth(inner, depth + 1),
                None => t.clone(),
            },
            ValueTy::Func(args, rets) => ValueTy::Func(
                args.iter()
                    .map(|a| self.resolve_value_depth(a, depth + 1))
                    .collect(),
                rets.iter()
                    .map(|r| self.resolve_value_depth(r, depth + 1))
                    .collect(),
            ),
            _ => t.clone(),
        }
    }
    /// Apply the substitution across a whole type.
    ///
    /// Signal-rate channels resolve their scalar element type; value-rate
    /// channels resolve their value type (lambda-parameter variables become
    /// the concrete types bound by call-site unification).
    pub fn apply(&self, t: &ArrowTy) -> ArrowTy {
        let block = |b: &Channel| match b.rate {
            Rate::Value => Channel::value(self.resolve_value(&b.vty)),
            Rate::Signal => Channel::signal(self.resolve_scalar(&b.elem)),
        };
        ArrowTy {
            ins: t.ins.iter().map(&block).collect(),
            outs: t.outs.iter().map(&block).collect(),
        }
    }
}

#[cfg(test)]
mod funcsig_tests {
    use super::*;

    #[test]
    fn func_signature_carries_arg_and_result_types() {
        let f = ValueTy::Func(vec![ValueTy::Float, ValueTy::Float], vec![ValueTy::Float]);
        assert_eq!(
            f,
            ValueTy::Func(vec![ValueTy::Float, ValueTy::Float], vec![ValueTy::Float])
        );
        let g = ValueTy::Func(vec![ValueTy::Float], vec![ValueTy::Float]);
        assert_ne!(f, g);
    }
}

#[cfg(test)]
mod channel_tests {
    use super::*;

    #[test]
    fn channel_rates_are_distinct() {
        let sig = Channel::signal(Scalar::Float);
        let val = Channel::value(ValueTy::Data("Point".into()));
        assert_eq!(sig.rate, Rate::Signal);
        assert_eq!(val.rate, Rate::Value);
        assert_eq!(sig.vty, ValueTy::Int);
    }

    #[test]
    fn apply_preserves_value_channels() {
        let s = Subst::default();
        let t = ArrowTy {
            ins: vec![Channel::value(ValueTy::Int)],
            outs: vec![],
        };
        let r = s.apply(&t);
        assert_eq!(r.ins[0].rate, Rate::Value);
        assert_eq!(r.ins[0].vty, ValueTy::Int);
    }
}
