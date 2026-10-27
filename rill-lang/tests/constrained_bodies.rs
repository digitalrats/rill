//! The type-system groundwork for runtime dictionaries (design §3.0/§3.1):
//! a `ValueTy::Dict` (the type of a dictionary parameter) and
//! `Scheme.constraints` (the class context a scheme requires).
//!
//! Dictionaries are the RUNTIME representation of a constraint the type system
//! has already proved satisfiable, so these tests pin the static layer only:
//! a constrained scheme records `(class, type variable)`, a dictionary unifies
//! by class identity, and a dictionary has no data interpretation (it is not a
//! nominal type, not a registry key, and has its own stable rank).

pub mod common;

use rill_lang::error::Span;
use rill_lang::lexer::tokenize;
use rill_lang::mode::Mode;
use rill_lang::parser::parse;
use rill_lang::types::infer::infer_program;
use rill_lang::types::ty::{kind_rank, value_type_key, TypeEnv, TypeVarId, ValueTy};
use rill_lang::types::unify::unify_value;
use rill_lang::types::Subst;

/// Whether `t` mentions the type-variable id `id` anywhere (a free `Var`, a
/// `TyConVar`, a `TyConApp` head, or inside a compound type's arguments).
fn mentions_var(t: &ValueTy, id: TypeVarId) -> bool {
    match t {
        ValueTy::Var(v) | ValueTy::TyConVar(v) => *v == id,
        ValueTy::TyConApp(f, args) => *f == id || args.iter().any(|a| mentions_var(a, id)),
        ValueTy::Thunk(args, rets) => {
            args.iter().any(|a| mentions_var(a, id)) || rets.iter().any(|r| mentions_var(r, id))
        }
        ValueTy::Con(_, args) | ValueTy::Newtype(_, args) => {
            args.iter().any(|a| mentions_var(a, id))
        }
        ValueTy::Int
        | ValueTy::Double
        | ValueTy::Integer
        | ValueTy::String
        | ValueTy::Void
        | ValueTy::Dict(_) => false,
        // The element is part of the buffer type, so a variable inside it is
        // mentioned by the type.
        ValueTy::Buffer(elem) => mentions_var(elem, id),
    }
}

#[test]
fn a_constrained_scheme_carries_its_constraint() {
    // `mk :: Ix i => Pair i i -> Int` is a class-constrained definition: its
    // scheme must carry the class context as `(class, type variable)`.
    let src = "mk p :: Ix i => Pair i i -> Int = rangeSize p;\nmain = 0.0;";
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes()).unwrap();
    let typed = infer_program(&prog, Mode::Strict).expect("a constrained signature must typecheck");

    let scheme = &typed.def_schemes["mk"];
    assert_eq!(
        scheme.constraints.len(),
        1,
        "one `Ix i =>` constraint is recorded, got {:?}",
        scheme.constraints
    );
    let (class, var) = &scheme.constraints[0];
    assert_eq!(class, "Ix", "the constraint names its class");
    // The recorded id must be a variable of the SCHEME's own type (not a
    // dangling id from a throwaway conversion): the class variable `i` appears
    // in `Pair i i` and is quantified by the scheme.
    assert!(
        scheme.vars.contains(var),
        "the constraint's variable {:?} must be quantified by the scheme (vars {:?})",
        var,
        scheme.vars
    );
    assert!(
        mentions_var(&scheme.ty.vty, *var),
        "the constraint's variable {:?} must occur in the scheme type {:?}",
        var,
        scheme.ty.vty
    );

    // An unconstrained scheme carries no constraints.
    assert!(
        typed.def_schemes["main"].constraints.is_empty(),
        "an unconstrained scheme has an empty context, got {:?}",
        typed.def_schemes["main"].constraints
    );

    // A multi-constraint context records one entry per class, in declaration
    // order, each paired with its OWN variable (two constraints must not
    // collapse onto one variable id).
    let multi =
        "two p q :: (Ix i, Ix j) => Pair i i -> Pair j j -> Int = rangeSize p;\nmain = 0.0;";
    let toks = tokenize(multi).unwrap();
    let prog = parse(&toks, multi.as_bytes()).unwrap();
    let typed =
        infer_program(&prog, Mode::Strict).expect("a two-constraint signature must typecheck");
    let scheme = &typed.def_schemes["two"];
    assert_eq!(
        scheme
            .constraints
            .iter()
            .map(|(c, _)| c.as_str())
            .collect::<Vec<_>>(),
        vec!["Ix", "Ix"],
        "both constraints are recorded, in declaration order: {:?}",
        scheme.constraints
    );
    let first = scheme.constraints[0].1;
    let second = scheme.constraints[1].1;
    assert_ne!(
        first, second,
        "each constraint keeps its own type variable ({first} vs {second})"
    );
    // `b : Pair i i` pins the first; `j` appears only in the SECOND argument,
    // so it is a distinct variable the type still mentions.
    assert!(mentions_var(&scheme.ty.vty, first));
    assert!(mentions_var(&scheme.ty.vty, second));

    // The `forall`-wrapped spelling (`forall i. Ix i => …`, Haskell 2010)
    // carries the same constraint — the context sits under the binder.
    let foralled = "mk2 p :: forall i. Ix i => Pair i i -> Int = rangeSize p;\nmain = 0.0;";
    let toks = tokenize(foralled).unwrap();
    let prog = parse(&toks, foralled.as_bytes()).unwrap();
    let typed =
        infer_program(&prog, Mode::Strict).expect("a forall-wrapped context must typecheck");
    let scheme = &typed.def_schemes["mk2"];
    assert_eq!(
        scheme
            .constraints
            .iter()
            .map(|(c, _)| c.as_str())
            .collect::<Vec<_>>(),
        vec!["Ix"],
        "the context under a forall binder is recorded: {:?}",
        scheme.constraints
    );
    assert!(
        mentions_var(&scheme.ty.vty, scheme.constraints[0].1),
        "the forall-wrapped constraint's variable occurs in the scheme type"
    );
}

