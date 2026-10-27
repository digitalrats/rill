//! Haskell 2010 §3.7 built-in type syntax: `[t]` for `List t`, `(t1, t2)` for
//! `Pair t1 t2`, and `[]` for the bare list type constructor.
//!
//! The syntax is DESUGARED at parse time into the nominal application the rest
//! of the compiler already understands — `[t]` → `TApp("List", [t])`, `[]` →
//! `TName("List")` — so there is exactly one type representation per concept.
//! These tests therefore assert BOTH halves: the parsed shape (equality with
//! the nominal spelling's tree) and the behaviour that follows from it (kind
//! checking, FFI classification, diagnostics).

pub mod common;

use common::*;
use rill_lang::ast::TypeExpr;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::lexer::tokenize;
use rill_lang::mode::Mode;
use rill_lang::parser::parse;
use rill_lang::render::render;
use rill_lang::types::ffi::{ffi_sig_from_typeexpr, FfiParam};
use rill_lang::types::ty::{TypeEnv, ValueTy};

/// Parse `src` and return the `TypeExpr` of definition `name`'s signature.
fn sig_of(src: &str, name: &str) -> TypeExpr {
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes()).unwrap();
    let def = prog
        .defs
        .iter()
        .find(|d| d.name() == name)
        .unwrap_or_else(|| panic!("no definition named `{name}` in: {src}"));
    match def {
        rill_lang::ast::Def::Foreign { sig, .. } => sig.clone(),
        other => other
            .sig()
            .unwrap_or_else(|| panic!("`{name}` has no signature in: {src}"))
            .clone(),
    }
}

/// The error text of a compile attempt, or `Ok`.
fn try_compile(src: &str) -> Result<(), String> {
    match rill_lang::compile_eval::<f32, 256>(src, "main", Mode::Strict) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("{e}")),
    }
}

// ---------------------------------------------------------------------------
// Step 1: `[t]` desugars to `TApp("List", [t])` — the SAME tree `List t` gives.
// ---------------------------------------------------------------------------

#[test]
fn list_syntax_desugars_to_the_nominal_constructor() {
    // The representation, not just "it parses": a parallel `TList` node would
    // make this equality fail while every end-to-end test still passed.
    assert_eq!(
        sig_of("f :: [Int] -> Int; f x = 1; main = 1.0;", "f"),
        sig_of("f :: List Int -> Int; f x = 1; main = 1.0;", "f"),
        "`[Int]` and `List Int` must parse to the same TypeExpr"
    );
    assert_eq!(
        sig_of("f :: [Int] -> Int; f x = 1; main = 1.0;", "f"),
        TypeExpr::TFunc(
            vec![TypeExpr::TApp(
                "List".into(),
                vec![TypeExpr::TName("Int".into())]
            )],
            Box::new(TypeExpr::TName("Int".into()))
        )
    );
}

#[test]
fn tuple_syntax_still_desugars_to_pair() {
    // The sibling rule (Haskell 2010 `(t1, t2)`) was already in place; it is
    // pinned here so both sugar forms are proved against the same target.
    assert_eq!(
        sig_of("f :: (Int, Double) -> Int; f x = 1; main = 1.0;", "f"),
        sig_of("f :: Pair Int Double -> Int; f x = 1; main = 1.0;", "f")
    );
}

#[test]
fn nested_forms_fall_out_of_one_rule() {
    // No special case for nesting: `[[Int]]`, `[Maybe Int]`, `(Int, [Double])`
    // and `[FixedBuffer Double]` all come from `[` + one type expression + `]`.
    assert_eq!(
        sig_of("f :: [[Int]] -> Int; f x = 1; main = 1.0;", "f"),
        sig_of("f :: List (List Int) -> Int; f x = 1; main = 1.0;", "f")
    );
    assert_eq!(
        sig_of("f :: [Maybe Int] -> Int; f x = 1; main = 1.0;", "f"),
        sig_of("f :: List (Maybe Int) -> Int; f x = 1; main = 1.0;", "f")
    );
    assert_eq!(
        sig_of("f :: (Int, [Double]) -> Int; f x = 1; main = 1.0;", "f"),
        sig_of(
            "f :: Pair Int (List Double) -> Int; f x = 1; main = 1.0;",
            "f"
        )
    );
    assert_eq!(
        sig_of(
            "f :: [FixedBuffer Double] -> Int; f x = 1; main = 1.0;",
            "f"
        ),
        sig_of(
            "f :: List (FixedBuffer Double) -> Int; f x = 1; main = 1.0;",
            "f"
        )
    );
}

