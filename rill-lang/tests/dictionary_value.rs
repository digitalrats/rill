//! The runtime dictionary value: its layout, the RC ownership of its
//! superclass dictionaries, the pool-aware clone, its [`ValueKind`], and
//! structural comparison.
//!
//! These are Rust-level tests over the arena API. Nothing in the compiler
//! constructs a dictionary yet — dictionary construction at
//! [`WorldState::seat`](rill_lang::WorldState::seat) belongs to a later stage —
//! so every test builds the value directly in an arena and inspects it there.
//!
//! `value_cmp_ref` is private to the interpreter, so the structural-order half
//! of the contract is proven from inside the crate by
//! `backend::interp::dictionary_walker_tests`; the public half (structural
//! equality over `supers` AND `methods`) is asserted here.
//!
//! Ownership convention (the arena's, see `rill_core::memory::Arena`):
//! allocating a value yields a ref with `rc == 1` that the CALLER owns.
//! Placing that ref into a container MOVES it, so the container's children are
//! accounted for by construction — which is exactly what lets these tests read
//! a `drop_ref` on the dictionary as "the dictionary released its child".

use rill_core::memory::{Arena, ArenaValue};
use rill_lang::arena::{DictSlot, KernelId, Value, ValueKind};

/// A leaf dictionary: a class with no superclasses and one kernel method.
fn leaf_dict(method: KernelId) -> Value {
    Value::Dictionary {
        supers: Vec::new(),
        methods: vec![DictSlot::Kernel(method)],
    }
}

/// The dictionary under test: `Ord`-shaped — one superclass (`Eq`) and one
/// method slot.
fn ord_dict(eq: u32) -> Value {
    Value::Dictionary {
        supers: vec![eq],
        methods: vec![DictSlot::Fragment { fragment: 3 }],
    }
}

#[test]
fn a_dictionary_is_a_value() {
    let mut a = Arena::with_capacity(8);
    // The superclass dictionary is itself a dictionary — the layout nests.
    let eq = a.alloc(leaf_dict(KernelId::Eq)).unwrap();
    let ord = a
        .alloc(Value::Dictionary {
            supers: vec![eq],
            methods: vec![
                DictSlot::Fragment { fragment: 3 },
                DictSlot::Kernel(KernelId::Compare),
            ],
        })
        .unwrap();

    match a.get(ord).expect("the dictionary slot must resolve") {
        Value::Dictionary { supers, methods } => {
            assert_eq!(supers, &vec![eq], "the superclass ref reads back intact");
            assert_eq!(
                methods,
                &vec![
                    DictSlot::Fragment { fragment: 3 },
                    DictSlot::Kernel(KernelId::Compare),
                ],
                "method slots are positional and read back intact"
            );
        }
        other => panic!("expected a Dictionary, got {other:?}"),
    }
    assert!(
        matches!(a.get(eq), Some(Value::Dictionary { .. })),
        "the superclass ref resolves to a dictionary, not a dangling slot"
    );
}

#[test]
fn its_supers_are_rc_children() {
    let mut a = Arena::with_capacity(8);
    let eq = a.alloc(leaf_dict(KernelId::Eq)).unwrap();
    // Ownership of `eq` moves into the dictionary's `supers` vector.
    let ord = a.alloc(ord_dict(eq)).unwrap();
    assert_eq!(a.rc(eq), 1, "the dictionary owns the one reference");

    a.drop_ref(ord);
    assert_eq!(
        a.rc(eq),
        0,
        "dropping the dictionary must release its super (a leaf walker misses this)"
    );
    assert!(a.get(eq).is_none(), "the released super's slot is freed");

    // A second owner keeps the super alive across the dictionary's drop: this
    // rules out "rc decremented by accident" and proves real child ownership.
    let mut b = Arena::with_capacity(8);
    let eq2 = b.alloc(leaf_dict(KernelId::Eq)).unwrap();
    let ord2 = b.alloc(ord_dict(eq2)).unwrap();
    let keep = b.copy(eq2).unwrap();
    assert_eq!(b.rc(eq2), 2);
    b.drop_ref(ord2);
    assert_eq!(b.rc(eq2), 1, "one owner (the extra copy) remains");
    assert!(
        b.get(eq2).is_some(),
        "the super survives via the extra owner"
    );
    b.drop_ref(keep);
    assert_eq!(b.rc(eq2), 0);
}