#[test]
fn dict_unifies_only_with_the_same_class() {
    let sp = Span::new(0, 1);
    let num = ValueTy::Dict("Num".to_string());
    let eq = ValueTy::Dict("Eq".to_string());

    // Same class: the dictionaries are the same type.
    let mut s = Subst::default();
    unify_value(&num, &num, &mut s, sp).expect("Dict(Num) ~ Dict(Num) must succeed");
    unify_value(&num, &ValueTy::Dict("Num".to_string()), &mut s, sp)
        .expect("Dict(Num) ~ Dict(Num) must succeed across separate allocations");

    // Different classes: a `Num` dictionary is not an `Eq` dictionary.
    let mut s = Subst::default();
    let err = unify_value(&num, &eq, &mut s, sp).expect_err("Dict(Num) ~ Dict(Eq) must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("Num") && msg.contains("Eq"),
        "the mismatch must name both classes, got: {msg}"
    );

    // A dictionary never unifies with a data type, even one whose name matches
    // the class name.
    for data in [
        ValueTy::Int,
        ValueTy::Con("Num".to_string(), vec![]),
        ValueTy::Con("Num".to_string(), vec![ValueTy::Int]),
        ValueTy::Newtype("Num".to_string(), vec![]),
    ] {
        let mut s = Subst::default();
        assert!(
            unify_value(&num, &data, &mut s, sp).is_err(),
            "Dict(Num) must not unify with {data:?}"
        );
        let mut s = Subst::default();
        assert!(
            unify_value(&data, &num, &mut s, sp).is_err(),
            "{data:?} must not unify with Dict(Num)"
        );
    }

    // A type variable binds to a dictionary (this is how a dictionary
    // parameter's type is solved later), and the binding is recorded.
    let mut s = Subst::default();
    unify_value(&ValueTy::Var(3), &num, &mut s, sp).expect("a var must bind to a dictionary");
    assert_eq!(s.resolve_value(&ValueTy::Var(3)), num);
}

#[test]
fn dict_has_no_data_interpretation() {
    let env = TypeEnv::with_builtins();
    let dict = ValueTy::Dict("Num".to_string());

    // A dictionary is not a nominal data type: no instance can be selected by
    // its "name", even though the class name is carried as the id.
    assert_eq!(
        env.type_name_of_vty(&dict),
        None,
        "a dictionary must not name a data type"
    );
    // ... while a real data type of that spelling does.
    assert_eq!(
        env.type_name_of_vty(&ValueTy::Con("Num".to_string(), vec![])),
        Some("Num".to_string()),
        "control: a nominal data type still names itself"
    );

    // A dictionary is never a registry key, so it can never select an instance.
    assert_eq!(
        value_type_key(&env, &dict),
        None,
        "a dictionary must not spell an instance-registry key"
    );
    assert_eq!(
        value_type_key(&env, &ValueTy::Int),
        Some("Int".to_string()),
        "control: a concrete type still spells its key"
    );

    // `kind_rank` is total and stable: every value type has a rank, a
    // dictionary sorts after every data form, and no data type shares its rank.
    let dict_rank = kind_rank(&dict);
    for data in [
        ValueTy::Int,
        ValueTy::Double,
        ValueTy::bool_ty(),
        ValueTy::String,
        ValueTy::Buffer(Box::new(ValueTy::Double)),
        ValueTy::Void,
        ValueTy::Con("Pair".to_string(), vec![]),
        ValueTy::Newtype("Wrap".to_string(), vec![]),
        ValueTy::Con("List".to_string(), vec![]),
        ValueTy::Thunk(vec![], vec![ValueTy::Int]),
        ValueTy::Var(0),
        ValueTy::TyConVar(0),
        ValueTy::TyConApp(0, vec![]),
    ] {
        assert!(
            kind_rank(&data) < dict_rank,
            "a dictionary must sort after every data form; {data:?} ranked {}",
            kind_rank(&data)
        );
    }
    assert_eq!(
        kind_rank(&dict),
        dict_rank,
        "the dictionary's rank is stable across calls"
    );
    // The rank orders type FORMS, not the dictionary's class: two different
    // class witnesses share a rank (a rank comparison can order, never equate).
    assert_eq!(
        kind_rank(&ValueTy::Dict("Eq".to_string())),
        dict_rank,
        "the rank is per type form, not per class"
    );
}