#[test]
fn list_syntax_is_an_atom_in_every_type_position() {
    // A bracketed type is a normal atom, so it works as an application
    // ARGUMENT (`Maybe [Int]`) and as a function result — not only as a whole
    // argument type.
    assert_eq!(
        sig_of("f :: Maybe [Int] -> Int; f x = 1; main = 1.0;", "f"),
        sig_of("f :: Maybe (List Int) -> Int; f x = 1; main = 1.0;", "f")
    );
    // A data field and a data payload are type positions too.
    assert_eq!(
        sig_of(
            "data H = { h: [Int] }; g :: H -> Int; g x = 1; main = 1.0;",
            "g"
        ),
        TypeExpr::TFunc(
            vec![TypeExpr::TName("H".into())],
            Box::new(TypeExpr::TName("Int".into()))
        )
    );
    assert!(
        try_compile("data H = { h: [Maybe [Int]] }; main = 1.0;").is_ok(),
        "a nested list field type must compile"
    );
    assert!(
        try_compile("data S = Mk [Int]; main = 1.0;").is_ok(),
        "a list constructor payload must compile"
    );
    assert!(
        try_compile("data S = Mk (Int, Double); main = 1.0;").is_ok(),
        "a tuple constructor payload must compile"
    );
}

// ---------------------------------------------------------------------------
// Step 1 continued: the acceptance rows.
// ---------------------------------------------------------------------------

#[test]
fn the_acceptance_signatures_type_check() {
    for src in [
        "f :: [Int] -> Int; f x = 1; main = 1.0;",
        "f :: [[Int]] -> Int; f x = 1; main = 1.0;",
        "f :: [Maybe Int] -> Int; f x = 1; main = 1.0;",
        "f :: (Int, [Double]) -> Int; f x = 1; main = 1.0;",
        // A list of buffers: `FixedBuffer Double` has kind `*`, so `[t]` over it
        // is well kinded.
        "f :: [FixedBuffer Double] -> Int; f x = 1; main = 1.0;",
        // The list functions themselves are expressible now.
        "f :: [Double] -> [Double]; f xs = xs; main = 1.0;",
    ] {
        assert!(
            try_compile(src).is_ok(),
            "Haskell 2010 signature must compile: {src}\n got: {:?}",
            try_compile(src)
        );
    }
}

#[test]
fn a_list_signature_runs_end_to_end() {
    // Not only a signature: a value flows through the list-typed parameter and
    // the list-typed result, so the desugared type is really in effect.
    let src = r#"
        len :: [Double] -> Int
        len xs = length xs
        main = len [1.0, 2.0, 3.0]
    "#;
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ForeignRegistry::<f32>::new())
        .unwrap_or_else(|e| panic!("a list-typed program must compile and run: {e}"));
    let v = prog.result().expect("the run must produce a single result");
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(3)
    );
}

// ---------------------------------------------------------------------------
// Step 2: `[]` is the bare list CONSTRUCTOR — K1 rejects it in a `*` position.
// ---------------------------------------------------------------------------

#[test]
fn bare_brackets_parse_as_the_list_constructor() {
    assert_eq!(
        sig_of("f :: [] -> Int; f x = 1; main = 1.0;", "f"),
        TypeExpr::TFunc(
            vec![TypeExpr::TName("List".into())],
            Box::new(TypeExpr::TName("Int".into()))
        ),
        "`[]` must be the bare constructor `TName(\"List\")`"
    );
}

