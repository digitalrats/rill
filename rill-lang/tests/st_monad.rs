//! ST monad: `runST` sugar. R3 keeps the check at the INFERENCE level — rank-N
//! application rejects a region result that escapes the phantom. The runtime
//! assertion (a value tick writing the erased region's value into a value
//! output) is DEFERRED to Task B4, which unifies the Signal/Value tracks; today
//! a value-typed `runST` body infers at `Rate::Signal` and the value-output
//! path is not the R3 concern.

use rill_lang::lexer::tokenize;
use rill_lang::mode::Mode;
use rill_lang::parser::parse;
use rill_lang::types::infer::infer_program;

#[test]
fn runst_region_compiles_ok() {
    // `runST (fn s -> 1.0)` infers successfully: the phantom `s` is erased and
    // the region types as its body (`1.0` is a signal constant today, hence
    // `main` is `Rate::Signal`). The rank-N application must ACCEPT a result
    // that does not mention `s`.
    let src = r"{-# MODE complete #-}
main = runST (\s -> 1.0);";
    let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
    let r = infer_program(&p, Mode::Complete);
    assert!(
        r.is_ok(),
        "runST over a non-escaping body must compile: {:?}",
        r.err(),
    );
}

#[test]
fn runst_region_rejects_escaping_phantom() {
    // A result whose type mentions `s` must NOT escape the region:
    // check-escaping rejects it at compile time.
    let src = r"{-# MODE complete #-}
main = runST (\s -> (s));";
    let p = tokenize(src).unwrap();
    let prog = parse(&p, src.as_bytes()).unwrap();
    let r = infer_program(&prog, Mode::Complete);
    assert!(r.is_err(), "ST result must not escape the runST region");
}
