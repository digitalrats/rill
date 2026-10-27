//! The buffer ELEMENT is real and checked: `FixedBuffer elem` carries `elem`,
//! two buffers unify only when their elements unify, and the one legal element
//! today is the registry's marked buffer element (`Double`).
//!
//! Haskell 2010 is the standard: `FixedBuffer Int` and `FixedBuffer Bool` are
//! DIFFERENT types (§4.1.2 — a type constructor is applied to its arguments), so
//! a function claiming one cannot return the other. Before this task the element
//! was discarded: `g :: FixedBuffer Int -> FixedBuffer Bool; g x = x` compiled
//! and ran, which no Haskell compiler accepts.

pub mod common;

use common::*;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::register;
use rill_lang::types::ffi::{ffi_sig_from_typeexpr, FfiParam};
use rill_lang::types::ty::{BufferRole, TypeEnv, ValueTy};

/// Compile + run a whole-buffer program and return the run's samples.
fn run_buffer(src: &str) -> Vec<f32> {
    let (defs, expr) = split_main(src);
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    let prog = eval_ffi(defs, expr, ffi)
        .unwrap_or_else(|e| panic!("program must compile and run: {e}\nsource:\n{src}"));
    let mut out = [0.0f32; 256];
    assert!(
        buffers(&prog, &mut out),
        "expected a whole-buffer result, got {:?}",
        out_value(&prog)
    );
    out.to_vec()
}

/// Split a one-expression program at its `main = ` binding, the shape the
/// shared helpers take.
fn split_main(src: &str) -> (&str, &str) {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    (defs, expr.trim().trim_end_matches(';'))
}

fn compile_err(src: &str) -> String {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    match eval_ffi(split_main(src).0, split_main(src).1, ffi) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected a compile error, got a successful run:\n{src}"),
    }
}

// ---------------------------------------------------------------------------
// 1. The element is carried and checked: two different elements are two types.
// ---------------------------------------------------------------------------

#[test]
fn buffers_with_different_elements_are_different_types() {
    // The reported defect, verbatim: `g :: FixedBuffer Int -> FixedBuffer Bool`
    // returning its argument must NOT compile.
    let err = compile_err("g :: FixedBuffer Int -> FixedBuffer Bool;\ng x = x;\nmain = g 1;");
    assert!(
        err.contains("String") || err.contains("Bool") || err.contains("type"),
        "a buffer with a different element must be a type error, got: {err}"
    );

    // The same at the `ValueTy` level, directly: the two buffer types are not
    // equal, and neither unifies with the other.
    let int_buf = ValueTy::Buffer(Box::new(ValueTy::Int));
    let bool_buf = ValueTy::Buffer(Box::new(ValueTy::bool_ty()));
    assert_ne!(int_buf, bool_buf);
    let mut subst = rill_lang::types::Subst::default();
    assert!(
        rill_lang::types::unify::unify_value(
            &int_buf,
            &bool_buf,
            &mut subst,
            rill_lang::error::Span::new(0, 0)
        )
        .is_err(),
        "`FixedBuffer Int` must not unify with `FixedBuffer Bool`"
    );
    // ... while the SAME element does unify, and a variable element solves.
    assert!(
        rill_lang::types::unify::unify_value(
            &int_buf,
            &ValueTy::Buffer(Box::new(ValueTy::Int)),
            &mut subst,
            rill_lang::error::Span::new(0, 0)
        )
        .is_ok(),
        "identical buffer types must unify"
    );
    let var = ValueTy::Var(99);
    rill_lang::types::unify::unify_value(
        &ValueTy::Buffer(Box::new(var.clone())),
        &ValueTy::Buffer(Box::new(ValueTy::Double)),
        &mut subst,
        rill_lang::error::Span::new(0, 0),
    )
    .expect("an element variable must unify with the concrete element");
    assert_eq!(
        subst.resolve_value(&var),
        ValueTy::Double,
        "the element variable must be solved by unification"
    );
}

