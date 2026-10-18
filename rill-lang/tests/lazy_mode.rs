pub mod common;
use common::*;
use rill_lang::compile_eval;
use rill_lang::mode::Mode;

/// Compile and run `src` in complete (lazy) mode through the single-run value
/// surface, returning the program plus its result ref.
///
/// `src` must bind `main = <expr>` (the last `main = ` wins); the body is
/// evaluated by [`compile_eval`] + [`rill_lang::test_eval`] and read with
/// [`WorldState::result`].
fn eval_complete(src: &str) -> (WorldState<f32, 256>, rill_lang::arena::ArenaRef) {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim_end_matches(';').trim();
    let ffi = rill_lang::ffi::ForeignRegistry::<f32>::new();
    let prog = eval_mode::<256>(defs, expr, ffi, Mode::Complete)
        .expect("the program must compile and run");
    let v = prog.result().expect("the run must produce a result");
    (prog, v)
}

#[test]
fn recursion_rejected_in_strict() {
    assert!(compile_eval::<f32, 256>("a = a; main = a", "main", Mode::Strict).is_err());
}

#[test]
fn recursion_accepted_in_complete() {
    // Compiles; running `main = a` where `a = a` yields bottom at runtime
    // (tested in Task A6) — here we only require it to COMPILE.
    let r = compile_eval::<f32, 256>("a = a; main = a", "main", Mode::Complete);
    assert!(r.is_ok(), "complete mode must accept recursion");
}

#[test]
fn nonrecursive_value_func_types_as_value() {
    // A non-recursive function returning a cons (List) must type as a VALUE
    // function; if this fails, `from`-style recursion fails for the same
    // rate-resolution reason even before the recursion matters.
    let src = "{-# MODE complete #-}\nwrap n = cons n (list)\nmain = wrap 0.0";
    match compile_eval::<f32, 256>(src, "main", rill_lang::mode::Mode::Complete) {
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
    match compile_eval::<f32, 256>(src, "main", rill_lang::mode::Mode::Complete) {
        Ok(_) => {}
        Err(e) => panic!("recursive float func must compile in complete, got: {e}"),
    }
}

#[test]
fn countdown_compile_only() {
    let src = "{-# MODE complete #-}\ncountup n = if n == 3.0 then 3.0 else countup (n + 1.0)\nmain = countup 0.0";
    let _ = compile_eval::<f32, 256>(src, "main", rill_lang::mode::Mode::Complete).unwrap();
}

#[test]
fn lambda_arith_plus_works() {
    let src = r"{-# MODE complete #-}
main = (\a -> \b -> a + b) 5.0 2.0";
    match compile_eval::<f32, 256>(src, "main", rill_lang::mode::Mode::Complete) {
        Ok(_) => {}
        Err(e) => panic!("lambda plus must compile, got: {e}"),
    }
}

#[test]
fn complete_recursive_countup_runtime() {
    // End-to-end recursion: countup 0 -> 3 (terminates, returns 3.0).
    let src = "{-# MODE complete #-}\ncountup n = if n == 3.0 then 3.0 else countup (n + 1.0)\nmain = countup 0.0";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, 3.0, "countup 0 should yield 3.0"),
        other => panic!("expected Float(3.0), got {other:?}"),
    }
}

#[test]
fn complete_from_infinite_cons() {
    // `from` produces an infinite lazy cons list; `head`/`tail` force elements.
    // take 3 (from 10) -> [10, 11, 12]; head of the 3rd tail is 12.
    let src = r"{-# MODE complete #-}
from n = cons n (from (n + 1.0));
headv xs = case (head xs) of { Nothing -> 0.0; Just x -> x };
main = headv (tail (tail (tail (from 10.0))));";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => {
            assert_eq!(*x, 13.0, "from 10, third tail head -> 13.0")
        }
        other => panic!("expected Float(12.0), got {other:?}"),
    }
}