// ---------------------------------------------------------------------------
// Task 3: a constrained definition's body IS typed, once, under a GIVEN
// constraint (design §3.0).
//
// `mk p :: Ix i => Pair i i -> Int = rangeSize p` is the judgement
// `Ix i => Pair i i -> Int`: `i` is ASSUMED, not solved. The body is therefore
// type-checked at LOAD for every `i`, and each method call inside it resolves
// against the GIVEN (abstract) dictionary. A concrete type with no instance is
// still a COMPILE error — but at the call site that supplies it.
// ---------------------------------------------------------------------------

use common::*;

/// Infer `src`, expecting success.
fn typed(src: &str) -> rill_lang::types::infer::TypedProgram {
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes()).unwrap();
    infer_program(&prog, Mode::Strict).unwrap_or_else(|e| panic!("`{src}` must typecheck: {e}"))
}

/// Infer `src`, expecting a rejection; return the rendered message.
fn type_err(src: &str) -> String {
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes()).unwrap();
    match infer_program(&prog, Mode::Strict) {
        Ok(_) => panic!("`{src}` must be rejected at load"),
        Err(e) => e.to_string(),
    }
}

/// The evidence recorded for the selection whose class is `class`, if any.
fn selection_for(t: &rill_lang::types::infer::TypedProgram, class: &str) -> Option<ValueTy> {
    t.dict_selections
        .values()
        .find(|(c, _)| c == class)
        .map(|(_, e)| e.clone())
}

#[test]
fn a_constrained_body_is_type_checked() {
    // An ill-typed body is rejected at LOAD, with no call site in the program.
    // Before Task 3 the body was skipped entirely (`infer_def_body` returned the
    // signature's result), so this program compiled.
    let msg = type_err("mk p :: Ix i => Pair i i -> Int = \"not an Int\";\nmain = 0.0;");
    assert!(
        msg.contains("String") && msg.contains("Int"),
        "an ill-typed constrained body must be rejected naming both types, got: {msg}"
    );

    // A body naming an unknown function is rejected too — the body is really
    // walked, not merely result-checked.
    let msg = type_err("mk p :: Ix i => Pair i i -> Int = missing_helper p;\nmain = 0.0;");
    assert!(
        msg.contains("missing_helper"),
        "an unknown name in a constrained body must be rejected, got: {msg}"
    );

    // The NEGATIVE control: the same shape, well typed, is accepted — so the
    // rejections above come from the body's content, not from constrained
    // bodies being untypeable in general.
    typed("mk p :: Ix i => Pair i i -> Int = rangeSize p;\nmain = 0.0;");
}