#[test]
fn a_buffer_never_unifies_with_a_non_buffer() {
    let mut subst = rill_lang::types::Subst::default();
    for other in [
        ValueTy::Int,
        ValueTy::Double,
        ValueTy::bool_ty(),
        ValueTy::Con("FixedBuffer".into(), vec![]),
        ValueTy::Con("List".into(), vec![ValueTy::Double]),
    ] {
        assert!(
            rill_lang::types::unify::unify_value(
                &ValueTy::Buffer(Box::new(ValueTy::Double)),
                &other,
                &mut subst,
                rill_lang::error::Span::new(0, 0)
            )
            .is_err(),
            "a buffer must not unify with {other:?}"
        );
    }
    // In particular there is no second `App("FixedBuffer", [])` shape: the
    // element-free application is NOT the buffer type.
    assert_ne!(
        ValueTy::Buffer(Box::new(ValueTy::Double)),
        ValueTy::Con("FixedBuffer".into(), vec![])
    );
}

// ---------------------------------------------------------------------------
// 2. `Double` is the one legal element, and any other element names the rule.
// ---------------------------------------------------------------------------

#[test]
fn wrong_buffer_element_is_rejected_and_names_the_requirement() {
    for wrong in ["Int", "Bool", "String"] {
        let src = format!("f :: FixedBuffer {wrong} -> FixedBuffer {wrong}; f x = x; main = 1.0;");
        let err = compile_err(&src);
        assert!(
            err.contains("buffer element") && err.contains("Double"),
            "a `FixedBuffer {wrong}` signature must be rejected naming the \
             requirement, got: {err}"
        );
        assert!(
            err.contains(wrong),
            "the diagnostic must name the offending element `{wrong}`, got: {err}"
        );
    }
    // A resource handle carries the element too, and is checked the same way.
    let err =
        compile_err("foreign read_head : Tape Int -> Double -> FixedBuffer Double;\nmain = 1.0;");
    assert!(
        err.contains("buffer element") && err.contains("Double"),
        "a `Tape Int` declaration must be rejected naming the requirement, got: {err}"
    );
}

#[test]
fn the_legal_element_check_is_data_driven() {
    // The rule is a property of the registry ENTRY, not a name list in the
    // checker: `is_buffer_element` answers for exactly the marked scalar(s).
    let env = TypeEnv::with_builtins();
    assert!(env.is_buffer_element("Double"), "`Double` is the element");
    for other in ["Int", "Bool", "String", "()", "Buffer", "FixedBuffer"] {
        assert!(
            !env.is_buffer_element(other),
            "`{other}` must not be a buffer element"
        );
    }
    assert_eq!(env.buffer_element_names(), vec!["Double".to_string()]);
    assert_eq!(
        env.buffer_vty(),
        ValueTy::Buffer(Box::new(ValueTy::Double)),
        "a buffer value's type carries the registered element"
    );
    // `Buffer` itself is registered as a type NAME for the buffer value type,
    // with its element — so the registry has one place that says what a buffer
    // type IS.
    assert_eq!(
        env.vty_of_name("Buffer"),
        Some(ValueTy::Buffer(Box::new(ValueTy::Double)))
    );
}

// ---------------------------------------------------------------------------
// 3. `FixedBuffer Double` works end to end: build, read the block, arithmetic.
// ---------------------------------------------------------------------------

