//! FFI signature descriptors: the language-side contract for foreign builtins.
//!
//! A `foreign fn name : TypeExpr;` declaration is converted into an [`FfiSig`]
//! — a flat list of parameter descriptors consumed by inference and lowering.
//! `FixedBuffer a` is the signal-channel type; scalars are compile-time params.

use crate::ast::TypeExpr;

/// One parameter of a foreign function signature.
#[derive(Debug, Clone, PartialEq)]
pub enum FfiParam {
    /// A signal channel: `FixedBuffer a`. Contributes one input arity.
    Signal,
    /// A compile-time scalar parameter: `Float`/`Int`/`Bool`/`String`.
    Scalar,
    /// Variadic signal channels: `List (FixedBuffer a)`. Consumes all remaining
    /// signal arguments.
    VariadicSignal,
    /// A `Data`-typed record parameter (e.g. a mixer config). SP-3b.
    Record(String),
}

/// A parsed foreign function signature.
#[derive(Debug, Clone, PartialEq)]
pub struct FfiSig {
    /// Parameter descriptors in declaration order.
    pub params: Vec<FfiParam>,
    /// Number of signal output channels (result `FixedBuffer`s).
    pub signal_outs: usize,
}

/// Convert a carried `TypeExpr` (`a -> b -> c -> r`) into an [`FfiSig`].
///
/// Supported parameter types:
/// - `FixedBuffer a` → [`FfiParam::Signal`]
/// - `Float`/`Int`/`Bool`/`String` → [`FfiParam::Scalar`]
/// - `List (FixedBuffer a)` → [`FfiParam::VariadicSignal`]
/// - any other `TName` (a `Data` record type) → [`FfiParam::Record(name)`]
///
/// The result type is one or more `FixedBuffer` channels (`FixedBuffer a` →
/// 1 out; `(FixedBuffer a, FixedBuffer b)` → 2 outs). Returns `None` for a
/// malformed signature (non-carried, unsupported result).
pub fn ffi_sig_from_typeexpr(te: &TypeExpr) -> Option<FfiSig> {
    // Unroll the carried arrows into a flat param list.
    let mut params = Vec::new();
    let mut ret = te;
    loop {
        match ret {
            TypeExpr::TFunc(args, r) => {
                // The parser's carried arrows are flat (`a -> b -> c -> r` →
                // `TFunc([a,b,c], r)`); a nested one-arg TFunc also unrolls here.
                for a in args {
                    params.push(param_from_typeexpr(a)?);
                }
                ret = r;
            }
            other => {
                // A VariadicSignal that is not the LAST param is ambiguous:
                // lowering folds every remaining call arg (including a trailing
                // scalar) as a signal while inference counts a different arity.
                // Reject the whole descriptor so the name does not resolve as a
                // foreign builtin.
                if params
                    .iter()
                    .rposition(|p| matches!(p, FfiParam::VariadicSignal))
                    .is_some_and(|i| i != params.len() - 1)
                {
                    return None;
                }
                return Some(FfiSig {
                    params,
                    signal_outs: outs_from_typeexpr(other)?,
                });
            }
        }
    }
}

fn param_from_typeexpr(te: &TypeExpr) -> Option<FfiParam> {
    match te {
        TypeExpr::TApp(head, args) if head == "FixedBuffer" && args.len() == 1 => {
            Some(FfiParam::Signal)
        }
        TypeExpr::TApp(head, args) if head == "List" && args.len() == 1 => {
            // `List (FixedBuffer a)` — variadic signal channels.
            if matches!(&args[0], TypeExpr::TApp(h, a) if h == "FixedBuffer" && a.len() == 1) {
                Some(FfiParam::VariadicSignal)
            } else {
                None
            }
        }
        TypeExpr::TName(n) if matches!(n.as_str(), "Float" | "Int" | "Bool" | "String") => {
            Some(FfiParam::Scalar)
        }
        TypeExpr::TName(n) => Some(FfiParam::Record(n.clone())),
        _ => None,
    }
}

fn outs_from_typeexpr(te: &TypeExpr) -> Option<usize> {
    match te {
        TypeExpr::TApp(head, args) if head == "FixedBuffer" && args.len() == 1 => Some(1),
        // `(FixedBuffer a, FixedBuffer b)` desugars to `Pair (FixedBuffer a) (FixedBuffer b)`.
        TypeExpr::TApp(head, args) if head == "Pair" && args.len() == 2 => {
            if args
                .iter()
                .all(|a| matches!(a, TypeExpr::TApp(h, x) if h == "FixedBuffer" && x.len() == 1))
            {
                Some(2)
            } else {
                None
            }
        }
        _ => None,
    }
}
