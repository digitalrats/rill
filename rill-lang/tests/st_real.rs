//! Real ST: `runST` runs its body in a fresh, isolated region and the `STRef`
//! family allocates, reads, writes, and modifies cells inside it at runtime.
//!
//! These tests exercise the whole pipeline (infer → reduce → lower → executor)
//! through `compile`, asserting the resulting value outputs.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::arena::Value;
use rill_lang::compile;

/// Compile `src`, run one tick, and return the single value output's arena
/// value (cloned out of the program's arena).
fn run_value_output(src: &str) -> Value {
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let outputs = prog.value_outputs();
    assert_eq!(outputs.len(), 1, "expected exactly one value output");
    let v = outputs[0].expect("the value output must be bound");
    prog.arena().get(v).unwrap().clone()
}

#[test]
fn stref_read_write_modify_roundtrip() {
    // One region: allocate a cell at 0.0, write 42.0, modify by +1.0, then read
    // it back. The whole `do` block runs through `bind` of `Monad (ST s)`.
    let v = run_value_output(
        "{-# MODE complete #-}\nmain = runST (\\s -> do { r <- newSTRef 0.0; writeSTRef r 42.0; modifySTRef r (\\x -> x + 1.0); readSTRef r });",
    );
    assert_eq!(v, Value::Float(43.0));
}

#[test]
fn two_runst_regions_are_isolated() {
    // Each `runST` allocates a FRESH region: the two cells never share state,
    // so the pair holds each region's own initial value.
    let mut prog = compile::<f32>(
        "{-# MODE complete #-}\nmain = Pair { first: runST (\\s -> do { r <- newSTRef 1.0; readSTRef r }), second: runST (\\s -> do { r <- newSTRef 2.0; readSTRef r }) };",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].expect("the value output must be bound");
    let fields = match prog.arena().get(v).unwrap() {
        Value::Record(fields) => fields.clone(),
        other => panic!("expected the Pair record, got {other:?}"),
    };
    assert_eq!(fields.len(), 2, "the Pair must have two fields");
    let first = prog.arena().get(fields[0]).unwrap().clone();
    let second = prog.arena().get(fields[1]).unwrap().clone();
    assert_eq!(first, Value::Float(1.0), "region A kept its own cell");
    assert_eq!(second, Value::Float(2.0), "region B kept its own cell");
}

#[test]
fn stref_escaping_region_is_compile_error() {
    // `return r` makes the region result an `STRef s a`, which mentions the
    // phantom `s`: it must not escape the region.
    let err = match compile::<f32>(
        "{-# MODE complete #-}\nmain = runST (\\s -> do { r <- newSTRef 0.0; return r });",
    ) {
        Ok(_) => panic!("an escaping STRef must be a compile error"),
        Err(err) => err,
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("escape") || msg.contains("phantom"),
        "expected an escaping-region type error, got: {msg}"
    );
}