#[test]
fn a_double_buffer_works_end_to_end() {
    // Build a buffer (a generator), read the block back, and confirm the samples
    // are real values — the type is `FixedBuffer Double` throughout.
    let samples = run_buffer("main = sine 440.0 0.5 0.0;");
    assert!(
        samples.iter().any(|s| s.abs() > 1e-6),
        "a `FixedBuffer Double` generator must produce non-silent samples"
    );
    assert!(
        samples.iter().all(|s| s.is_finite()),
        "all samples must be finite"
    );

    // Elementwise arithmetic over the buffer: `b * 2.0` doubles every sample, so
    // the two blocks differ by exactly that factor wherever it is audible.
    let scaled = run_buffer("main = sine 440.0 0.5 0.0 * 2.0;");
    for (a, b) in samples.iter().zip(scaled.iter()) {
        assert!(
            (b - a * 2.0).abs() < 1e-5,
            "elementwise `* 2.0` must scale every sample: {a} -> {b}"
        );
    }

    // Summing two buffers is elementwise too; `b + b` is `2 * b`.
    let summed = run_buffer("main = sine 440.0 0.5 0.0 + sine 440.0 0.5 0.0;");
    for (a, b) in samples.iter().zip(summed.iter()) {
        assert!(
            (b - a * 2.0).abs() < 1e-5,
            "elementwise `+` must add the samples: {a} + {a} != {b}"
        );
    }
}

#[test]
fn a_buffer_flows_through_a_typed_annotation() {
    // A signature that names the legal element is accepted, and the value keeps
    // its `FixedBuffer Double` type at the call.
    let samples = run_buffer(
        "lerp :: FixedBuffer Double -> FixedBuffer Double -> FixedBuffer Double;\n\
         lerp a b = a * 0.25 + b * 0.75;\n\
         main = lerp (sine 440.0 1.0 0.0) (sine 440.0 0.0 0.0);",
    );
    assert!(samples.iter().all(|s| s.is_finite()));
}

// ---------------------------------------------------------------------------
// 4. The FFI layer carries the element and distinguishes elements.
// ---------------------------------------------------------------------------

#[test]
fn ffi_distinguishes_buffer_elements() {
    use rill_lang::ast::TypeExpr;
    let env = TypeEnv::with_builtins();
    let buf = |n: &str| TypeExpr::TApp("FixedBuffer".into(), vec![TypeExpr::TName(n.into())]);
    let sig = |param: TypeExpr| {
        ffi_sig_from_typeexpr(
            &env,
            "probe",
            &TypeExpr::TFunc(vec![param], Box::new(buf("Double"))),
        )
    };

    // The declared element is RECORDED, not erased: `FixedBuffer Double` and a
    // would-be `FixedBuffer Int` do not classify identically.
    let ok = sig(buf("Double")).expect("a `FixedBuffer Double` signature is valid");
    assert_eq!(ok.params, vec![FfiParam::Signal(ValueTy::Double)]);

    // An element the language does not have makes the signature invalid — the
    // FFI layer cannot accept a channel whose element the host cannot produce.
    assert!(
        sig(buf("Int")).is_none(),
        "`FixedBuffer Int` must not convert to an FFI signature"
    );
    assert!(
        sig(buf("Bool")).is_none(),
        "`FixedBuffer Bool` must not convert to an FFI signature"
    );

    // A RESULT with a wrong element is rejected too, and so is a resource.
    assert!(
        ffi_sig_from_typeexpr(
            &env,
            "probe",
            &TypeExpr::TFunc(vec![], Box::new(buf("Int")))
        )
        .is_none(),
        "a wrong-element RESULT must be rejected"
    );
    let tape = TypeExpr::TApp("Tape".into(), vec![TypeExpr::TName("Int".into())]);
    assert!(
        ffi_sig_from_typeexpr(
            &env,
            "probe",
            &TypeExpr::TFunc(vec![tape], Box::new(buf("Double")))
        )
        .is_none(),
        "a wrong-element `Tape` must be rejected"
    );
    let good_tape = TypeExpr::TApp("Tape".into(), vec![TypeExpr::TName("Double".into())]);
    let resource = ffi_sig_from_typeexpr(
        &env,
        "probe",
        &TypeExpr::TFunc(vec![good_tape], Box::new(buf("Double"))),
    )
    .expect("a `Tape Double` signature is valid");
    assert_eq!(
        resource.params,
        vec![FfiParam::Resource(ValueTy::Double)],
        "a resource distinguishes its element"
    );

    // The roles themselves are registry data, not a list in the FFI layer.
    assert_eq!(env.buffer_role("FixedBuffer"), Some(BufferRole::Channel));
    assert_eq!(env.buffer_role("Tape"), Some(BufferRole::Resource));
    assert_eq!(env.buffer_role("List"), None);
}

