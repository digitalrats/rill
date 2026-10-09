use rill_core::traits::MultichannelAlgorithm;

fn run(src: &str) -> rill_lang::program_engine::ProgramEngine<f32, 256> {
    let reg = rill_lang::builtin::Registry::<f32>::new();
    rill_lang::compile_graph::<f32, 256>(src, &reg, 44100.0).unwrap()
}

#[test]
fn runst_region_erases_phantom() {
    // `runST (fn s -> e)` executes `e` and erases the phantom type `s`;
    // the value flowing out is `e`'s result. s is only a type-level token.
    let mut p = run("{-# MODE complete #-}\nmain = runST (fn s -> 1.0);");
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut p, &[], &mut [&mut out]);
    assert_eq!(out[0], 1.0);
}

#[test]
fn runst_region_rejects_escaping_phantom() {
    // A result whose type mentions `s` must NOT escape the region:
    // check-escaping rejects it at compile time.
    let src = "{-# MODE complete #-}\nmain = runST (fn s -> (s));";
    let p = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&p, src.as_bytes()).unwrap();
    let r = rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Complete);
    assert!(r.is_err(), "ST result must not escape the runST region");
}