#[test]
fn a_superclass_method_resolves_in_a_body() {
    // `f :: Ord a => a -> a -> Bool` may call `==`, which `Eq` declares:
    // `Ord`'s constraint ENTAILS `Eq`'s, so the `Ord` dictionary is the witness
    // (System FC: the superclass dictionary is a field of the subclass one).
    // Typing this needs `a` to stay ABSTRACT — the old model had no concrete
    // receiver to dispatch `==` on.
    //
    // Both spellings: `x == y` and `(==) x y` are the SAME AST (Haskell 2010
    // §3.4 — `desugar_infix_op` produces the `Ref("==")` application), so the
    // infix form must resolve against the given `Ord` witness exactly like the
    // prefix one. That was OD-13: before dictionary stage 3a the infix spelling
    // never reached method dispatch and recorded nothing.
    let t = typed("f x y :: Ord a => a -> a -> Bool = (==) x y;\nmain = 0.0;");
    assert_eq!(
        selection_for(&t, "Eq"),
        Some(ValueTy::Dict("Ord".to_string())),
        "`(==)` inside `Ord a =>` records the GIVEN `Ord` witness (the dictionary \
         that arrives), not a concrete Eq instance; selections: {:?}",
        t.dict_selections
    );

    // The body must NOT have fallen back to a scalar `Double` instance: an
    // abstract witness is recorded, which no concrete dispatch can produce.
    assert!(
        !t.dict_selections
            .values()
            .any(|(_, e)| matches!(e, ValueTy::Double | ValueTy::Int)),
        "a superclass call in a body must not dispatch to a scalar default; \
         selections: {:?}",
        t.dict_selections
    );

    // The `Eq`/`Ord` superclass relation is exercised on a second method too,
    // and the recorded evidence names the class that actually arrives.
    let t = typed("g x y :: Ord a => a -> a -> Bool = (<) x y;\nmain = 0.0;");
    assert_eq!(
        selection_for(&t, "Ord"),
        Some(ValueTy::Dict("Ord".to_string())),
        "a DIRECT method of the given class records that class; selections: {:?}",
        t.dict_selections
    );

    // An INFIX comparison records the same given witness as the prefix one —
    // the property OD-13 denied.
    for (infix_src, prefix_src, class) in [
        (
            "f x y :: Ord a => a -> a -> Bool = x == y;\nmain = 0.0;",
            "f x y :: Ord a => a -> a -> Bool = (==) x y;\nmain = 0.0;",
            "Eq",
        ),
        (
            "f x y :: Ord a => a -> a -> Bool = x < y;\nmain = 0.0;",
            "f x y :: Ord a => a -> a -> Bool = (<) x y;\nmain = 0.0;",
            "Ord",
        ),
    ] {
        let infix = typed(infix_src);
        let prefix = typed(prefix_src);
        assert_eq!(
            selection_for(&infix, class),
            Some(ValueTy::Dict("Ord".to_string())),
            "`{infix_src}` must record the GIVEN `Ord` witness for `{class}`;              selections: {:?}",
            infix.dict_selections
        );
        assert_eq!(
            selection_for(&infix, class),
            selection_for(&prefix, class),
            "the infix and prefix spellings are one binding: {infix_src} vs {prefix_src}"
        );
    }

    // A constrained body that calls a method of a class the context neither
    // names nor entails is still rejected, so the given context is a genuine
    // restriction and not a blanket "anything goes". BOTH spellings are checked:
    // numeric defaulting must not rescue `Num` under an `Ord`-only context.
    for src in [
        "f x :: Ord a => a -> a = (+) x x;\nmain = 0.0;",
        "f x :: Ord a => a -> a = x + x;\nmain = 0.0;",
    ] {
        let msg = type_err(src);
        assert!(
            msg.contains("cannot resolve method `+`"),
            "`+` (Num) under only `Ord a` must be rejected, got: {msg} ({src})"
        );
    }
}

#[test]
fn no_instance_is_still_a_compile_error() {
    // The body is typed once, so the constraint is discharged at each CALL SITE.
    // A concrete index type with no instance cannot supply the dictionary and
    // must fail at compile time — never fall back to a runtime lookup.
    let src = "mk p :: Ix i => Pair i i -> Int = rangeSize p;\nmain = mk (Pair { first: true, second: false });";
    let msg = type_err(src);
    assert!(
        msg.contains("no instance of `Ix` for type `Bool`"),
        "expected a missing-instance error naming the class and type, got: {msg}"
    );

    // The same definition applied at a type that DOES have an instance
    // typechecks, so the error above is the missing instance and nothing else.
    typed(
        "mk p :: Ix i => Pair i i -> Int = rangeSize p;\nmain = mk (Pair { first: 0, second: 9 });",
    );
}

#[test]
fn an_ix_body_still_compiles() {
    // The prelude's array surface is written as constrained definitions; every
    // one of them must still typecheck now that its body is actually inferred.
    let body = "x = index (listArray (Pair { first: 0, second: 2 }) [1.0, 2.0, 3.0]) 1;";
    let toks = tokenize(body).unwrap();
    let prog = parse(&toks, body.as_bytes()).unwrap();
    infer_program(&prog, Mode::Strict)
        .expect("the prelude's Ix bodies must typecheck under their given constraints");

    // ... and they still RUN. The value exercises `listArray` (whose body calls
    // the GIVEN `rangeSize`) and `index` (whose body calls the GIVEN `inRange`
    // and `index` as the dual-named offset method).
    let (defs, expr) = (
        "",
        "index (listArray (Pair { first: 0, second: 2 }) [1.0, 2.0, 3.0]) 1",
    );
    let world = eval_ffi(defs, expr, rill_lang::ffi::ForeignRegistry::<f32>::new())
        .expect("the array helpers must keep working end to end");
    let v = world
        .result()
        .expect("the run must produce a single result");
    assert_eq!(
        world.arena().get(v),
        Some(&rill_lang::arena::Value::Double(2.0)),
        "`index arr 1` must read the second element through the Ix bodies"
    );
}