#[test]
fn the_prelude_instances_name_the_legal_element() {
    // The prelude's `Num`/`Buffer` instances use MONOMORPHIC instance heads
    // naming `Double` — legal Haskell 2010 §4.3.1 heads that need no dictionary
    // builder. A variable-headed head would advertise an instance for every
    // element while the element is checked.
    let env = TypeEnv::with_builtins();
    for class in ["Num", "Buffer"] {
        let by_ty = env.instances.get(class).expect("class registered");
        assert!(
            by_ty.contains_key("FixedBuffer Double"),
            "`instance {class} (FixedBuffer Double)` must register"
        );
        assert!(
            by_ty.contains_key("Tape Double"),
            "`instance {class} (Tape Double)` must register"
        );
        assert!(
            !by_ty.contains_key("FixedBuffer"),
            "no variable-headed `instance {class} (FixedBuffer a)` key may exist"
        );
    }
}

#[test]
fn a_wrong_element_is_rejected_in_data_fields_and_annotations() {
    // The element is checked EVERYWHERE a type expression can appear, not only in
    // a definition signature.
    let err = compile_err("data B = { f: FixedBuffer Int };\nmain = 1.0;");
    assert!(
        err.contains("buffer element") && err.contains("Double"),
        "a data field with a wrong element must be rejected, got: {err}"
    );

    let err = compile_err("main = (sine 440.0 0.5 0.0) :: FixedBuffer Bool;");
    assert!(
        err.contains("buffer element") && err.contains("Double"),
        "an expression annotation with a wrong element must be rejected, got: {err}"
    );

    // A TYPE-VARIABLE element is a polymorphic signature, not a concrete claim,
    // so it is accepted — that is an ordinary Haskell signature, and a call site
    // checks the element it actually passes.
    let samples = run_buffer("id_buf :: FixedBuffer a -> FixedBuffer a;\nid_buf x = x;\nmain = id_buf (sine 440.0 0.5 0.0);");
    assert!(samples.iter().all(|s| s.is_finite()));
}

#[test]
fn a_polymorphic_buffer_element_is_freshened_per_instantiation() {
    // A quantified element is part of the SCHEME: two instantiations must get
    // DISTINCT element variables, or the second call site would silently reuse
    // (and overwrite) the first one's element. This is what a walker that
    // forgets the element payload breaks.
    let samples = run_buffer(
        "idb :: FixedBuffer a -> FixedBuffer a;\n\
         idb x = x;\n\
         pick b = idb b;\n\
         main = pick (sine 440.0 0.5 0.0);",
    );
    assert!(samples.iter().all(|s| s.is_finite()));

    // Two calls in one expression, both through the polymorphic identity, still
    // produce one well-typed buffer.
    let samples = run_buffer(
        "idb :: FixedBuffer a -> FixedBuffer a;\n\
         idb x = x;\n\
         main = idb (sine 440.0 0.5 0.0) * 0.5 + idb (sine 440.0 0.5 0.0) * 0.5;",
    );
    assert!(samples.iter().all(|s| s.is_finite()));
}

#[test]
fn a_foreign_declaration_with_a_wrong_element_reports_the_requirement() {
    // A user-written `foreign` declaration goes through the same name/element
    // validation as any other signature, so the diagnostic is the compiled one —
    // it names the offending element AND the requirement.
    let err = compile_err("foreign bad : FixedBuffer Int -> FixedBuffer Double;\nmain = 1.0;");
    assert!(
        err.contains("wrong buffer element") && err.contains("`Int`") && err.contains("`Double`"),
        "the foreign diagnostic must name the element and the requirement, got: {err}"
    );
}
