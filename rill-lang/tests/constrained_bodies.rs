//! The type-system groundwork for runtime dictionaries (design §3.0/§3.1):
//! a `ValueTy::Dict` (the type of a dictionary parameter) and
//! `Scheme.constraints` (the class context a scheme requires).
//!
//! Dictionaries are the RUNTIME representation of a constraint the type system
//! has already proved satisfiable, so these tests pin the static layer only:
//! a constrained scheme records `(class, type variable)`, a dictionary unifies
//! by class identity, and a dictionary has no data interpretation (it is not a
//! nominal type, not a registry key, and has its own stable rank).

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
        ValueTy::Data(_, args) | ValueTy::Newtype(_, args) | ValueTy::App(_, args) => {
            args.iter().any(|a| mentions_var(a, id))
        }
        ValueTy::Int
        | ValueTy::Float
        | ValueTy::Bool
        | ValueTy::String
        | ValueTy::Buffer
        | ValueTy::Void
        | ValueTy::Dict(_) => false,
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
        ValueTy::Data("Num".to_string(), vec![]),
        ValueTy::App("Num".to_string(), vec![ValueTy::Int]),
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
        env.type_name_of_vty(&ValueTy::Data("Num".to_string(), vec![])),
        Some("Num".to_string()),
        "control: a nominal data type still names itself"
    );

    // A dictionary is never a registry key, so it can never select an instance.
    assert_eq!(
        value_type_key(&dict),
        None,
        "a dictionary must not spell an instance-registry key"
    );
    assert_eq!(
        value_type_key(&ValueTy::Int),
        Some("Int".to_string()),
        "control: a concrete type still spells its key"
    );

    // `kind_rank` is total and stable: every value type has a rank, a
    // dictionary sorts after every data form, and no data type shares its rank.
    let dict_rank = kind_rank(&dict);
    for data in [
        ValueTy::Int,
        ValueTy::Float,
        ValueTy::Bool,
        ValueTy::String,
        ValueTy::Buffer,
        ValueTy::Void,
        ValueTy::Data("Pair".to_string(), vec![]),
        ValueTy::Newtype("Wrap".to_string(), vec![]),
        ValueTy::App("List".to_string(), vec![]),
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