#[test]
fn bare_brackets_in_a_type_position_is_a_kind_error_not_a_parse_error() {
    // GHC: `Expecting one more argument to '[]'; Expected a type, but '[]' has
    // kind '* -> *'`. The wording comes from K1's single kind-error function;
    // what this test pins is that the failure is a KIND error (not a parse
    // error) and that the missing-argument count comes from the kind.
    match try_compile("f :: [] -> Int; f x = 1; main = 1.0;") {
        Ok(()) => panic!("a bare list constructor in a `*` position must be rejected"),
        Err(msg) => {
            assert!(
                msg.contains("has kind `* -> *`"),
                "expected a KIND diagnostic (not a parse error), got: {msg}"
            );
            assert!(
                msg.contains("1 more argument"),
                "the diagnostic must name the missing argument count: {msg}"
            );
        }
    }
    // At kind `* -> *` the bare constructor is exactly what is wanted:
    // `instance Functor []` in GHC terms.
    let env = TypeEnv::with_builtins();
    env.check_instance_head(
        "Functor",
        &TypeExpr::TName("List".into()),
        rill_lang::error::Span::new(0, 0),
    )
    .expect("`instance Functor []` must kind-check at kind `* -> *`");
}

#[test]
fn bracket_sugar_kind_checks_like_its_nominal_form() {
    // An over-application is still an over-application: `[Int Double]` cannot be
    // spelled, but the kind walk must see `[FixedBuffer]` as a `* -> *` element.
    match try_compile("f :: [FixedBuffer] -> Int; f x = 1; main = 1.0;") {
        Ok(()) => panic!("`[FixedBuffer]` has a `* -> *` element and must be rejected"),
        Err(msg) => assert!(
            msg.contains("has kind `* -> *`"),
            "the element's kind must be reported: {msg}"
        ),
    }
}

// ---------------------------------------------------------------------------
// Step 3: the renderer round-trips the source spelling.
// ---------------------------------------------------------------------------

#[test]
fn renderer_prints_the_source_spelling() {
    // The renderer takes a DESUGARED program (a standalone `Def::Signature` is
    // rejected by design), so the definitions are written in the signature +
    // equation form and rendered as a whole.
    let src = "f a :: [Int] -> [Maybe [Double]] -> (Int, [Bool]) -> [FixedBuffer Double] = a;\nmain = 1.0;";
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes()).unwrap();
    let out = render(&prog).unwrap();
    assert!(
        out.contains(
            "f a :: ([Int] -> [Maybe [Double]] -> (Int, [Bool]) -> [FixedBuffer Double]) = a"
        ),
        "a list/tuple type must render in its source spelling: {out}"
    );
    // And rendering is a fixed point: the rendered text re-parses and renders
    // identically (the sugar is re-read as the same `TApp`).
    let toks2 = tokenize(&out).unwrap();
    let reparsed = parse(&toks2, out.as_bytes()).unwrap();
    assert_eq!(render(&reparsed).unwrap(), out);
}

#[test]
fn renderer_keeps_the_nominal_spelling_for_a_partial_application() {
    // `Pair Int` / `List` are not fully applied, so they have no sugar
    // spelling; the nominal name is the only form that names them (and it is
    // what a kind diagnostic must quote).
    let src = "f a b :: Pair Int -> List -> Int = 1;\nmain = 1.0;";
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes()).unwrap();
    let out = render(&prog).unwrap();
    assert!(
        out.contains("(Pair Int -> List -> Int)"),
        "a partial application keeps its nominal spelling: {out}"
    );
}

