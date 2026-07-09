//! Type representation for the HM scalar layer.
//!
//! Scalar types classify the *element* type of a signal wire. Arities (wire
//! counts) are synthesized separately (see `infer.rs`) because `<:`/`:>`
//! divisibility is not expressible by unification.

use std::collections::HashMap;

/// A unification variable identifier.
pub type TypeVarId = u32;

/// The scalar (element) type of a wire.
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    /// Integer.
    Int,
    /// Floating point (the runtime `T`).
    Float,
    /// Unresolved unification variable.
    Var(TypeVarId),
}

/// A block/diagram type: the scalar type of each input and output wire.
///
/// The vector *lengths* are the arities. During inference we usually know the
/// arities as concrete integers; unification only touches the `Scalar`s.
#[derive(Debug, Clone, PartialEq)]
pub struct Type {
    /// Scalar type of each input wire (len = input arity).
    pub ins: Vec<Scalar>,
    /// Scalar type of each output wire (len = output arity).
    pub outs: Vec<Scalar>,
}

impl Type {
    /// A (n_in → n_out) type where every wire has the same scalar `s`.
    pub fn uniform(n_in: usize, n_out: usize, s: Scalar) -> Type {
        Type {
            ins: vec![s.clone(); n_in],
            outs: vec![s; n_out],
        }
    }
    /// Input arity.
    pub fn arity_in(&self) -> usize {
        self.ins.len()
    }
    /// Output arity.
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
    pub ty: Type,
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
    pub fn apply(&self, t: &Type) -> Type {
        Type {
            ins: t.ins.iter().map(|s| self.resolve_scalar(s)).collect(),
            outs: t.outs.iter().map(|s| self.resolve_scalar(s)).collect(),
        }
    }
}
