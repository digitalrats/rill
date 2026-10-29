//! FFI signature descriptors: the language-side contract for foreign builtins.
//!
//! A `foreign name : TypeExpr;` declaration is converted into an [`FfiSig`]
//! — a flat list of parameter descriptors consumed by inference and lowering.
//! `FixedBuffer a` is the signal-channel type; scalars are
//! compile-time params; `Tape a` is a shared-buffer resource handle.

use crate::ast::TypeExpr;
use crate::types::ty::{BufferRole, DataInfo, TypeEnv, ValueTy};

/// One parameter of a foreign function signature.
#[derive(Debug, Clone, PartialEq)]
pub enum FfiParam {
    /// A signal channel: `FixedBuffer elem`. Carries the ELEMENT the signature
    /// declares, so the layer distinguishes `FixedBuffer Double` from
    /// `FixedBuffer Int` instead of erasing both to "a signal". Contributes one
    /// input arity.
    Signal(ValueTy),
    /// A compile-time scalar parameter: `Double`/`Int`/`Bool`/`String`.
    Scalar,
    /// Variadic signal channels: `List (FixedBuffer elem)`. Consumes all
    /// remaining signal arguments, all with the declared element.
    VariadicSignal(ValueTy),
    /// A `Data`-typed record parameter (e.g. a mixer config).
    Record(String),
    /// A named shared buffer (`Tape elem`) — a symbolic resource reference, NOT
    /// a signal. Carries the element for the same reason [`Self::Signal`] does.
    Resource(ValueTy),
}

/// A parsed foreign function signature.
#[derive(Debug, Clone, PartialEq)]
pub struct FfiSig {
    /// Parameter descriptors in declaration order.
    pub params: Vec<FfiParam>,
    /// Number of signal output channels (result `FixedBuffer`s).
    pub signal_outs: usize,
    /// Display names of the scalar params in declaration order (host tooling
    /// addresses a built-in's scalar params by these names). Filled from the
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
    /// A `Double`-typed field.
    Double,
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
/// `with_names` calls).
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
        ("read_head", &["delay"]),
    ];
    TABLE
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, names)| names.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