#[test]
fn diagnostics_quote_the_source_spelling() {
    // An argument-type mismatch right inside a list must quote `[Int]`, not the
    // internal `List Int`.
    let msg = match try_compile("f :: [Int] -> Int; f xs = 1; main = f [\"nope\"];") {
        Ok(()) => panic!("a String list must not satisfy a `[Int]` parameter"),
        Err(msg) => msg,
    };
    assert!(
        msg.contains("[Int]"),
        "the diagnostic must spell the list type `[Int]`: {msg}"
    );
    assert!(
        !msg.contains("List Int"),
        "the internal `List Int` spelling must not leak into a diagnostic: {msg}"
    );

    // The same for a tuple.
    let msg = match try_compile("f :: (Int, Double) -> Int; f p = 1; main = f (true, 1.0);") {
        Ok(()) => panic!("a Bool first field must not satisfy an `Int`"),
        Err(msg) => msg,
    };
    assert!(
        msg.contains("(Int, Double)") && !msg.contains("Pair Int Double"),
        "the diagnostic must spell the tuple type `(Int, Double)`: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Step 4: FFI classification is identical for `[t]` and `List t`.
// ---------------------------------------------------------------------------

#[test]
fn an_ffi_signature_classifies_the_same_with_bracket_syntax() {
    // Both spellings are the SAME TypeExpr, so the classifier cannot tell them
    // apart — this is the structural proof, and the assertions below show what
    // it actually classifies to.
    let nominal = sig_of(
        "foreign merge : List (FixedBuffer Double) -> FixedBuffer Double; main = 1.0;",
        "merge",
    );
    let bracket = sig_of(
        "foreign merge : [FixedBuffer Double] -> FixedBuffer Double; main = 1.0;",
        "merge",
    );
    assert_eq!(
        bracket, nominal,
        "`[FixedBuffer Double]` and `List (FixedBuffer Double)` must be the same tree"
    );

    let env = TypeEnv::with_builtins();
    let sig = ffi_sig_from_typeexpr(&env, "merge", &bracket)
        .expect("a variadic-signal signature classifies");
    assert_eq!(
        sig.params,
        vec![FfiParam::VariadicSignal(ValueTy::Double)],
        "`[FixedBuffer Double]` must classify as a VARIADIC signal channel"
    );
    assert_eq!(sig.signal_outs, 1);

    // A non-terminal variadic is rejected for BOTH spellings — the classifier's
    // arity rule, not the syntax, decides.
    let terminal_rule_is_spelling_independent =
        |te: &rill_lang::ast::TypeExpr| ffi_sig_from_typeexpr(&env, "bad", te);
    let nominal_bad = sig_of(
        "foreign bad : List (FixedBuffer Double) -> Double -> FixedBuffer Double; main = 1.0;",
        "bad",
    );
    let bracket_bad = sig_of(
        "foreign bad : [FixedBuffer Double] -> Double -> FixedBuffer Double; main = 1.0;",
        "bad",
    );
    assert_eq!(bracket_bad, nominal_bad);
    assert_eq!(terminal_rule_is_spelling_independent(&nominal_bad), None);
    assert_eq!(terminal_rule_is_spelling_independent(&bracket_bad), None);
}

#[test]
fn an_ffi_record_schema_reads_a_bracketed_list_field() {
    // `bands: [EqBand]` must produce the same BandList schema entry the nominal
    // `bands: List EqBand` produces — the FFI side sees one shape, not two.
    use rill_lang::types::ffi::FfiScalar;

    let schema_of = |field_ty: &str| {
        let src = format!(
            "data EqBand = {{ freq: Double }}\n\
             data Cfg = {{ bands: {field_ty} }}\n\
             main = 1.0"
        );
        let toks = tokenize(&src).unwrap();
        let prog = parse(&toks, src.as_bytes()).unwrap();
        let typed = rill_lang::types::infer::infer_program(&prog, Mode::Strict)
            .expect("the record declarations must typecheck");
        rill_lang::types::ffi::ffi_record_schema(&typed.type_env, "Cfg")
            .expect("the record schema must be readable")
    };

    let nominal = schema_of("List EqBand");
    let bracket = schema_of("[EqBand]");
    assert_eq!(
        bracket, nominal,
        "`[EqBand]` and `List EqBand` must produce the same record schema"
    );
    assert!(
        matches!(&bracket.fields[0].1, FfiScalar::BandList(t) if t == "EqBand"),
        "the bracketed field must classify as a BandList of `EqBand`, got {:?}",
        bracket.fields
    );
}

#[test]
fn a_bracket_spelled_ffi_declaration_reaches_inference_identically() {
    // End to end through the front end: a `foreign` declaration written with
    // bracket syntax registers and types a variadic call exactly like the
    // nominal spelling. (Lowering a variadic in VALUE position is a separate,
    // pre-existing limitation — it rejects both spellings with the same
    // message, which is itself the equivalence being proved.)
    let with = |ty: &str| {
        format!(
            r#"
            foreign sine : Double -> FixedBuffer Double;
            foreign merge : {ty} -> FixedBuffer Double;
            main = merge (sine 440.0) (sine 220.0)
            "#
        )
    };
    let error_of = |src: &str| {
        let toks = tokenize(src).unwrap();
        let prog = parse(&toks, src.as_bytes()).unwrap();
        match rill_lang::types::infer::infer_program(&prog, Mode::Strict) {
            Ok(_) => None,
            Err(e) => Some(e.to_string()),
        }
    };
    let nominal = with("List (FixedBuffer Double)");
    let bracket = with("[FixedBuffer Double]");
    assert_eq!(
        error_of(&bracket),
        error_of(&nominal),
        "`[FixedBuffer Double]` and `List (FixedBuffer Double)` must type identically"
    );
    assert_eq!(
        error_of(&bracket),
        None,
        "a variadic FFI call must reach lowering, not fail in the type checker"
    );

    // And the declaration is registered under the same name with the same
    // parameter descriptors, so the factory-facing contract is unchanged.
    let env_of = |src: &str| {
        let toks = tokenize(src).unwrap();
        let prog = parse(&toks, src.as_bytes()).unwrap();
        rill_lang::types::infer::infer_program(&prog, Mode::Strict)
            .expect("the program must typecheck")
            .type_env
    };
    let env_bracket = env_of(&bracket);
    let env_nominal = env_of(&nominal);
    let describe = |env: &TypeEnv| {
        let sig = env.foreign_sigs["merge"].clone();
        ffi_sig_from_typeexpr(env, "merge", &sig).expect("the registered signature classifies")
    };
    assert_eq!(describe(&env_bracket), describe(&env_nominal));
}

// ---------------------------------------------------------------------------
// Step 5: no behaviour change for the nominal spellings.
// ---------------------------------------------------------------------------

#[test]
fn nominal_spellings_behave_exactly_as_before() {
    // Nominal spellings keep their previous OUTCOMES: saturated applications
    // compile, a bare `List` in a `*` position is still a kind error, and
    // `instance Monoid List` still kind-checks.
    for src in [
        "f :: List Int -> Int; f x = 1; main = 1.0;",
        "f :: Pair Int Double -> Int; f x = 1; main = 1.0;",
        "f :: Maybe (List Int) -> Int; f x = 1; main = 1.0;",
    ] {
        assert!(
            try_compile(src).is_ok(),
            "a well-kinded nominal signature must still compile: {src}\n got: {:?}",
            try_compile(src)
        );
    }
    for src in [
        "f :: List -> Int; f x = 1; main = 1.0;",
        "f :: Pair Int -> Int; f x = 1; main = 1.0;",
    ] {
        match try_compile(src) {
            Ok(()) => panic!("a partially applied constructor is still a kind error: {src}"),
            Err(msg) => assert!(
                msg.contains("has kind `* -> *"),
                "the kind wording must be unchanged: {msg}"
            ),
        }
    }
    // The bare-constructor instance head the prelude uses still kind-checks.
    let bare_list = TypeExpr::TName("List".into());
    let env = TypeEnv::with_builtins();
    env.check_instance_head("Monoid", &bare_list, rill_lang::error::Span::new(0, 0))
        .expect("`instance Monoid List` must still kind-check");
    // `[]` parses to exactly that same constructor, so it kind-checks
    // identically — the reason `instance Monoid []` needs no second check.
    assert_eq!(
        sig_of("f :: [] -> Int; f x = 1; main = 1.0;", "f").arg_types()[0],
        bare_list
    );
}
