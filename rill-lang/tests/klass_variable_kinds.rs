//! A class variable is a TYPE VARIABLE whatever the class's arity.
//!
//! Haskell 2010 §4.1.2 draws no distinction between `a` in `Eq a` (kind `*`)
//! and `m` in `Monad m` (kind `* -> *`): `Maybe`, `W m` and `Kleisli m` are all
//! things `m` can BE. So `m a` must be the application of a VARIABLE
//! (`TyConApp(m, [a])`) and must unify with `Maybe a` by binding `m := Maybe`.
//!
//! rill spelled it `Con("m", [a])` — the NAME `m` used as a concrete
//! constructor — so the unification failed with
//! `cannot unify Con("m", [Double]) with Con("Maybe", [Double])`, and a
//! constrained definition could never be instantiated at a concrete type.
//!
//! The same failure appeared for a class the program invents, which is the proof
//! that nothing here is specific to `Monad`.

mod common;

use common::*;

fn run(src: &str) -> String {
    let mut prog = rill_lang::compile::<f32, 256>(src, rill_lang::ffi::ForeignRegistry::new())
        .unwrap_or_else(|e| panic!("compile failed for:\n{src}\n{e}"));
    prog.run()
        .unwrap_or_else(|e| panic!("run failed for:\n{src}\n{e}"));
    prog.stdout().concat()
}

#[test]
fn a_monad_constrained_definition_instantiates_at_a_concrete_monad() {
    // `f` is polymorphic over the monad; `g :: Maybe Double` is the use that
    // decides `m := Maybe`.
    assert_eq!(
        run("f :: Monad m => Double -> m Double;\n\
             f x = return x;\n\
             g :: Maybe Double = f 3.0;\n\
             main = case g of { Just v -> write (Just (show_double v)) stdout; \
                                Nothing -> write Nothing stdout; };"),
        "3.0"
    );
}

#[test]
fn a_user_class_variable_behaves_the_same_way() {
    // The SAME shape with a class the program invents. If this passes and the
    // `Monad` case above did not, the defect would be `Monad`-specific — it is
    // not.
    assert_eq!(
        run("typeclass Wrap w where { wrapv: a -> w a; }\n\
             data Boxed a = { unBoxed: a };\n\
             instance Wrap Boxed where { wrapv x = Boxed { unBoxed: x }; }\n\
             f :: Wrap w => Double -> w Double;\n\
             f x = wrapv x;\n\
             g :: Boxed Double = f 3.0;\n\
             main = write (Just (show_double g.unBoxed)) stdout;"),
        "3.0"
    );
}

#[test]
fn a_result_directed_method_resolves_through_a_given_constraint() {
    // Inside `f`'s body the container comes from the GIVEN `Monad m`, not from a
    // call site: `return x` is typed as `m Double` for the body's own `m`.
    assert_eq!(
        run("f :: Monad m => Double -> m (Double);\n\
             f x = return x;\n\
             g :: Maybe Double = f 1.5;\n\
             main = case g of { Just v -> write (Just (show_double v)) stdout; \
                                Nothing -> write Nothing stdout; };"),
        "1.5"
    );
}