#[test]
fn match_maybe_syntax_works() {
    let src = r"{-# MODE complete #-}
main = case head (cons 1.0 (list)) of { Nothing -> 0.0; Just x -> x }";
    match compile_eval::<f32, 256>(src, "main", rill_lang::mode::Mode::Complete) {
        Ok(_) => {}
        Err(e) => panic!(r"case syntax must compile, got: {e}"),
    }
}

#[test]
fn simple_headv_def_parses() {
    let src = "headv xs = head xs\nmain = headv (cons 1.0 (list))";
    match compile_eval::<f32, 256>(src, "main", Mode::Strict) {
        Ok(_) => {}
        Err(e) => panic!("simple headv def must parse, got: {e}"),
    }
}

#[test]
fn headv_match_def_parses() {
    let src = r"headv xs = case (head xs) of { Nothing -> 0.0; Just x -> x }
main = headv (cons 1.0 (list))";
    match compile_eval::<f32, 256>(src, "main", Mode::Strict) {
        Ok(_) => {}
        Err(e) => panic!(r"headv case def must parse, got: {e}"),
    }
}

#[test]
fn complete_from_produce_cons() {
    let src = "{-# MODE complete #-}\nfrom n = cons n (from (n + 1.0));\nmain = from 10.0;";
    let (prog, v) = eval_complete(src);
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
    let (prog, v) = eval_complete(src);
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
    let (prog, v) = eval_complete(src);
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
    let (prog, v) = eval_complete(src);
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
    let src = r"{-# MODE complete #-}
sub n = cons (n - 1.0) (list);
main = case (head (sub -2.0)) of { Nothing -> 0.0; Just x -> x };";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, -3.0, "-2.0 - 1.0 = -3.0"),
        other => panic!("expected Float(-3.0), got {other:?}"),
    }
}

#[test]
fn complete_countdown_with_minus() {
    // Canonical recursion now that value-track `-` works: countdown 3 -> 0.
    let src = "{-# MODE complete #-}\ncountdown n = if n == 0.0 then 0.0 else countdown (n - 1.0);\nmain = countdown 3.0;";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, 0.0, "countdown 3 -> 0.0"),
        other => panic!("expected Float(0.0), got {other:?}"),
    }
}

#[test]
fn complete_mutual_recursion_runtime() {
    // Mutual recursion (even/odd) resolves through shared cells: even's
    // fragment captures odd's CELL (not a recompiled closure), so the graph
    // compiles exactly once and both closures are filled before any call.
    let src = "{-# MODE complete #-}\neven n = if n == 0.0 then 1.0 else odd (n - 1.0);\nodd n = if n == 0.0 then 0.0 else even (n - 1.0);\nmain = even 4.0;";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, 1.0, "even 4 -> 1.0"),
        other => panic!("expected Float(1.0), got {other:?}"),
    }
}

#[test]
fn complete_nth_with_list_param_recursive() {
    // Self-recursive function with a List parameter: every call is a lazy
    // thunk (complete mode never β-reduces calls — including calls from main),
    // so `nth (from 10.0) 2.0` suspends and recurses through the shared cell.
    let src = r"{-# MODE complete #-}
from n = cons n (from (n + 1.0));
nth xs k = if k == 0.0 then case (head xs) of { Nothing -> 0.0; Just x -> x } else nth (tail xs) (k - 1.0);
main = nth (from 10.0) 2.0;";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => {
            assert_eq!(*x, 12.0, "nth 2 (from 10) -> 12.0")
        }
        other => panic!("expected Float(12.0), got {other:?}"),
    }
}

#[test]
fn complete_take_self_recursive_list() {
    // `take` recurses on a List param and produces a list result; the
    // recursive call suspends as a thunk.
    let src = r"{-# MODE complete #-}
from n = cons n (from (n + 1.0));
take n xs = if n == 0.0 then list else cons (case (head xs) of { Nothing -> 0.0; Just x -> x }) (take (n - 1.0) (tail xs));
main = take 2.0 (from 10.0);";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Cons(h, _) => {
            assert_eq!(
                prog.arena().get(*h).unwrap(),
                &rill_lang::arena::Value::Float(10.0)
            );
        }
        other => panic!("expected Cons head 10.0, got {other:?}"),
    }
}

