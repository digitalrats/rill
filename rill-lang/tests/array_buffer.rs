//! The `FixedBuffer` <-> `Array Int Float` bridge through the LANGUAGE surface.
//!
//! `arrayFromBuffer` copies a signal block into an `Int`-indexed immutable array
//! (bounds `(0, len - 1)`); `bufferFromArray` copies an array back into a
//! program buffer, zero-padding a short array to the live block and faulting
//! cleanly on one that does not fit.
//!
//! The flat primitives are pinned by `tests/array_storage.rs`; this target
//! verifies the same contract END TO END — the samples are compared against a
//! directly-generated buffer, not just against each other, so a bridge that
//! mangles both directions symmetrically cannot pass.

use rill_core::traits::ProcessError;
use rill_lang::arena::Value;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::program::RillProgram;
use rill_lang::register;

/// The block length the bridge operates on (the program's `BUF_SIZE`).
const BLOCK: usize = 256;

/// Compile `src` against the foreign generator registry (the buffer generators
/// `sine`/`square` are FFI builtins).
fn buffer_program(src: &str) -> RillProgram<f32, 256> {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    rill_lang::compile_with_ffi::<f32>(src, &ffi, 44_100.0)
        .unwrap_or_else(|e| panic!("compile failed: {e}\nsrc:\n{src}"))
}

/// Run `src` and return the live block of its single BUFFER output.
fn run_buffer(src: &str) -> Vec<f32> {
    let mut prog = buffer_program(src);
    prog.run_value()
        .unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    let mut got = [0.0f32; BLOCK];
    assert!(
        prog.copy_output_samples(0, &mut got),
        "the output of `{src}` must be a buffer"
    );
    got.to_vec()
}

/// Run `src` expecting a clean latched fault, returning its detail.
fn fault(src: &str) -> String {
    let mut prog = buffer_program(src);
    match prog.run_value() {
        Ok(()) => panic!("expected a fault, but the program ran\nsrc:\n{src}"),
        Err(ProcessError::Processing(msg)) => msg,
        Err(other) => panic!("expected a latched processing fault, got {other:?}\nsrc:\n{src}"),
    }
}

/// The single value output of an already-run program.
fn output(prog: &RillProgram<f32, 256>) -> Value {
    let outputs = prog.value_outputs();
    assert_eq!(outputs.len(), 1, "expected exactly one value output");
    let v = outputs[0].expect("the value output must be bound");
    prog.arena()
        .get(v)
        .expect("the output ref must resolve")
        .clone()
}

fn pair_values(prog: &RillProgram<f32, 256>, v: &Value) -> (Value, Value) {
    let fields = match v {
        Value::Record(fields) if fields.len() == 2 => fields,
        other => panic!("expected a Pair record, got {other:?}"),
    };
    (
        prog.arena().get(fields[0]).expect("first resolves").clone(),
        prog.arena()
            .get(fields[1])
            .expect("second resolves")
            .clone(),
    )
}