/// Read a record type's scalar fields from `TypeEnv::data_types`, converting
/// `ValueTy::Double` → [`FfiScalar::Double`] and `ValueTy::Int` →
/// [`FfiScalar::Int`]. A `List`-typed field of band records (`bands: List
/// EqBand`) becomes [`FfiScalar::BandList`] carrying the band record type
/// name. Defaults come from [`foreign_record_defaults`] (the legacy
/// `RecordField::default` values). Returns `None` when the type is not a
/// declared record.
pub fn ffi_record_schema(env: &TypeEnv, type_name: &str) -> Option<FfiRecordSchema> {
    let fields = match env.data_types.get(type_name)? {
        DataInfo::Record(fields) => fields,
        DataInfo::Sum(_) | DataInfo::Opaque => return None,
    };
    let mut out = Vec::new();
    for (name, vty, _) in fields {
        let default = foreign_record_defaults(type_name, name.as_str());
        match vty {
            ValueTy::Double => out.push((name.clone(), FfiScalar::Double, default)),
            ValueTy::Int => out.push((name.clone(), FfiScalar::Int, default)),
            // `Integer` has no host scalar representation: a bignum record
            // field cannot cross the FFI boundary as an f32/i64 parameter.
            ValueTy::Integer => {}
            // `bands: List EqBand` — a sequence of band records, each flattened
            // in the band schema's field order at lowering.
            ValueTy::Con(head, args)
                if head == "List"
                    && args.len() == 1
                    && matches!(&args[0], ValueTy::Con(_band_ty, _)) =>
            {
                let ValueTy::Con(band_ty, _) = &args[0] else {
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
/// - `FixedBuffer elem` → [`FfiParam::Signal(elem)`]
/// - a name the registry marks as an FFI scalar (`Double`/`Int`/`Bool`/`String`)
///   → [`FfiParam::Scalar`] — the REGISTRY decides, not a list here
/// - `List (FixedBuffer elem)` → [`FfiParam::VariadicSignal(elem)`]
/// - `Tape elem` → [`FfiParam::Resource(elem)`]
/// - any other `TName` (a `Data` record type) → [`FfiParam::Record(name)`]
///
/// A buffer parameter's ELEMENT is resolved through the registry and CHECKED:
/// an element the registry does not mark as a buffer element
/// ([`TypeEnv::is_buffer_element`]) makes the parameter a wrong element, so the
/// signature converts to `None` and the declaration is rejected by the same
/// walk that rejects unknown names, with a `wrong buffer element` diagnostic
/// naming the requirement. `FixedBuffer Double` and `FixedBuffer Int`
/// therefore classify differently instead of both becoming "a signal".
///
/// The result type is one or more `FixedBuffer` channels (`FixedBuffer elem` →
/// 1 out; `(FixedBuffer elem, FixedBuffer elem)` → 2 outs). Returns `None` for a
/// malformed signature (non-carried, unsupported result, or a wrong element).
pub fn ffi_sig_from_typeexpr(env: &TypeEnv, name: &str, te: &TypeExpr) -> Option<FfiSig> {
    // Unroll the carried arrows into a flat param list.
    let mut params = Vec::new();
    let mut ret = te;
    loop {
        match ret {
            TypeExpr::TFunc(args, r) => {
                // The parser's carried arrows are flat (`a -> b -> c -> r` →
                // `TFunc([a,b,c], r)`); a nested one-arg TFunc also unrolls here.
                for a in args {
                    params.push(param_from_typeexpr_env(env, a)?);
                }
                ret = r;
            }
            other => {
                // A VariadicSignal that is not the LAST param is ambiguous —
                // lowering would fold the trailing call args (a trailing scalar)
                // as signals while inference counts a different arity. EXCEPT a
                // trailing RECORD param: the record is the config scalar and the
                // variadic's signal span stops before it (mixer's `List
                // (FixedBuffer Double) -> MixerConfig`). `[Signal, VariadicSignal,
                // Scalar]` stays invalid.
                if let Some(i) = params
                    .iter()
                    .rposition(|p| matches!(p, FfiParam::VariadicSignal(_)))
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
                    signal_outs: outs_from_typeexpr(env, other)?,
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

fn param_from_typeexpr(env: &TypeEnv, te: &TypeExpr) -> Option<FfiParam> {
    // A synonym is TRANSPARENT: `type B = FixedBuffer Double` names a signal
    // channel exactly as the target does, so the SHAPE test below runs on the
    // expansion.
    let expanded = env.expand_alias_expr(te);
    let te = &expanded;
    match te {
        // `FixedBuffer elem` — a signal channel, carrying its element.
        TypeExpr::TApp(head, args)
            if env.buffer_role(head) == Some(BufferRole::Channel) && args.len() == 1 =>
        {
            Some(FfiParam::Signal(buffer_element(env, &args[0])?))
        }
        TypeExpr::TApp(head, args) if head == "List" && args.len() == 1 => {
            // `List (FixedBuffer elem)` — variadic signal channels, all with
            // the declared element.
            if let TypeExpr::TApp(h, a) = &args[0] {
                if env.buffer_role(h) == Some(BufferRole::Channel) && a.len() == 1 {
                    return Some(FfiParam::VariadicSignal(buffer_element(env, &a[0])?));
                }
            }
            None
        }
        // `Tape elem` — a named shared buffer (resource), not a signal.
        TypeExpr::TApp(head, args)
            if env.buffer_role(head) == Some(BufferRole::Resource) && args.len() == 1 =>
        {
            Some(FfiParam::Resource(buffer_element(env, &args[0])?))
        }
        TypeExpr::TName(n) => Some(FfiParam::Record(n.clone())),
        _ => None,
    }
}

/// The buffer ELEMENT a written element type-expression declares, checked
/// against what the host can actually produce.
///
/// `Some(elem)` when `te` names an element the host materialises
/// ([`TypeEnv::is_buffer_element_ty`] — `Double` today); `None` for anything
/// else, which makes the enclosing signature conversion fail. The check is
/// data-driven: it asks the registry, so a second supported element is one seed
/// edit, not a new list here.
fn buffer_element(env: &TypeEnv, te: &TypeExpr) -> Option<ValueTy> {
    let TypeExpr::TName(n) = te else {
        return None;
    };
    let elem = env.vty_of_name(n)?;
    if !env.is_buffer_element_ty(&elem) {
        return None;
    }
    Some(elem)
}

/// [`param_from_typeexpr`] with the type registry in hand: a `TName` that the
/// registry marks as an FFI scalar (`Double`/`Int`/`Bool`/`String`) is a SCALAR
/// parameter; any other name is a record type. The registry decides — no
/// private scalar-name list lives here.
fn param_from_typeexpr_env(env: &TypeEnv, te: &TypeExpr) -> Option<FfiParam> {
    // A synonym is TRANSPARENT before the scalar test too: `type D = Double`
    // names the scalar `Double`, and `type B = FixedBuffer Double` names a
    // signal channel — neither is an opaque record.
    let expanded = env.expand_alias_expr(te);
    let te = &expanded;
    match te {
        TypeExpr::TName(n) if env.is_ffi_scalar(n) => Some(FfiParam::Scalar),
        _ => param_from_typeexpr(env, te),
    }
}

fn outs_from_typeexpr(env: &TypeEnv, te: &TypeExpr) -> Option<usize> {
    // A synonym is TRANSPARENT (see [`param_from_typeexpr`]).
    let expanded = env.expand_alias_expr(te);
    let te = &expanded;
    match te {
        TypeExpr::TApp(head, args)
            if env.buffer_role(head) == Some(BufferRole::Channel) && args.len() == 1 =>
        {
            buffer_element(env, &args[0])?;
            Some(1)
        }
        // `(FixedBuffer a, FixedBuffer b)` desugars to `Pair (FixedBuffer a) (FixedBuffer b)`.
        TypeExpr::TApp(head, args) if head == "Pair" && args.len() == 2 => {
            let all_channels = args.iter().all(|a| match a {
                TypeExpr::TApp(h, x) => {
                    env.buffer_role(h) == Some(BufferRole::Channel)
                        && x.len() == 1
                        && buffer_element(env, &x[0]).is_some()
                }
                _ => false,
            });
            if all_channels {
                Some(2)
            } else {
                None
            }
        }
        _ => None,
    }
}