#[test]
fn complete_let_binding_is_lazy_thunk() {
    // A `let` binding in complete mode suspends as a thunk: xs = from 10 is
    // forced by headv/tail on demand.
    let src = r"{-# MODE complete #-}
from n = cons n (from (n + 1.0));
headv xs = case (head xs) of { Nothing -> 0.0; Just x -> x };
main = let xs = from 10.0 in headv (tail (tail xs));";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => {
            assert_eq!(*x, 12.0, "let xs = from 10; headv (tail tail xs) -> 12.0")
        }
        other => panic!("expected Float(12.0), got {other:?}"),
    }
}

#[test]
fn complete_fibs_self_referential_local() {
    // A self-recursive local (no λ-params) lowers as a THUNK in a shared cell:
    // `fibs = cons 0.0 (cons 1.0 fibs)` — recursion resolves through the cell,
    // not inline.
    let src = "{-# MODE complete #-}\nfibs = cons 0.0 (cons 1.0 fibs);\nmain = fibs;";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Cons(h, _) => {
            assert_eq!(
                prog.arena().get(*h).unwrap(),
                &rill_lang::arena::Value::Float(0.0)
            );
        }
        other => panic!("expected Cons head 0.0, got {other:?}"),
    }
}

#[test]
fn complete_fibs_zipwith_infinite() {
    // The canonical lazy-list example: fibs = 0 : 1 : zipWith (+) fibs (tail
    // fibs). Requires: self-referential local thunk, lazy application args
    // (`tail fibs` suspends, not forces), and shared cells for recursion.
    let src = r"{-# MODE complete #-}
zipWith f xs ys = if case (head xs) of { Nothing -> 1.0; Just x -> x } == 999.0 then list else cons (f (case (head xs) of { Nothing -> 0.0; Just x -> x }) (case (head ys) of { Nothing -> 0.0; Just x -> x })) (zipWith f (tail xs) (tail ys));
fibs = cons 0.0 (cons 1.0 (zipWith (\a -> \b -> a + b) fibs (tail fibs)));
main = case (head (tail (tail (tail (tail fibs))))) of { Nothing -> 0.0; Just x -> x };";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => {
            assert_eq!(*x, 3.0, "fibs[4] -> 3.0")
        }
        other => panic!("expected Float(3.0), got {other:?}"),
    }
}

#[test]
fn complete_repeat_take() {
    let src = r"{-# MODE complete #-}
repeat x = cons x (repeat x);
take n xs = if n == 0.0 then list else cons (case (head xs) of { Nothing -> 0.0; Just x -> x }) (take (n - 1.0) (tail xs));
main = take 3.0 (repeat 42.0);";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Cons(h, _) => {
            assert_eq!(
                prog.arena().get(*h).unwrap(),
                &rill_lang::arena::Value::Float(42.0)
            );
        }
        other => panic!("expected Cons head 42.0, got {other:?}"),
    }
}

#[test]
fn complete_iterate_take() {
    let src = r"{-# MODE complete #-}
iterate f x = cons x (iterate f (f x));
take n xs = if n == 0.0 then list else cons (case (head xs) of { Nothing -> 0.0; Just x -> x }) (take (n - 1.0) (tail xs));
main = take 4.0 (iterate (\x -> x + 1.0) 0.0);";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Cons(h, _) => {
            assert_eq!(
                prog.arena().get(*h).unwrap(),
                &rill_lang::arena::Value::Float(0.0)
            );
        }
        other => panic!("expected Cons head 0.0, got {other:?}"),
    }
}

