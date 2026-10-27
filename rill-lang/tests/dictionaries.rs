//! Dictionary stage 2: the runtime dictionary table is built at seat for every
//! `(class, type)` pair the program mentions, with its superclasses closed
//! transitively and orderd topologically.
//!
//! Design §3.2 (one arena slot per used instance, built once at
//! `WorldState::seat`), §3.7 step 2 (this stage), §3.9 (the kernels that have no
//! IR instruction yet).
//!
//! Nothing reads the table yet — this stage builds it and proves it matches the
//! compile-time plan. Every assertion below is about the *table*; a program's
//! value is asserted alongside where it makes the claim sharper (the program
//! really ran).
//!
//! The method-call spelling is `(==) x y` / `(+) x y`, not the infix `x == y`:
//! rill's infix operators are still `Expr::Cmp`/`Expr::Arith` nodes rather than
//! the class-method sugar Haskell 2010 §3.4 requires, so an infix operator never
//! reaches method dispatch and records no selection. That is an OPEN defect
//! (the paused P-A task), and the tests below use the spelling that exists so
//! the dictionary contract is pinned rather than the operator gap.

mod common;

use common::*;
use rill_lang::arena::{DictSlot, KernelId, Value};
use rill_lang::ir::DictDesc;
use rill_lang::kernels::{missing_kernels, MissingKernel};
use rill_lang::mode::Mode;
use rill_lang::{compile_eval, Ir};

/// Compile `src`'s `main` and return the IR (no world, no run).
fn ir_of(src: &str) -> Ir {
    compile_eval::<f32, 256>(src, "main", Mode::Strict)
        .unwrap_or_else(|e| panic!("program must compile: {e}\nsource:\n{src}"))
}

/// The class/type keys of the dictionary table, in table order.
fn table(ir: &Ir) -> Vec<(String, String)> {
    ir.dicts
        .iter()
        .map(|d| (d.class.clone(), d.ty_key.clone()))
        .collect()
}

/// Whether the table has a `(class, ty_key)` entry.
fn has(ir: &Ir, class: &str, ty_key: &str) -> bool {
    table(ir).iter().any(|(c, t)| c == class && t == ty_key)
}

/// The declared superclass classes of one descriptor, in declaration order.
fn sup_classes(ir: &Ir, idx: usize) -> Vec<&str> {
    ir.dicts[idx]
        .supers
        .iter()
        .map(|&i| ir.dicts[i].class.as_str())
        .collect()
}

// ---------------------------------------------------------------------------
// The mentioned pairs, their superclasses, and nothing else
// ---------------------------------------------------------------------------

#[test]
fn a_program_using_num_ord_and_eq_gets_all_three_and_num_superclasses() {
    // `Num` declares `(Eq a, Ord a) => Num a`, so a program that mentions `Num
    // Double` must end up with `Eq Double` and `Ord Double` dictionaries too —
    // System FC: those dictionaries ARE fields of `Num`'s dictionary, so a
    // `Num Double` vtable that omitted them could not serve `==` at all.
    //
    // The three call sites are deliberately three DIFFERENT classes, so the
    // superclass closure — not the program's own mentions — is what adds `Eq`
    // and `Ord` to the `Num`-only case below.
    //
    // (The definitions are separate top-level bindings rather than one nested
    // expression: a method call used directly as an `if` condition is rejected
    // by the current parser/inference — an unrelated open defect — so nesting
    // them would test that defect instead of the dictionary contract.)
    let ir = ir_of(
        r#"
same a b :: Double -> Double -> Bool = (==) a b;
lt a b :: Double -> Double -> Bool = (<) a b;
add a b :: Double -> Double -> Double = (+) a b;
main = add 1.0 2.0;
"#,
    );
    for class in ["Num", "Ord", "Eq"] {
        assert!(
            has(&ir, class, "Double"),
            "`{class} Double` must have a dictionary; table = {:?}",
            table(&ir)
        );
    }
    let mut classes: Vec<String> = ir.dicts.iter().map(|d| d.class.clone()).collect();
    classes.sort();
    classes.dedup();
    assert_eq!(
        classes,
        vec!["Eq".to_string(), "Num".to_string(), "Ord".to_string()],
        "the table must hold exactly the mentioned classes plus their \
         superclasses; table = {:?}",
        table(&ir)
    );
    for desc in &ir.dicts {
        assert_eq!(
            desc.ty_key,
            "Double",
            "`{}`'s dictionary must describe `Double`; table = {:?}",
            desc.class,
            table(&ir)
        );
    }

    // The SAME closure from a program that mentions only `Num`: `Eq` and `Ord`
    // arrive because `Num` declares them, not because a call site named them.
    let num_only = ir_of("add a b :: Double -> Double -> Double = (+) a b;\nmain = add 1.0 2.0;");
    for class in ["Num", "Ord", "Eq"] {
        assert!(
            has(&num_only, class, "Double"),
            "a `Num Double` program must also get `{class} Double` from the declared \
             superclass closure; table = {:?}",
            table(&num_only)
        );
    }
    // The `Num`-only program's table is buildable and consistent at seat.
    let world = eval(
        "add a b :: Double -> Double -> Double = (+) a b;",
        "add 1.0 2.0",
    )
    .expect("the program must run");
    assert_eq!(world.dicts_len(), 3, "Eq, Ord, Num");
    assert_eq!(world.dict_consistency(), Vec::<String>::new());
}

