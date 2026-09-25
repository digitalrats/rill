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

use std::collections::HashMap;

/// A unification variable identifier.
pub type TypeVarId = u32;

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
    /// A function value.
    Func(String),
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

/// A substitution mapping type variables to scalars.
#[derive(Debug, Clone, Default)]
pub struct Subst {
    /// The mapping.
    pub map: HashMap<TypeVarId, Scalar>,
}

/// Shape of a declared data type.
#[derive(Debug, Clone)]
pub enum DataInfo {
    /// A product type: field name → value type.
    Record(Vec<(String, ValueTy)>),
    /// A sum type: constructor name → payload value types.
    Sum(Vec<(String, Vec<ValueTy>)>),
}

/// The compile-time type environment: aliases, newtypes, and data-type shapes.
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
    /// Apply the substitution across a whole type.
    ///
    /// Value-rate channels carry concrete value types (`vty`) that are not
    /// quantified in v1, so they pass through unchanged.
    pub fn apply(&self, t: &ArrowTy) -> ArrowTy {
        let block = |b: &Channel| match b.rate {
            Rate::Value => Channel::value(b.vty.clone()),
            Rate::Signal => Channel::signal(self.resolve_scalar(&b.elem)),
        };
        ArrowTy {
            ins: t.ins.iter().map(&block).collect(),
            outs: t.outs.iter().map(&block).collect(),
        }
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
