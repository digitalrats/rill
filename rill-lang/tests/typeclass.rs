//! Typeclass tests: `typeclass`/`instance` declare polymorphic methods that
//! resolve **at compile time** to the concrete instance's body. The method
//! argument's static type selects the instance; the body is β-substituted at
//! compile time (zero runtime dispatch — typeclasses and instances produce no
//! arena values).

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn typeclass_method_resolves_at_compile_time() {
    // `show: a` declares the method over the class variable. `show f = f` is
    // the Float instance's identity body: `show 1.0` resolves to it, binds
    // `f := 1.0`, and the value output is Float(1.0). If resolution did not
    // happen (or the param binding were lost), the call could not compile.
    let src = r#"
        typeclass Show a where { show: a; }
        instance Show Float where { show f = f; }
        main = show 1.0;
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let vo = prog.value_outputs();
    assert_eq!(vo.len(), 1);
    let v = vo[0].unwrap();
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Float(1.0));
}

#[test]
fn typeclass_selects_instance_by_argument_type() {
    // Two instances of the same class with different bodies: the argument's
    // static type must pick the right one. `show 1` (an Int literal) must
    // resolve to the Int instance's body (2.5), not the Float instance's (1.5).
    let src = r#"
        typeclass Show a where { show: a; }
        instance Show Float where { show f = 1.5; }
        instance Show Int where { show f = 2.5; }
        main = show 1;
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let vo = prog.value_outputs();
    assert_eq!(vo.len(), 1);
    let v = vo[0].unwrap();
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Float(2.5));
}