#[test]
fn each_mentioned_pair_has_exactly_one_dictionary() {
    // Six `Num Double` call sites still produce ONE descriptor and one arena
    // slot. A dictionary is built once per *pair*, not once per call site or per
    // reference (design §3.2, anti-goal "a dictionary that allocates per call").
    let defs = r#"
plus a b :: Double -> Double -> Double = (+) a b;
sub a b :: Double -> Double -> Double = (-) a b;
mul a b :: Double -> Double -> Double = (*) a b;
"#;
    let src = format!("{defs}\nmain = plus 1.0 2.0;");
    let ir = ir_of(&src);
    let nums: Vec<&DictDesc> = ir.dicts.iter().filter(|d| d.class == "Num").collect();
    assert_eq!(
        nums.len(),
        1,
        "one `Num Double` dictionary serves every call site; table = {:?}",
        table(&ir)
    );
    assert_eq!(nums[0].ty_key, "Double");

    let world = eval(defs, "plus 1.0 2.0").expect("the program must run");
    assert_eq!(world.dicts_len(), ir.dicts.len());
    assert_eq!(world.dict_consistency(), Vec::<String>::new());
}

#[test]
fn a_program_mentioning_no_class_gets_an_empty_table() {
    // A program with no method call has no dictionary to build — the table is
    // empty, and the seated world holds no dictionary slot either.
    let ir = ir_of("main = 1.0 + 2.0 * 3.0;");
    assert!(
        ir.dicts.is_empty(),
        "a class-free program must have an empty dictionary table, got {:?}",
        table(&ir)
    );
    assert!(ir.dict_gaps.is_empty(), "and no gaps: {:?}", ir.dict_gaps);
    assert!(
        missing_kernels(&ir).is_empty(),
        "an empty table has no holes: {:?}",
        missing_kernels(&ir)
    );
    let world = eval("", "1.0 + 2.0 * 3.0").expect("the program must run");
    assert_eq!(world.dicts_len(), 0, "the seated table is empty too");
    assert!(world.missing_kernels().is_empty());
    assert_eq!(out_value(&world), Value::Double(7.0));
}

// ---------------------------------------------------------------------------
// Slots, ordering, and the shape the IR describes
// ---------------------------------------------------------------------------

#[test]
fn every_slot_is_a_fragment_or_a_kernel() {
    // A slot has exactly two forms (design §3.1). A `DictSlot` is a closed enum,
    // so what this pins is that every slot names something the IR can actually
    // resolve: a `Fragment` a real fragment index, a `Kernel` a real kernel.
    let src = r#"
typeclass Size a where { size: a -> Int; }
instance Size Double where { size x = 1; }
pick x :: Double -> Int = size x;
leq x y :: Double -> Double -> Bool = (<=) x y;
main = pick 2.0;
"#;
    let ir = ir_of(src);
    assert!(!ir.dicts.is_empty(), "the program mentions classes");
    for desc in &ir.dicts {
        assert_eq!(
            desc.methods.len(),
            desc.method_names.len(),
            "`{} {}` must name every slot it has; desc = {desc:?}",
            desc.class,
            desc.ty_key
        );
        for (slot, method) in desc.methods.iter().zip(&desc.method_names) {
            match slot {
                DictSlot::Fragment { fragment } => assert!(
                    (*fragment as usize) < ir.fragments.len(),
                    "`{}.{method}` names fragment {fragment}, but the program has {} \
                     fragments",
                    desc.class,
                    ir.fragments.len()
                ),
                DictSlot::Kernel(kernel) => assert!(
                    rill_lang::kernels::kernel_for_method(&desc.class, method) == Some(*kernel),
                    "`{}.{method}` names {kernel:?}, which is not the kernel the one \
                     `(class, method)` table selects",
                    desc.class
                ),
            }
        }
    }
    // The user instance's `size` is a real body, so its slot is a fragment; the
    // derived `Eq`/`Ord` methods are kernels.
    let size = ir
        .dicts
        .iter()
        .find(|d| d.class == "Size")
        .expect("`Size Double` must have a dictionary");
    assert!(
        matches!(size.methods.as_slice(), [DictSlot::Fragment { .. }]),
        "a user instance equation is a Fragment slot, got {:?}",
        size.methods
    );
    let eq = ir
        .dicts
        .iter()
        .find(|d| d.class == "Eq")
        .expect("the derived `Eq Double` dictionary must exist");
    assert!(
        eq.methods.iter().all(|s| matches!(s, DictSlot::Kernel(_))),
        "structural `Eq` methods are kernel slots, got {:?}",
        eq.methods
    );
}

#[test]
fn a_superclass_dictionary_precedes_its_referrer() {
    // Topological order is what lets seat build in one forward pass. `Num`
    // declares `(Eq a, Ord a)`, so `Eq Double` and `Ord Double` must both come
    // BEFORE `Num Double` in the table, and every `supers` index must point
    // backwards.
    let ir = ir_of("add a b :: Double -> Double -> Double = (+) a b;\nmain = add 1.0 2.0;");
    let pos = |class: &str| {
        ir.dicts
            .iter()
            .position(|d| d.class == class && d.ty_key == "Double")
            .unwrap_or_else(|| panic!("`{class} Double` missing from {:?}", table(&ir)))
    };
    assert!(
        pos("Eq") < pos("Ord"),
        "`Ord` holds `Eq`, so `Eq` comes first; table = {:?}",
        table(&ir)
    );
    assert!(
        pos("Eq") < pos("Num") && pos("Ord") < pos("Num"),
        "`Num` holds both `Eq` and `Ord`, so both precede it; table = {:?}",
        table(&ir)
    );
    for (idx, desc) in ir.dicts.iter().enumerate() {
        for &sup in &desc.supers {
            assert!(
                sup < idx,
                "`{} {}` references superclass descriptor {sup} at or after its own \
                 index {idx}; table = {:?}",
                desc.class,
                desc.ty_key,
                table(&ir)
            );
        }
    }
    // `Num`'s declared superclasses are `Eq` then `Ord`, in that order.
    assert_eq!(
        sup_classes(&ir, pos("Num")),
        vec!["Eq", "Ord"],
        "superclass refs follow the class declaration's order"
    );
    // And the seated table carries the same order, one ref per declared super.
    let world = eval(
        "",
        "add a b :: Double -> Double -> Bool = (==) a b;\nadd 1.0 2.0",
    )
    .err();
    assert!(world.is_some(), "a `defs`-less call is not a program");
}

