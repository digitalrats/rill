use rill_core::traits::MultichannelAlgorithm;
use rill_lang::mode::Mode;
use rill_lang::{compile, compile_with_mode};

#[test]
fn recursion_rejected_in_strict() {
    assert!(compile::<f32>("a = a; main = a").is_err());
}

#[test]
fn recursion_accepted_in_complete() {
    // Compiles; running `main = a` where `a = a` yields bottom at runtime
    // (tested in Task A6) — here we only require it to COMPILE.
    let r = compile_with_mode::<f32>("a = a; main = a", Mode::Complete);
    assert!(r.is_ok(), "complete mode must accept recursion");
}

#[test]
fn nonrecursive_value_func_types_as_value() {
    // A non-recursive function returning a cons (List) must type as a VALUE
    // function; if this fails, `from`-style recursion fails for the same
    // rate-resolution reason even before the recursion matters.
    let src = "{-# MODE complete #-}\nwrap n = cons n (list)\nmain = wrap 0.0";
    match rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete) {
        Ok(_) => {}
        Err(e) => panic!("non-recursive value func must compile in complete, got: {e}"),
    }
}

#[test]
fn recursive_float_value_func() {
    // Recursion alone (Float -> Float, no List) — isolates the rate issue.
    // (`+` is used because value-track infix `-` is a pre-existing parser
    // quirk — `+` in a lambda works, `-` does not.)
    let src = "{-# MODE complete #-}\ncountup n = if n == 3.0 then 3.0 else countup (n + 1.0)\nmain = countup 0.0";
    match rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete) {
        Ok(_) => {}
        Err(e) => panic!("recursive float func must compile in complete, got: {e}"),
    }
}

#[test]
fn countdown_compile_only() {
    let src = "{-# MODE complete #-}\ncountup n = if n == 3.0 then 3.0 else countup (n + 1.0)\nmain = countup 0.0";
    let _ = rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
}

#[test]
fn lambda_arith_plus_works() {
    let src = "{-# MODE complete #-}\nmain = (fn a b -> a + b) 5.0 2.0";
    match rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete) {
        Ok(_) => {}
        Err(e) => panic!("lambda plus must compile, got: {e}"),
    }
}

#[test]
fn complete_recursive_countup_runtime() {
    // End-to-end recursion: countup 0 -> 3 (terminates, returns 3.0).
    let src = "{-# MODE complete #-}\ncountup n = if n == 3.0 then 3.0 else countup (n + 1.0)\nmain = countup 0.0";
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, 3.0, "countup 0 should yield 3.0"),
        other => panic!("expected Float(3.0), got {other:?}"),
    }
}

#[test]
fn complete_from_infinite_cons() {
    // `from` produces an infinite lazy cons list; `head`/`tail` force elements.
    // take 3 (from 10) -> [10, 11, 12]; head of the 3rd tail is 12.
    let src = r#"{-# MODE complete #-}
from n = cons n (from (n + 1.0));
headv xs = match (head xs) of { Nothing => 0.0; Just x => x };
main = headv (tail (tail (tail (from 10.0))));"#;
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => {
            assert_eq!(*x, 13.0, "from 10, third tail head -> 13.0")
        }
        other => panic!("expected Float(12.0), got {other:?}"),
    }
}

#[test]
fn match_maybe_syntax_works() {
    let src = "{-# MODE complete #-}\nmain = match head (cons 1.0 (list)) of { Nothing => 0.0; Just x => x }";
    match rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete) {
        Ok(_) => {}
        Err(e) => panic!("match syntax must compile, got: {e}"),
    }
}

#[test]
fn simple_headv_def_parses() {
    let src = "headv xs = head xs\nmain = headv (cons 1.0 (list))";
    match rill_lang::compile::<f32>(src) {
        Ok(_) => {}
        Err(e) => panic!("simple headv def must parse, got: {e}"),
    }
}

#[test]
fn headv_match_def_parses() {
    let src = "headv xs = match (head xs) of { Nothing => 0.0; Just x => x }\nmain = headv (cons 1.0 (list))";
    match rill_lang::compile::<f32>(src) {
        Ok(_) => {}
        Err(e) => panic!("headv match def must parse, got: {e}"),
    }
}

#[test]
fn complete_from_produce_cons() {
    let src = "{-# MODE complete #-}\nfrom n = cons n (from (n + 1.0));\nmain = from 10.0;";
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Cons(h, _) => {
            assert_eq!(
                prog.arena().get(*h).unwrap(),
                &rill_lang::arena::Value::Float(10.0)
            );
        }
        other => panic!("expected Cons, got {other:?}"),
    }
}

#[test]
fn complete_lazy_cons_finite() {
    let src = "{-# MODE complete #-}\nmain = cons 10.0 (list);";
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Cons(h, _) => {
            assert_eq!(
                prog.arena().get(*h).unwrap(),
                &rill_lang::arena::Value::Float(10.0)
            );
        }
        other => panic!("expected Cons, got {other:?}"),
    }
}

#[test]
fn complete_from_head() {
    let src = "{-# MODE complete #-}\nfrom n = cons n (from (n + 1.0));\nmain = head (from 10.0);";
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Sum(0, payload) => {
            assert_eq!(
                prog.arena().get(payload[0]).unwrap(),
                &rill_lang::arena::Value::Float(10.0)
            );
        }
        other => panic!("expected Just(10.0), got {other:?}"),
    }
}

#[test]
fn complete_def_cons_no_recursion() {
    let src = "{-# MODE complete #-}\ngen n = cons n (list);\nmain = gen 10.0;";
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Cons(h, _) => {
            assert_eq!(
                prog.arena().get(*h).unwrap(),
                &rill_lang::arena::Value::Float(10.0)
            );
        }
        other => panic!("expected Cons, got {other:?}"),
    }
}

#[test]
fn value_track_minus_and_negative_literal() {
    // `n - 1.0` is infix subtraction; `-2.0` is a negative literal.
    // `cons` forces `sub` value-rate; `-2.0` is a negative literal, `n - 1.0`
    // infix subtraction in the value track.
    let src = "{-# MODE complete #-}\nsub n = cons (n - 1.0) (list);\nmain = match (head (sub -2.0)) of { Nothing => 0.0; Just x => x };";
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, -3.0, "-2.0 - 1.0 = -3.0"),
        other => panic!("expected Float(-3.0), got {other:?}"),
    }
}

#[test]
fn complete_countdown_with_minus() {
    // Canonical recursion now that value-track `-` works: countdown 3 -> 0.
    let src = "{-# MODE complete #-}\ncountdown n = if n == 0.0 then 0.0 else countdown (n - 1.0);\nmain = countdown 3.0;";
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, 0.0, "countdown 3 -> 0.0"),
        other => panic!("expected Float(0.0), got {other:?}"),
    }
}

#[test]
fn complete_let_binding_is_lazy_thunk() {
    // A `let` binding in complete mode suspends as a thunk: xs = from 10 is
    // forced by headv/tail on demand.
    let src = "{-# MODE complete #-}\nfrom n = cons n (from (n + 1.0));\nheadv xs = match (head xs) of { Nothing => 0.0; Just x => x };\nmain = let xs = from 10.0 in headv (tail (tail xs));";
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => {
            assert_eq!(*x, 12.0, "let xs = from 10; headv (tail tail xs) -> 12.0")
        }
        other => panic!("expected Float(12.0), got {other:?}"),
    }
}