/// The `Float` payloads of the single `List` value output of `src`.
fn run_elems(src: &str) -> Vec<f64> {
    let mut prog = buffer_program(src);
    prog.run_value()
        .unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    match output(&prog) {
        Value::List { elems } => elems
            .iter()
            .map(|r| match prog.arena().get(*r) {
                Some(Value::Float(f)) => *f,
                other => panic!("expected a Float element, got {other:?}"),
            })
            .collect(),
        other => panic!("expected a List, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// `arrayFromBuffer`
// ---------------------------------------------------------------------------

#[test]
fn array_from_buffer_is_zero_indexed_over_the_live_block() {
    // The bounds are `(0, len - 1)`: the whole block, no offset.
    let mut prog = buffer_program("main = bounds (arrayFromBuffer (sine 440.0 0.5 0.0));");
    prog.run_value().expect("the program must run");
    let (lo, hi) = pair_values(&prog, &output(&prog));
    assert_eq!(lo, Value::Int(0), "the first sample is index 0");
    assert_eq!(
        hi,
        Value::Int(BLOCK as i64 - 1),
        "the last sample is index len - 1"
    );
}

#[test]
fn array_from_buffer_length_is_the_block_length() {
    // 256 samples in, 256 elements out — the bridge copies the LIVE block, not
    // the buffer's capacity.
    assert_eq!(
        run_elems("main = elems (arrayFromBuffer (sine 440.0 0.5 0.0));").len(),
        BLOCK
    );
}

#[test]
fn array_from_buffer_copies_the_actual_samples() {
    // Compare against a DIRECTLY generated buffer: the array's element `i` must
    // be the block's sample `i` (an `f32` widened to `f64`), so the bridge is
    // a copy, not a re-synthesis or a shifted read.
    let direct = run_buffer("main = sine 440.0 0.5 0.0;");
    let elems = run_elems("main = elems (arrayFromBuffer (sine 440.0 0.5 0.0));");
    assert_eq!(elems.len(), direct.len());
    for (i, (array_elem, sample)) in elems.iter().zip(&direct).enumerate() {
        assert_eq!(
            *array_elem,
            f64::from(*sample),
            "element {i} must be the block's sample {i}"
        );
    }
    assert!(
        direct.iter().any(|s| *s != 0.0),
        "the fixture waveform must contain non-zero samples"
    );
}

#[test]
fn array_from_buffer_indexes_both_bounds_through_the_accessor() {
    // `! 0` and `! 255` are the two ends of the bridged array; both must equal
    // the corresponding block samples.
    let direct = run_buffer("main = sine 440.0 0.5 0.0;");
    let mut prog = buffer_program(
        "main = (\\ b -> Pair { first: (arrayFromBuffer b) ! 0, \
          second: (arrayFromBuffer b) ! 255 }) (sine 440.0 0.5 0.0);",
    );
    prog.run_value().expect("the program must run");
    let (first, last) = pair_values(&prog, &output(&prog));
    assert_eq!(first, Value::Float(f64::from(direct[0])));
    assert_eq!(last, Value::Float(f64::from(direct[BLOCK - 1])));
}

// ---------------------------------------------------------------------------
// `bufferFromArray`
// ---------------------------------------------------------------------------

#[test]
fn buffer_from_array_writes_the_elements_and_zero_pads_the_rest() {
    // A three-element array fills the first three samples of the live block; the
    // remaining 253 are zeroed so the block is deterministic. This is the
    // documented T7 behaviour, asserted through `listArray` (the language
    // constructor) rather than the flat primitive.
    let got = run_buffer(
        "main = bufferFromArray (listArray (Pair { first: 0, second: 2 }) [1.5, -2.5, 3.25]);",
    );
    assert_eq!(&got[..3], &[1.5, -2.5, 3.25], "the array fills the head");
    assert!(
        got[3..].iter().all(|s| *s == 0.0),
        "the live block is zero-padded after the array, got {:?}",
        &got[3..8]
    );
}

#[test]
fn buffer_from_array_rejects_an_array_longer_than_the_buffer() {
    // 300 elements do not fit a 256-sample block: a clean capacity fault, not a
    // silent truncation. The oversized array is built through the language
    // surface, so the fault surfaces from the bridge itself.
    let elems: Vec<String> = (0..300).map(|i| format!("{i}.0")).collect();
    let msg = fault(&format!(
        "main = bufferFromArray (listArray (Pair {{ first: 0, second: 299 }}) [{}]);",
        elems.join(", ")
    ));
    assert!(
        msg.contains("bufferFromArray") && msg.contains("capacity"),
        "expected the clean capacity fault, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Round trips
// ---------------------------------------------------------------------------

#[test]
fn buffer_round_trip_preserves_every_sample() {
    // `bufferFromArray (arrayFromBuffer b)` must equal `b` sample for sample.
    let direct = run_buffer("main = sine 440.0 0.5 0.0;");
    let roundtrip = run_buffer("main = bufferFromArray (arrayFromBuffer (sine 440.0 0.5 0.0));");
    assert_eq!(roundtrip, direct, "the bridge must preserve every sample");
}

#[test]
fn array_round_trip_returns_the_array_padded_to_the_block_length() {
    // The other direction: an array -> buffer -> array. `bufferFromArray` always
    // produces a whole live block, so the returning array is `BLOCK` long with
    // the original elements at the head and zeros after them. This pins the
    // documented block-length contract end to end (it is NOT a shape-preserving
    // round trip, and pretending otherwise would hide the padding).
    let elems =
        run_elems("main = elems (arrayFromBuffer (bufferFromArray (listArray (Pair { first: 0, second: 2 }) [1.0, 2.0, 3.0])));");
    assert_eq!(elems.len(), BLOCK, "the block length, not the array length");
    assert_eq!(&elems[..3], &[1.0, 2.0, 3.0], "the head survives");
    assert!(
        elems[3..].iter().all(|e| *e == 0.0),
        "the short array is zero-padded to the block length"
    );
}