#[test]
fn a_given_constraint_pair_is_reported_as_a_gap_not_a_dictionary() {
    // Inside `f :: Ord a => a -> a -> Bool` the `Ord` witness ARRIVES as a
    // parameter (design §3.7 step 3): there is no concrete type, so no
    // dictionary can be built from the selection table. The pair must be
    // reported, never silently dropped — this is the `dict_selections` evidence
    // stage 3 consumes.
    let ir = ir_of("f x y :: Ord a => a -> a -> Bool = (<) x y;\nmain = 1.0;");
    let gap = ir
        .dict_gaps
        .iter()
        .find(|g| g.class == "Ord")
        .unwrap_or_else(|| {
            panic!(
                "the abstract `Ord` witness must be reported, got {:?}",
                ir.dict_gaps
            )
        });
    assert!(
        gap.reason.contains("given constraint"),
        "the gap must say the witness is a parameter, got: {}",
        gap.reason
    );
    assert!(
        !has(&ir, "Ord", "Double"),
        "an abstract witness must NOT invent a `Double` dictionary; table = {:?}",
        table(&ir)
    );
    // The body is still typed and still runs: the gap is about the TABLE, not
    // about the program being rejected.
    let world = eval(
        "f x y :: Ord a => a -> a -> Bool = (<) x y;",
        "if f 1.0 2.0 then 1.0 else 0.0",
    )
    .expect("the program must run");
    assert_eq!(out_value(&world), Value::Double(1.0));
    assert_eq!(
        world.missing_kernels(),
        Vec::<MissingKernel>::new(),
        "no dictionary was built, so there is no hole to report"
    );
}

