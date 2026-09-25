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
    pub fn apply(&self, t: &ArrowTy) -> ArrowTy {
        // TODO(Task 7): preserve `rate`/`vty` through substitution. For now a
        // Value-rate channel reaching here is flattened to Signal/Int by the
        // `Block::new` back-compat constructor — fail loudly instead of silently
        // corrupting the type.
        ArrowTy {
            ins: t
                .ins
                .iter()
                .map(|b| {
                    debug_assert_eq!(b.rate, Rate::Signal);
                    Block::new(self.resolve_scalar(&b.elem))
                })
                .collect(),
            outs: t
                .outs
                .iter()
                .map(|b| {
                    debug_assert_eq!(b.rate, Rate::Signal);
                    Block::new(self.resolve_scalar(&b.elem))
                })
                .collect(),
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

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic]
    fn apply_asserts_on_value_channel() {
        let s = Subst::default();
        let t = ArrowTy {
            ins: vec![Channel::value(ValueTy::Int)],
            outs: vec![],
        };
        s.apply(&t);
    }
}