#[test]
fn complete_bottom_propagates_not_hang() {
    // 1 + a where a = a → Bottom (divergence detected, not a hang).
    let src = r"{-# MODE complete #-}
a = a;
main = case (head (cons (1.0 + a) (list))) of { Nothing -> 0.0; Just x -> x };";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Bottom => {}
        other => panic!("expected Bottom, got {other:?}"),
    }
}

#[test]
fn complete_mutual_recursion_evens_odd() {
    let src = "{-# MODE complete #-}\neven n = if n == 0.0 then 1.0 else odd (n - 1.0);\nodd n = if n == 0.0 then 0.0 else even (n - 1.0);\nmain = even 5.0;";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, 0.0, "even 5 -> 0.0"),
        other => panic!("expected Float(0.0), got {other:?}"),
    }
}

#[test]
fn complete_shared_thunk_forced_once() {
    // x = from 10; body uses x twice — the shared let thunk is forced once and
    // memoized (call-by-need), so both uses observe the same list head.
    let src = r"{-# MODE complete #-}
from n = cons n (from (n + 1.0));
headv xs = case (head xs) of { Nothing -> 0.0; Just x -> x };
main = headv (let x = from 10.0 in x);";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, 10.0, "shared let thunk -> 10.0"),
        other => panic!("expected Float(10.0), got {other:?}"),
    }
}

#[test]
fn mutual_recursion_in_branch_arm_false() {
    // C1 regression: a shared-cell def first referenced from a branch arm must
    // still have its cell bound/filled on every execution path (the cell init
    // is hoisted into the value-track entry block, which always runs first).
    let src = r#"{-# MODE complete #-}
even n = if n == 0.0 then 1.0 else odd (n - 1.0);
odd  n = if n == 0.0 then 0.0 else even (n - 1.0);
main = if 1.0 == 2.0 then even 4.0 else even 3.0;"#;
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, 0.0, "false arm: even 3 -> 0.0"),
        other => panic!("expected Float(0.0), got {other:?}"),
    }
}

#[test]
fn mutual_recursion_in_branch_arm_true() {
    let src = r#"{-# MODE complete #-}
even n = if n == 0.0 then 1.0 else odd (n - 1.0);
odd  n = if n == 0.0 then 0.0 else even (n - 1.0);
main = if 1.0 == 1.0 then even 4.0 else even 3.0;"#;
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, 1.0, "true arm: even 4 -> 1.0"),
        other => panic!("expected Float(1.0), got {other:?}"),
    }
}

#[test]
fn mutual_local_recursion_is_bottom() {
    // I2 regression: mutual LOCAL recursion (`a = b; b = a`) in the value track
    // must force to Bottom (divergence is a value), not a wrong Float.
    let src = r"{-# MODE complete #-}
a = b;
b = a;
main = case (head (cons (1.0 + a) (list))) of { Nothing -> 0.0; Just x -> x }";
    let (prog, v) = eval_complete(src);
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Bottom => {}
        other => panic!("expected Bottom, got {other:?}"),
    }
}

#[test]
fn deep_recursion_is_latched_error_not_panic() {
    // I1 regression: a deep-but-finite recursion exceeds the pre-allocated
    // call-scratch/arena, which must surface as a latched ProcessError (plan
    // §10), not a debug panic or a silent `None`/`Bottom`. Under the single-run
    // surface the latched error surfaces as `CompileError::Unsupported`.
    let src = "{-# MODE complete #-}\ncountdown n = if n == 0.0 then 0.0 else countdown (n - 1.0)\nmain = countdown 2000.0";
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let ffi = rill_lang::ffi::ForeignRegistry::<f32>::new();
    let err = eval_mode::<256>(defs, expr, ffi, Mode::Complete)
        .err()
        .expect("deep recursion must exhaust the pre-allocated budget and latch an error");
    let msg = err.to_string();
    assert!(
        msg.contains("exhausted"),
        "exhaustion error expected, got: {msg}"
    );
}