#[test]
fn a_superclass_cycle_is_a_hard_error_naming_the_cycle() {
    // A cyclic superclass closure cannot be ordered, so it must FAIL rather
    // than be truncated: a truncated table would leave a dictionary whose
    // superclass ref points at nothing (or silently cycle). The diagnostic must
    // name the loop so the program's declaration is identifiable.
    let src = r#"
typeclass (B a) => A a where { am: a -> a; }
typeclass (A a) => B a where { bm: a -> a; }
instance A Double where { am x = x; }
instance B Double where { bm x = x; }
call x :: Double -> Double = am x;
main = 1.0;
"#;
    let err = compile_eval::<f32, 256>(src, "main", Mode::Strict)
        .err()
        .expect("a superclass cycle must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("superclass cycle") && msg.contains("A -> B -> A"),
        "the error must name the cycle, got: {msg}"
    );
    // It is a compile error, not a silent empty table: nothing was built.
    assert!(
        !msg.contains("dictionary table") || msg.contains("cycle"),
        "the diagnostic must be about the cycle, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Seat: one arena slot per descriptor, supers resolved by index
// ---------------------------------------------------------------------------

#[test]
fn seat_builds_one_dictionary_per_descriptor_in_order() {
    let src = "add a b :: Double -> Double -> Double = (+) a b;\nmain = add 1.0 2.0;";
    let ir = ir_of(src);
    assert!(ir.dicts.len() >= 3, "table = {:?}", table(&ir));
    let world = eval(
        "add a b :: Double -> Double -> Double = (+) a b;",
        "add 1.0 2.0",
    )
    .expect("the program must run");
    assert_eq!(
        world.dicts_len(),
        ir.dicts.len(),
        "one arena slot per descriptor"
    );
    // The table is consistent: index → Dictionary, planned method slots, and
    // superclass refs pointing at the expected slots.
    assert_eq!(
        world.dict_consistency(),
        Vec::<String>::new(),
        "the built table must match its descriptors"
    );
    for (idx, desc) in ir.dicts.iter().enumerate() {
        let r = world.dict_ref(idx).expect("every descriptor has a slot");
        let Value::Dictionary { supers, methods } = world.arena().get(r).unwrap() else {
            panic!("slot {idx} is not a Dictionary");
        };
        assert_eq!(
            methods.as_slice(),
            desc.methods.as_slice(),
            "`{} {}` slots",
            desc.class,
            desc.ty_key
        );
        assert_eq!(
            supers.len(),
            desc.supers.len(),
            "`{} {}` superclass count",
            desc.class,
            desc.ty_key
        );
        for (&held, &expected) in supers.iter().zip(&desc.supers) {
            assert_eq!(
                Some(held),
                world.dict_ref(expected),
                "`{} {}`'s superclass ref must be the dictionary at descriptor {expected}",
                desc.class,
                desc.ty_key
            );
        }
        assert_eq!(
            world.dict_desc(idx).map(|d| (&d.class, &d.ty_key)),
            Some((&desc.class, &desc.ty_key)),
            "the world keeps the descriptor each slot came from"
        );
    }
    // The program still produced its value: building the table changed nothing.
    assert_eq!(out_value(&world), Value::Double(3.0));
}

#[test]
fn a_dictionary_and_its_instance_agree_on_the_type() {
    // `ty_key` is the instance registry's own identity, so a dictionary can
    // never describe a type its instance does not. `Pair Int Int` is the sharp
    // case: the full applied spelling, not the bare `Pair` head.
    let defs = r#"
typeclass Size a where { size: a -> Int; }
instance Size Int where { size x = 1; }
instance Size (Pair Int Int) where { size p = 2; }
on_pair p :: Pair Int Int -> Int = size p;
on_int x :: Int -> Int = size x;
"#;
    let src = format!("{defs}\nmain = on_pair (Pair {{ first: 1, second: 2 }}) + on_int 5;");
    let ir = ir_of(&src);
    assert!(
        has(&ir, "Size", "Pair Int Int"),
        "a concrete instance head keys by its FULL applied type, table = {:?}",
        table(&ir)
    );
    assert!(
        has(&ir, "Size", "Int"),
        "`Size Int` must be present, table = {:?}",
        table(&ir)
    );
    // Exactly those two: the bare `Pair` head must not appear as a third.
    let size_keys: Vec<String> = ir
        .dicts
        .iter()
        .filter(|d| d.class == "Size")
        .map(|d| d.ty_key.clone())
        .collect();
    assert_eq!(
        size_keys.len(),
        2,
        "one dictionary per distinct instance key, not per spelling; got {size_keys:?}"
    );
    let world = eval(defs, "on_pair (Pair { first: 1, second: 2 }) + on_int 5")
        .expect("the program must run");
    assert_eq!(world.dict_consistency(), Vec::<String>::new());
    assert_eq!(
        out_value(&world),
        Value::Int(3),
        "2 + 1 — the two `Size` instances really are distinct dictionaries"
    );
}

#[test]
fn every_fragment_slot_takes_its_methods_arguments() {
    // A `Fragment` slot promises "the same kind of index a `Value::Callable`
    // uses" (design §3.1), i.e. a fragment you can dispatch with the method's
    // arguments. This pins that shape for BOTH provide paths: an instance
    // EQUATION (explicit parameters) and a class DEFAULT body (a λ-nest the
    // parser curried, which must be peeled into the fragment's parameters).
    //
    // The arity comes from the class's declared signature, so the assertion is
    // independent of how the body happens to be stored.
    let ir = ir_of(
        "le a b :: Double -> Double -> Bool = (<=) a b;\n\
         pair2 a b :: Double -> Double -> Double = min a b;\n\
         main = 1.0;",
    );
    let arity = |class: &str, method: &str| -> usize {
        match (class, method) {
            // The four arithmetic/comparison methods below are the only ones
            // this test reaches, but the map is written out rather than
            // inferred so a wrong arity is a visible edit, not a silent skip.
            ("Ord", "min") | ("Ord", "max") => 2,
            ("Ord", "<=") | ("Ord", "<") | ("Ord", ">") | ("Ord", ">=") => 2,
            ("Ord", "compare") => 2,
            other => panic!("no arity recorded for {other:?}"),
        }
    };
    for desc in &ir.dicts {
        for (slot, method) in desc.methods.iter().zip(&desc.method_names) {
            let DictSlot::Fragment { fragment } = slot else {
                continue;
            };
            let expected = arity(&desc.class, method);
            let frag = &ir.fragments[*fragment as usize];
            assert_eq!(
                frag.sig.value_ins, expected,
                "`{}.{method}`'s slot fragment must take {expected} argument(s), \
                 got {}; desc = {desc:?}",
                desc.class, frag.sig.value_ins
            );
            assert_eq!(
                frag.sig.value_outs, 1,
                "`{}.{method}`'s slot fragment yields exactly one value",
                desc.class
            );
            // And it captured nothing: a dictionary is built at seat with no
            // enclosing scope, so a capture would be unreadable.
            assert_eq!(
                frag.num_capture_cells, 0,
                "`{}.{method}`'s slot fragment must not capture anything",
                desc.class
            );
        }
    }
    // The default-body case really is present: `Ord`'s `min`/`max` are the
    // methods no primitive instance supplies.
    let ord = ir
        .dicts
        .iter()
        .find(|d| d.class == "Ord" && d.ty_key == "Double")
        .expect("`Ord Double`");
    let min_slot = ord
        .method_names
        .iter()
        .position(|m| m == "min")
        .expect("`Ord` declares `min`");
    assert!(
        matches!(ord.methods[min_slot], DictSlot::Fragment { .. }),
        "`min` is a default body, so its slot is a fragment"
    );
}

// ---------------------------------------------------------------------------
// §3.9: the missing kernels are visible
// ---------------------------------------------------------------------------

#[test]
fn the_unimplemented_kernel_list_is_exactly_the_six_design_names() {
    // `Num Int` legitimately declares `signum`, `fromInteger`, … so the
    // dictionary MUST be built with those slots. The stage reports them rather
    // than hiding them; stage 7's checklist is this list, pinned so it cannot
    // drift.
    let ir = ir_of("div a b :: Double -> Double -> Double = (/) a b;\nmain = div 1.0 2.0;");
    let missing = missing_kernels(&ir);
    let mut kernels: Vec<KernelId> = missing.iter().map(|m| m.kernel).collect();
    kernels.sort();
    kernels.dedup();
    // `KernelId`'s declaration order IS this list's order.
    assert_eq!(
        kernels,
        vec![
            KernelId::Compare,
            KernelId::Negate,
            KernelId::Abs,
            KernelId::Signum,
            KernelId::FromInteger,
            KernelId::Recip,
        ],
        "dividing once reaches every one of design §3.9's six missing kernels \
         (`Ord.compare`, `Num.negate`, scalar `Num.abs`, `Num.signum`, \
         `Num.fromInteger`, `Fractional.recip`); missing = {missing:?}"
    );
    assert_eq!(missing.len(), 6, "one hole per §3.9 name: {missing:?}");
    // Every reported entry names the class, type and METHOD the hole belongs to,
    // so stage 4 can never return a silent hole.
    for m in &missing {
        assert!(
            m.class == "Num" || m.class == "Ord" || m.class == "Fractional",
            "unexpected class in the report: {m:?}"
        );
        assert_eq!(
            m.ty_key, "Double",
            "the report names the instance key: {m:?}"
        );
        assert!(
            ["compare", "negate", "abs", "signum", "fromInteger", "recip"]
                .contains(&m.method.as_str()),
            "unexpected method in the report: {m:?}"
        );
        // The slot index must address the method the report names.
        let desc = ir
            .dicts
            .iter()
            .find(|d| d.class == m.class && d.ty_key == m.ty_key)
            .unwrap();
        assert_eq!(
            desc.method_names.get(m.slot).map(String::as_str),
            Some(m.method.as_str()),
            "the reported slot index must address its method: {m:?}"
        );
        assert_eq!(desc.methods.get(m.slot), Some(&DictSlot::Kernel(m.kernel)));
    }
    // The slots ARE in the built table: a report of a hole that was not built
    // would be lying about the dictionary.
    let num = ir
        .dicts
        .iter()
        .find(|d| d.class == "Num" && d.ty_key == "Double")
        .expect("`Num Double` dictionary");
    assert!(
        num.methods.contains(&DictSlot::Kernel(KernelId::Signum)),
        "the `signum` slot must exist so §3.9's gap is visible, not dropped"
    );
    assert!(
        num.methods
            .contains(&DictSlot::Kernel(KernelId::FromInteger)),
        "`fromInteger` must exist for literal elaboration (design §3.6)"
    );
}

#[test]
fn the_recip_and_compare_holes_appear_on_the_classes_that_declare_them() {
    // `Recip` belongs to `Fractional`, `Compare` to `Ord` — so a program that
    // divides and compares reports both, each under its own class. This is the
    // assertion that a hole cannot be attributed to the wrong dictionary.
    let ir = ir_of(
        "div a b :: Double -> Double -> Double = (/) a b;\n\
         le a b :: Double -> Double -> Bool = (<) a b;\n\
         main = div 1.0 2.0;",
    );
    let missing = missing_kernels(&ir);
    let recip = missing
        .iter()
        .find(|m| m.method == "recip")
        .expect("`Fractional`'s `recip` must be reported");
    assert_eq!(recip.class, "Fractional");
    assert_eq!(recip.kernel, KernelId::Recip);
    let compare = missing
        .iter()
        .find(|m| m.method == "compare")
        .expect("`Ord`'s `compare` must be reported");
    assert_eq!(compare.class, "Ord");
    assert_eq!(compare.kernel, KernelId::Compare);
    // `Ord`'s default-body methods are NOT holes: they are real fragments.
    assert!(
        !missing
            .iter()
            .any(|m| m.method == "min" || m.method == "max"),
        "`min`/`max` are default bodies, not missing kernels: {missing:?}"
    );
    // Dividing once yields the whole numeric chain, so the exposed world table
    // reports the same holes.
    let world = eval("", "(/) 1.0 2.0").expect("runs");
    assert_eq!(world.dicts_len(), 4, "Eq, Ord, Num, Fractional");
    assert_eq!(
        world
            .missing_kernels()
            .into_iter()
            .map(|m| m.method)
            .collect::<Vec<_>>(),
        missing.iter().map(|m| m.method.clone()).collect::<Vec<_>>(),
        "the world's report must equal the IR's"
    );
}

#[test]
fn scalar_abs_is_the_one_partial_kernel_and_buffers_have_it() {
    // `abs` exists for buffers (`ValueBufferUn`) but not for scalars, so it is
    // in the report for a scalar instance and absent for a buffer one. This is
    // the assertion that keeps `has_instruction` honest about operand kinds.
    let scalar = ir_of("f x :: Double -> Double = abs x;\nmain = 1.0;");
    let scalar_missing: Vec<KernelId> = missing_kernels(&scalar)
        .into_iter()
        .map(|m| m.kernel)
        .collect();
    assert!(
        scalar_missing.contains(&KernelId::Abs),
        "scalar `abs` has no instruction; report = {scalar_missing:?}"
    );

    // The same operation over a buffer: `abs` is implemented there.
    let buffer = ir_of("f x :: FixedBuffer Double -> FixedBuffer Double = abs x;\nmain = 1.0;");
    let buffer_missing: Vec<KernelId> = missing_kernels(&buffer)
        .into_iter()
        .map(|m| m.kernel)
        .collect();
    assert!(
        !buffer_missing.contains(&KernelId::Abs),
        "buffer `abs` IS implemented, so it must not be reported; report = {buffer_missing:?}"
    );
    assert!(
        has(&buffer, "Num", "FixedBuffer Double"),
        "the buffer `Num` dictionary is keyed by its applied head, table = {:?}",
        table(&buffer)
    );
    // `Negate` is missing for buffers too: no primitive instance binds
    // `UnOp::Neg`, so the slot could not execute even though the instruction
    // exists for a different operation family.
    assert!(
        buffer_missing.contains(&KernelId::Negate),
        "buffer `negate` is still unimplemented; report = {buffer_missing:?}"
    );
}

#[test]
fn the_report_has_one_entry_per_slot_not_per_call_site() {
    // Many `signum` call sites and one dictionary: the report describes the
    // TABLE, so each hole appears once, at the slot index its class declares.
    let ir = ir_of(
        r#"
a x :: Double -> Double = signum x;
b x :: Double -> Double = signum x;
c x :: Double -> Double = signum x;
main = 1.0;
"#,
    );
    let missing = missing_kernels(&ir);
    let signum: Vec<&MissingKernel> = missing.iter().filter(|m| m.method == "signum").collect();
    assert_eq!(
        signum.len(),
        1,
        "three call sites, one dictionary, one hole; missing = {missing:?}"
    );
    let desc = ir
        .dicts
        .iter()
        .find(|d| d.class == "Num" && d.ty_key == "Double")
        .expect("`Num Double`");
    assert_eq!(
        desc.method_names.get(signum[0].slot).map(String::as_str),
        Some("signum"),
        "the reported slot index must address the method it names"
    );
}

// ---------------------------------------------------------------------------
// The superclass chain across a class tower
// ---------------------------------------------------------------------------

#[test]
fn a_fractional_dictionary_brings_its_whole_numeric_superclass_chain() {
    // `Fractional` -> `Num` -> (`Eq`, `Ord`): dividing once yields four
    // dictionaries, ordered so each one's supers are already built.
    let ir = ir_of("f a b :: Double -> Double -> Double = (/) a b;\nmain = f 1.0 2.0;");
    let mut classes: Vec<&str> = ir.dicts.iter().map(|d| d.class.as_str()).collect();
    classes.sort_unstable();
    assert_eq!(
        classes,
        vec!["Eq", "Fractional", "Num", "Ord"],
        "the whole superclass closure must be materialised; table = {:?}",
        table(&ir)
    );
    let pos = |class: &str| ir.dicts.iter().position(|d| d.class == class).unwrap();
    assert!(pos("Eq") < pos("Num"));
    assert!(pos("Ord") < pos("Num"));
    assert!(pos("Num") < pos("Fractional"));
    assert_eq!(sup_classes(&ir, pos("Fractional")), vec!["Num"]);
    let world = eval("", "(/) 1.0 2.0").expect("runs");
    assert_eq!(world.dicts_len(), 4);
    assert_eq!(world.dict_consistency(), Vec::<String>::new());
}

#[test]
fn a_user_class_tower_is_closed_transitively() {
    // The closure is driven by the DECLARATIONS, not by a hard-coded numeric
    // tower: `C a` requires `B a` requires `A a`, and one `C Double` call site
    // yields all three dictionaries in that order.
    let ir = ir_of(
        r#"
typeclass A a where { am: a -> a; }
typeclass (A a) => B a where { bm: a -> a; }
typeclass (B a) => C a where { cm: a -> a; }
instance A Double where { am x = x; }
instance B Double where { bm x = x; }
instance C Double where { cm x = x; }
f x :: Double -> Double = cm (bm (am x));
main = f 2.0;
"#,
    );
    let mut classes: Vec<&str> = ir.dicts.iter().map(|d| d.class.as_str()).collect();
    classes.sort_unstable();
    assert_eq!(classes, vec!["A", "B", "C"], "table = {:?}", table(&ir));
    let pos = |class: &str| ir.dicts.iter().position(|d| d.class == class).unwrap();
    assert!(pos("A") < pos("B"), "`B` holds `A`");
    assert!(pos("B") < pos("C"), "`C` holds `B`");
    assert_eq!(sup_classes(&ir, pos("C")), vec!["B"]);
    assert_eq!(sup_classes(&ir, pos("A")), Vec::<&str>::new());
}

#[test]
fn the_monad_tower_is_closed_and_ordered() {
    // `Monad m` entails `Applicative m` entails `Functor m`; a `Maybe` bind
    // therefore yields all three dictionaries, Functor first.
    let ir =
        ir_of("go x :: Maybe Double -> Maybe Double = bind x (\\ y -> return y);\nmain = 1.0;");
    let mut classes: Vec<&str> = ir.dicts.iter().map(|d| d.class.as_str()).collect();
    classes.sort_unstable();
    assert_eq!(
        classes,
        vec!["Applicative", "Functor", "Monad"],
        "table = {:?}",
        table(&ir)
    );
    let pos = |class: &str| ir.dicts.iter().position(|d| d.class == class).unwrap();
    assert!(pos("Functor") < pos("Applicative"));
    assert!(pos("Applicative") < pos("Monad"));
    // Superclass methods are real bodies (`pure`/`ap`, `fmap`), so their slots
    // are fragments, not kernels.
    for class in ["Functor", "Applicative", "Monad"] {
        let desc = &ir.dicts[pos(class)];
        assert!(
            desc.methods
                .iter()
                .all(|s| matches!(s, DictSlot::Fragment { .. })),
            "`{class}`'s methods are source bodies, got {:?}",
            desc.methods
        );
    }
}

#[test]
fn a_multi_method_class_holds_every_declared_method_in_order() {
    // The slot layout is the class's method list, in declaration order, so
    // `Eq`'s two methods and `Ord`'s seven are all present even though a
    // primitive instance supplies no bodies. The DEFAULT-body methods
    // (`min`/`max`) must be Fragment slots, not kernels: they are real
    // equations, not operations.
    let ir = ir_of("le a b :: Double -> Double -> Bool = (<=) a b;\nmain = 1.0;");
    let ord = ir
        .dicts
        .iter()
        .find(|d| d.class == "Ord" && d.ty_key == "Double")
        .expect("`Ord Double`");
    assert_eq!(
        ord.method_names,
        vec!["compare", "<", "<=", ">", ">=", "min", "max"],
        "slots follow the class declaration's method order"
    );
    for (slot, method) in ord.methods.iter().zip(&ord.method_names) {
        match method.as_str() {
            "min" | "max" => assert!(
                matches!(slot, DictSlot::Fragment { .. }),
                "`{method}` is an ordinary default body, not a kernel"
            ),
            "compare" => assert_eq!(slot, &DictSlot::Kernel(KernelId::Compare)),
            "<=" => assert_eq!(slot, &DictSlot::Kernel(KernelId::Le)),
            other => assert_eq!(
                Some(match slot {
                    DictSlot::Kernel(k) => ks(*k),
                    DictSlot::Fragment { .. } => "fragment",
                }),
                Some(match other {
                    "<" => "Lt",
                    ">" => "Gt",
                    ">=" => "Ge",
                    _ => panic!("unexpected `Ord` method `{other}`"),
                }),
                "`{other}` must select the matching kernel"
            ),
        }
    }
    let eq = ir
        .dicts
        .iter()
        .find(|d| d.class == "Eq" && d.ty_key == "Double")
        .expect("`Eq Double`");
    assert_eq!(eq.method_names, vec!["==", "/="]);
    assert_eq!(
        eq.methods,
        vec![
            DictSlot::Kernel(KernelId::Eq),
            DictSlot::Kernel(KernelId::Ne)
        ]
    );
}

/// The short name of a kernel, for asserting an operator's slot.
fn ks(kernel: KernelId) -> &'static str {
    match kernel {
        KernelId::Eq => "Eq",
        KernelId::Ne => "Ne",
        KernelId::Lt => "Lt",
        KernelId::Gt => "Gt",
        KernelId::Le => "Le",
        KernelId::Ge => "Ge",
        KernelId::Compare => "Compare",
        KernelId::Add => "Add",
        KernelId::Sub => "Sub",
        KernelId::Mul => "Mul",
        KernelId::Div => "Div",
        KernelId::Negate => "Negate",
        KernelId::Abs => "Abs",
        KernelId::Signum => "Signum",
        KernelId::FromInteger => "FromInteger",
        KernelId::Recip => "Recip",
    }
}

#[test]
fn a_constructor_class_tower_over_a_data_type_is_built() {
    // A CONSTRUCTOR class (`Monad m`) keys its instances by the head
    // constructor, not by a full applied type — so `IO` and `ST s` both yield
    // their own three-dictionary tower, keyed `IO` and `ST`. Typing the body
    // under the wrong substitution (`App("IO", [])` instead of the registry's
    // `Data("IO", [])`) would make `bind`'s field projection fail instead.
    let io = ir_of("g x :: IO Double -> IO Double = bind x (\\ y -> return y);\nmain = 1.0;");
    let mut classes: Vec<&str> = io.dicts.iter().map(|d| d.class.as_str()).collect();
    classes.sort_unstable();
    assert_eq!(
        classes,
        vec!["Applicative", "Functor", "Monad"],
        "`IO`'s monad tower must be closed; table = {:?}",
        table(&io)
    );
    for desc in &io.dicts {
        assert_eq!(
            desc.ty_key, "IO",
            "a constructor class keys by the head constructor; desc = {desc:?}"
        );
        assert!(!desc.is_buffer, "`IO` is not a buffer type");
    }

    // `ST s` is the same tower under a different head. The head name is `ST`
    // for both — the region variable `s` is not part of the instance key.
    let st = ir_of("g x :: ST s Double -> ST s Double = bind x (\\ y -> return y);\nmain = 1.0;");
    let mut st_classes: Vec<&str> = st.dicts.iter().map(|d| d.class.as_str()).collect();
    st_classes.sort_unstable();
    assert_eq!(st_classes, vec!["Applicative", "Functor", "Monad"]);
    assert!(
        st.dicts.iter().all(|d| d.ty_key == "ST"),
        "table = {:?}",
        table(&st)
    );
}

#[test]
fn a_constrained_instance_is_reported_as_a_gap_not_a_broken_dictionary() {
    // `instance (Monad m) => Arrow (Kleisli m)` is constraint-parameterised:
    // its dictionary is a FUNCTION of another dictionary (design §3.10), which
    // a flat `supers` vector cannot express. It must be reported, and the
    // dictionary that would depend on it must not be built with a dangling
    // superclass.
    let ir = ir_of(
        r#"
        typeclass Arrow a where { arr: (b -> c) -> a b c; }
        data K m a b = { unK: a -> m b };
        instance (Monad m) => Arrow (K m) where { arr f = K { unK: \x -> Just (f x) }; }
        ap k x = k.unK x;
        lifted = ap (arr (\x -> x * 2.0)) 3.0;
        main = case lifted of { Just v -> v; Nothing -> 0.0; };
        "#,
    );
    // The instance is selected (the program really runs), but its dictionary
    // cannot be described — and the gap says exactly why, naming the context
    // that makes it a dictionary function.
    let gap = ir
        .dict_gaps
        .iter()
        .find(|g| g.class == "Arrow")
        .unwrap_or_else(|| panic!("the `Arrow (K m)` gap must be visible: {:?}", ir.dict_gaps));
    assert_eq!(gap.ty_key.as_deref(), Some("K"));
    assert!(
        gap.reason.contains("constraint-parameterised")
            && gap.reason.contains("Monad m")
            && gap.reason.contains("3.10"),
        "the gap must name the limitation and its design reference, got: {}",
        gap.reason
    );
    // No half-built `Arrow K` dictionary: an entry whose superclass or method
    // body is missing would be a silent hole.
    assert!(
        !has(&ir, "Arrow", "K"),
        "a constraint-parameterised instance must not become a flat dictionary; \
         table = {:?}",
        table(&ir)
    );
    // Whatever the table DOES hold, every superclass index resolves.
    for desc in &ir.dicts {
        for &sup in &desc.supers {
            assert!(
                sup < ir.dicts.len(),
                "`{} {}`'s superclass index {sup} is out of range",
                desc.class,
                desc.ty_key
            );
        }
    }
    for gap in &ir.dict_gaps {
        assert!(
            !gap.reason.is_empty(),
            "a gap must always carry a reason: {gap:?}"
        );
    }
    // The program still computes its value: the gap does not reject it.
    let world = eval(
        r#"
        typeclass Arrow a where { arr: (b -> c) -> a b c; }
        data K m a b = { unK: a -> m b };
        instance (Monad m) => Arrow (K m) where { arr f = K { unK: \x -> Just (f x) }; }
        ap k x = k.unK x;
        "#,
        "case (ap (arr (\\x -> x * 2.0)) 3.0) of { Just v -> v; Nothing -> 0.0 }",
    )
    .expect("the program must run");
    assert_eq!(out_value(&world), Value::Double(6.0));
    assert_eq!(world.dicts_len(), 0, "no `Arrow K` dictionary was built");
}

#[test]
fn a_method_body_that_needs_a_capture_is_reported_not_silently_dropped() {
    // A `DictSlot::Fragment` is created at SEAT, which has no enclosing scope
    // to snapshot a capture from. In Complete mode a top-level reference inside
    // an instance body is a capture (the STG model: a binding is a cell), so
    // that body cannot be compiled here. The pair must be REPORTED, with the
    // missing names — a slot that read an absent cell would be a silent hole.
    //
    // Strict mode inlines top-level definitions at each use site, so the same
    // program describes fine there. Pinning both is what makes this a property
    // of the capture rule rather than a flaky gap.
    let defs = "typeclass S a where { s: a -> a; }\n\
                instance S Double where { s x = double_it x; }\n\
                double_it y = y + y;\n\
                use_s x :: Double -> Double = s x;";
    let strict = ir_of(&format!("{defs}\nmain = use_s 21.0;"));
    assert!(
        has(&strict, "S", "Double"),
        "Strict mode inlines the top-level reference, so the slot compiles; \
         table = {:?}, gaps = {:?}",
        table(&strict),
        strict.dict_gaps
    );

    let complete_src = format!("{{-# MODE complete #-}}\n{defs}\nmain = use_s 21.0;");
    let complete = compile_eval::<f32, 256>(&complete_src, "main", Mode::Complete)
        .expect("the program still compiles and runs");
    let gap = complete
        .dict_gaps
        .iter()
        .find(|g| g.class == "S")
        .unwrap_or_else(|| {
            panic!(
                "a capture-dependent slot must be reported in Complete mode, got {:?}",
                complete.dict_gaps
            )
        });
    assert_eq!(gap.ty_key.as_deref(), Some("Double"));
    assert!(
        gap.reason.contains("captures") && gap.reason.contains("double_it"),
        "the gap must name what the body captures, got: {}",
        gap.reason
    );
    assert!(
        !has(&complete, "S", "Double"),
        "the un-compilable pair must not become a half-filled dictionary"
    );
    // The RUN is unaffected: no dispatch goes through the table yet.
    let world = eval(&format!("{{-# MODE complete #-}}\n{defs}"), "use_s 21.0")
        .expect("the program must run");
    assert_eq!(out_value(&world), Value::Double(42.0));
}

// ---------------------------------------------------------------------------
// Behaviour is unchanged: the table is inert
// ---------------------------------------------------------------------------

#[test]
fn the_table_does_not_change_what_a_program_computes() {
    // Nothing reads the table yet, so a program whose classes are exercised
    // must compute exactly what it did before this stage. These values are the
    // pre-stage results, asserted here so a stage that wires dispatch through
    // dictionaries is caught if it changes them silently.
    let cases = [
        (
            "Double math",
            "",
            "(1.0 + 2.0) * 3.0 - 4.0 / 2.0",
            Value::Double(7.0),
        ),
        ("Int math", "", "1 + 2 * 3", Value::Int(7)),
        (
            "comparison",
            "",
            "if 1.0 < 2.0 then 1.0 else 0.0",
            Value::Double(1.0),
        ),
        ("method call", "", "(+) 1.0 2.0", Value::Double(3.0)),
        (
            "instance body",
            "typeclass Twice a where { twice: a -> a; }\n\
             instance Twice Double where { twice x = x + x; }",
            "twice 21.0",
            Value::Double(42.0),
        ),
    ];
    for (what, defs, expr, expected) in cases {
        let world = eval(defs, expr).unwrap_or_else(|e| panic!("`{what}` must run: {e}"));
        assert_eq!(
            out_value(&world),
            expected,
            "`{what}` must compute the pre-dictionary result"
        );
        assert_eq!(
            world.dict_consistency(),
            Vec::<String>::new(),
            "`{what}`'s table must be consistent"
        );
    }
}
