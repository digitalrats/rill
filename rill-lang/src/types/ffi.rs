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
/// `data` syntax carries no defaults, so `default` is sourced from
/// [`foreign_record_defaults`] (the legacy `RecordField::default` values).
#[derive(Debug, Clone, PartialEq)]
pub struct FfiRecordSchema {
    /// Fields in declaration order. A list-of-record field (`bands: List
    /// EqBand`) is represented as [`FfiScalar::BandList`] carrying the band
    /// record type name — it flattens to one band-record's fields per element.
    pub fields: Vec<(String, FfiScalar, Option<f64>)>,
}

/// The scalar element type of a record field.
#[derive(Debug, Clone, PartialEq)]
pub enum FfiScalar {
    /// A `Float`-typed field.
    Float,
    /// An `Int`-typed field.
    Int,
    /// A `List`-typed field of band records (`bands: List EqBand`): a sequence
    /// of band records, each flattened in schema order. Holds the band record
    /// type name.
    BandList(String),
}

/// The legacy `RecordField::default` values for the migrated record configs
/// (`dry_wet`/`mixer`/`eq`), verified against `rill-lang/src/register.rs`'s
/// `RecordSchema::new` calls. The catalog `data` syntax carries no defaults, so
/// the FFI schema sources them from here — an omitted field keeps its legacy
/// default instead of silently becoming 0.0.
pub(crate) fn foreign_record_defaults(type_name: &str, field_name: &str) -> Option<f64> {
    const TABLE: &[(&str, &[(&str, f64)])] = &[
        ("DryWetConfig", &[("mix", 0.5)]),
        ("MixerConfig", &[("buses", 0.0), ("master_vol", 1.0)]),
        (
            "EqBand",
            &[
                ("freq", 1000.0),
                ("q", 1.0),
                ("gain_db", 0.0),
                ("band_type", 0.0),
            ],
        ),
    ];
    TABLE
        .iter()
        .find(|(t, _)| *t == type_name)
        .and_then(|(_, fields)| fields.iter().find(|(f, _)| *f == field_name))
        .map(|(_, v)| *v)
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
/// [`FfiScalar::Int`]. A `List`-typed field of band records (`bands: List
/// EqBand`) becomes [`FfiScalar::BandList`] carrying the band record type
/// name. Defaults come from [`foreign_record_defaults`] (the legacy
/// `RecordField::default` values). Returns `None` when the type is not a
/// declared record.
pub fn ffi_record_schema(env: &TypeEnv, type_name: &str) -> Option<FfiRecordSchema> {
    let fields = match env.data_types.get(type_name)? {
        DataInfo::Record(fields) => fields,
        DataInfo::Sum(_) => return None,
    };
    let mut out = Vec::new();
    for (name, vty) in fields {
        let default = foreign_record_defaults(type_name, name.as_str());
        match vty {
            ValueTy::Float => out.push((name.clone(), FfiScalar::Float, default)),
            ValueTy::Int => out.push((name.clone(), FfiScalar::Int, default)),
            // `bands: List EqBand` — a sequence of band records, each flattened
            // in the band schema's field order at lowering.
            ValueTy::App(head, args)
                if head == "List"
                    && args.len() == 1
                    && matches!(&args[0], ValueTy::Data(_band_ty, _)) =>
            {
                let ValueTy::Data(band_ty, _) = &args[0] else {
                    unreachable!()
                };
                out.push((name.clone(), FfiScalar::BandList(band_ty.clone()), default));
            }
            _ => {} // other non-scalar fields contribute no params.
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
                // A VariadicSignal that is not the LAST param is ambiguous —
                // lowering would fold the trailing call args (a trailing scalar)
                // as signals while inference counts a different arity. EXCEPT a
                // trailing RECORD param: the record is the config scalar and the
                // variadic's signal span stops before it (mixer's `List
                // (FixedBuffer f32) -> MixerConfig`). `[Signal, VariadicSignal,
                // Scalar]` stays invalid.
                if let Some(i) = params
                    .iter()
                    .rposition(|p| matches!(p, FfiParam::VariadicSignal))
                {
                    let trailing = &params[i + 1..];
                    let only_records = trailing.iter().all(|p| matches!(p, FfiParam::Record(_)));
                    if i != params.len() - 1 && !only_records {
                        return None;
                    }
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
