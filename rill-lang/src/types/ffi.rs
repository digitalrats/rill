//! FFI signature descriptors: the language-side contract for foreign builtins.
//!
//! A `foreign fn name : TypeExpr;` declaration is converted into an [`FfiSig`]
//! — a flat list of parameter descriptors consumed by inference, lowering and
//! graph reconstruction. `FixedBuffer a` is the signal-channel type; scalars are
//! compile-time params; `Tape a` is a shared-buffer resource handle.

use crate::ast::TypeExpr;
use crate::types::ty::{DataInfo, TypeEnv, ValueTy};

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
    /// A `Data`-typed record parameter (e.g. a mixer config).
    Record(String),
    /// A named shared buffer (`Tape a`) — a symbolic resource reference, NOT a
    /// signal. Lowering records the referenced name; the build path resolves it.
    Resource,
}

/// A parsed foreign function signature.
#[derive(Debug, Clone, PartialEq)]
pub struct FfiSig {
    /// Parameter descriptors in declaration order.
    pub params: Vec<FfiParam>,
    /// Number of signal output channels (result `FixedBuffer`s).
    pub signal_outs: usize,
    /// Display names of the scalar params in declaration order (graph
    /// reconstruction orders a node's params by these). Filled from the
    /// catalog-backed name table ([`foreign_param_names`]); a name absent from
    /// the table falls back to index names (`param0`, `param1`, …).
    pub param_names: Vec<String>,
}

/// A record schema (field name, scalar type, default), mirroring the legacy
/// `RecordSchema`. Filled from a `data` declaration's fields; the catalog's
/// `data` syntax carries no defaults, so `default` is `None` for catalog types.
#[derive(Debug, Clone, PartialEq)]
pub struct FfiRecordSchema {
    /// Fields in declaration order. A non-scalar field (e.g. `bands: List
    /// EqBand`) is omitted — a list-typed field flattens to zero params in v1.
    pub fields: Vec<(String, FfiScalar, Option<f64>)>,
}

/// The scalar element type of a record field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FfiScalar {
    /// A `Float`-typed field.
    Float,
    /// An `Int`-typed field.
    Int,
}

/// The catalog-backed scalar-param display names, mirroring the legacy
/// `BuiltinSig::param_names` registrations (verified against each crate's
/// `with_names` calls). Graph reconstruction consumes these to match GraphSpec
/// node params to FFI arg positions by name.
///
/// A name missing from this table falls back to index names in
/// [`ffi_sig_from_typeexpr`].
pub fn foreign_param_names(name: &str) -> Vec<String> {
    const TABLE: &[(&str, &[&str])] = &[
        ("sine", &["freq", "amp", "phase"]),
        ("saw", &["freq", "amp", "phase"]),
        ("square", &["freq", "amp", "phase"]),
        ("triangle", &["freq", "amp", "phase"]),
        ("noise", &["type", "amp"]),
        ("complex", &["re", "im"]),
        ("ay38910", &["clock", "regs"]),
        ("sampler", &["gate", "rate", "amp", "cubic", "source"]),
        ("leaky_integrator", &["coeff"]),
        ("onepole", &["cutoff", "q"]),
        ("moog", &["cutoff", "resonance"]),
        ("lowpass", &["cutoff", "q"]),
        ("highpass", &["cutoff", "q"]),
        ("biquad", &["type", "cutoff", "q", "gain_db"]),
        ("delay", &["time", "feedback", "dry_wet"]),
        ("distortion", &["drive", "gain"]),
        ("limiter", &["threshold", "ratio"]),
        ("graphic_eq", &["gain"]),
        ("mono_to_stereo", &["pan", "smoothing"]),
        ("spectralgate", &["threshold", "ratio"]),
        ("spectraldelay", &["mix", "feedback"]),
        ("convolver", &["ir_gain", "mix"]),
        ("analog_moog", &["cutoff", "resonance"]),
        (
            "lofi",
            &[
                "bit_depth",
                "sample_rate",
                "dry_wet",
                "gain",
                "bitcrush",
                "sr_reduction",
                "noise",
            ],
        ),
        ("write_head", &["delay_time", "feedback"]),
        ("read_head", &["delay"]),
    ];
    TABLE
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, names)| names.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

/// Read a record type's scalar fields from `TypeEnv::data_types`, converting
/// `ValueTy::Float` → [`FfiScalar::Float`] and `ValueTy::Int` →
/// [`FfiScalar::Int`]. A non-scalar field (a nested record, a `List` of bands)
/// is skipped — it flattens to zero params in v1. Returns `None` when the type
/// is not a declared record.
pub fn ffi_record_schema(env: &TypeEnv, type_name: &str) -> Option<FfiRecordSchema> {
    let fields = match env.data_types.get(type_name)? {
        DataInfo::Record(fields) => fields,
        DataInfo::Sum(_) => return None,
    };
    let mut out = Vec::new();
    for (name, vty) in fields {
        match vty {
            ValueTy::Float => out.push((name.clone(), FfiScalar::Float, None)),
            ValueTy::Int => out.push((name.clone(), FfiScalar::Int, None)),
            _ => {} // non-scalar field (e.g. `bands: List EqBand`) — no params.
        }
    }
    Some(FfiRecordSchema { fields: out })
}

/// Convert a carried `TypeExpr` (`a -> b -> c -> r`) into an [`FfiSig`].
///
/// Supported parameter types:
/// - `FixedBuffer a` → [`FfiParam::Signal`]
/// - `Float`/`Int`/`Bool`/`String` → [`FfiParam::Scalar`]
/// - `List (FixedBuffer a)` → [`FfiParam::VariadicSignal`]
/// - `Tape a` → [`FfiParam::Resource`]
/// - any other `TName` (a `Data` record type) → [`FfiParam::Record(name)`]
///
/// The result type is one or more `FixedBuffer` channels (`FixedBuffer a` →
/// 1 out; `(FixedBuffer a, FixedBuffer b)` → 2 outs). Returns `None` for a
/// malformed signature (non-carried, unsupported result).
pub fn ffi_sig_from_typeexpr(name: &str, te: &TypeExpr) -> Option<FfiSig> {
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
                    param_names: param_names_for(name, &params),
                    params,
                    signal_outs: outs_from_typeexpr(other)?,
                });
            }
        }
    }
}

/// Scalar display names in declaration order: the catalog table for known
/// builtins, index names (`param0`, `param1`, …) for unknown/foreign ones.
fn param_names_for(name: &str, params: &[FfiParam]) -> Vec<String> {
    let table = foreign_param_names(name);
    let mut next = 0usize;
    let mut scalar_idx = 0usize;
    let mut out = Vec::new();
    for p in params {
        if matches!(p, FfiParam::Scalar) {
            out.push(
                table
                    .get(next)
                    .cloned()
                    .unwrap_or_else(|| format!("param{scalar_idx}")),
            );
            next += 1;
            scalar_idx += 1;
        }
    }
    out
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
        TypeExpr::TApp(head, args) if head == "Tape" && args.len() == 1 => {
            // `Tape a` — a named shared buffer (resource), not a signal.
            Some(FfiParam::Resource)
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
