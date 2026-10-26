// Deep non-tail recursion through the runner: behavior parity guard for the
// `run_blocks` extraction (Task 1). This asserts the SAME result after the
// unified block-walker refactor — it is a parity check, not a stack-safety
// test (the trampoline is a deferred, separate step).
use rill_lang::compile;
use rill_lang::ffi::ForeignRegistry;

#[test]
fn deep_non_tail_recursion_same_result() {
    let src = r#"{-# MODE complete #-}
take n xs = if n == 0.0 then list else cons (case (head xs) of { Nothing -> 0.0; Just x -> x }) (take (n - 1.0) (tail xs));
from n = cons n (from (n + 1.0));
main = do { xs <- fromList (take 3.0 (from 10.0));
    ys <- toList xs;
    write (Just (fold (\acc -> \x -> concat_string acc (show_double x)) "" ys)) stdout;
    return () };
"#;
    let mut r = compile::<f32, 256>(src, ForeignRegistry::new()).unwrap();
    r.run().unwrap();
    assert_eq!(r.stdout(), ["10.011.012.0"]);
}