#[test]
fn it_recounts_on_copy() {
    let mut a = Arena::with_capacity(8);
    let eq = a.alloc(leaf_dict(KernelId::Eq)).unwrap();
    let ord = a.alloc(ord_dict(eq)).unwrap();

    // A COW copy clones the payload and recounts every child ref into the new
    // owner (the same `visit_refs` walk `recount_children` mirrors).
    let shared = a.copy(ord).unwrap();
    assert_eq!(
        a.rc(eq),
        1,
        "sharing the dictionary itself does not touch the super"
    );

    let fresh = a.mutate(shared).unwrap();
    assert_ne!(fresh, shared, "a shared value is copied before mutation");
    assert_eq!(
        a.rc(eq),
        2,
        "the copy's supers are recounted (one owner per dictionary slot)"
    );

    a.drop_ref(shared);
    assert_eq!(a.rc(eq), 1, "dropping the original leaves the copy's ref");
    a.drop_ref(fresh);
    assert_eq!(a.rc(eq), 0, "dropping the copy releases the last ref");
}

#[test]
fn it_clones_deeply_for_the_pool() {
    let mut a = Arena::with_capacity(8);
    let eq = a.alloc(leaf_dict(KernelId::Eq)).unwrap();
    let ord = a.alloc(ord_dict(eq)).unwrap();

    // The pool-aware clone (the arena half of `pool_clone_value`): the clone
    // carries the `supers` refs, not a stripped shell.
    let cloned = a.clone_pooled(ord).expect("a dictionary clones");
    let cloned_supers = match &cloned {
        Value::Dictionary { supers, methods } => {
            assert_eq!(methods.len(), 1, "method slots survive the clone");
            supers.clone()
        }
        other => panic!("expected a Dictionary, got {other:?}"),
    };
    assert_eq!(cloned_supers, vec![eq]);

    // Give the clone its own ownership of the supers (what `alloc_copy` does
    // through `recount_children`) and store it.
    cloned.visit_refs(|r| {
        a.copy(r).unwrap();
    });
    let copy = a.alloc(cloned).unwrap();
    assert_eq!(a.rc(eq), 2, "the clone owns its own super reference");

    a.drop_ref(ord);
    assert_eq!(a.rc(eq), 1, "the pooled clone keeps the super reachable");
    assert!(
        matches!(a.get(eq), Some(Value::Dictionary { .. })),
        "the super is still a live dictionary after the original is gone"
    );
    a.drop_ref(copy);
    assert_eq!(a.rc(eq), 0);
}

#[test]
fn kind_is_dictionary() {
    let mut a = Arena::with_capacity(4);
    let eq = a.alloc(leaf_dict(KernelId::Eq)).unwrap();
    let ord = a.alloc(ord_dict(eq)).unwrap();

    assert_eq!(a.get(ord).unwrap().kind(), ValueKind::Dictionary);
    assert_eq!(
        Value::Dictionary {
            supers: Vec::new(),
            methods: Vec::new(),
        }
        .kind(),
        ValueKind::Dictionary,
        "an empty dictionary is still a dictionary"
    );
}

#[test]
fn comparison_is_defined() {
    // Structural equality is defined over `supers` AND `methods` (derived
    // `PartialEq`), so a dictionary can be compared without panicking and a
    // difference in either half is visible. The interpreter's `value_cmp_ref`
    // is private; the in-crate
    // `backend::interp::dictionary_walker_tests::value_cmp_ref_orders_dictionaries`
    // covers that half over the same values.
    let mut a = Arena::with_capacity(8);
    let eq = a.alloc(leaf_dict(KernelId::Eq)).unwrap();

    let left = a.alloc(ord_dict(eq)).unwrap();
    let same = a.alloc(ord_dict(eq)).unwrap();
    let other_method = a
        .alloc(Value::Dictionary {
            supers: vec![eq],
            methods: vec![DictSlot::Kernel(KernelId::Compare)],
        })
        .unwrap();
    let no_supers = a
        .alloc(Value::Dictionary {
            supers: Vec::new(),
            methods: vec![DictSlot::Fragment { fragment: 3 }],
        })
        .unwrap();

    assert_eq!(a.get(left).unwrap(), a.get(same).unwrap());
    assert_ne!(
        a.get(left).unwrap(),
        a.get(other_method).unwrap(),
        "differing method slots make dictionaries unequal"
    );
    assert_ne!(
        a.get(left).unwrap(),
        a.get(no_supers).unwrap(),
        "differing supers make dictionaries unequal"
    );
    // Debug must not panic on the new payload either.
    assert!(format!("{:?}", a.get(left).unwrap()).contains("Dictionary"));
}
