use rill_lang::compile_with_ffi_channels;
use rill_lang::ffi::ForeignRegistry;

#[test]
fn compiles_simple_graph() {
    let result = compile_with_ffi_channels::<f32>(
        "main i1 = do { writeOutput [i1 * 0.5]; return () }",
        &ForeignRegistry::<f32>::new(),
        44100.0,
        1,
        1,
    );
    assert!(result.is_ok(), "should compile simple graph");
}

#[test]
fn single_algorithm_works() {
    let result = compile_with_ffi_channels::<f32>(
        "main i1 = do { writeOutput [i1 * 0.5]; return () }",
        &ForeignRegistry::<f32>::new(),
        44100.0,
        1,
        1,
    );
    assert!(result.is_ok(), "single algorithm should compile");
}

#[test]
fn engine_executes_and_produces_output() {
    let mut prog = compile_with_ffi_channels::<f32>(
        "main i1 = do { writeOutput [i1 * 0.5]; return () }",
        &ForeignRegistry::<f32>::new(),
        44100.0,
        1,
        1,
    )
    .unwrap();

    let mut output = [0.0f32; 8];
    let input = [2.0f32; 8];
    use rill_core::traits::Algorithm;
    prog.process(Some(&input), &mut output).unwrap();
    assert_eq!(output[0], 1.0, "2.0 * 0.5 = 1.0");
}

#[test]
fn engine_with_param_compiles_and_processes() {
    // IO model: `?level` is a control param (default 0.0); the main input is a
    // Buffer.
    let mut prog = compile_with_ffi_channels::<f32>(
        "main i1 = do { writeOutput [i1 * ?level]; return () }",
        &ForeignRegistry::<f32>::new(),
        44100.0,
        1,
        1,
    )
    .unwrap();

    let mut output = [0.0f32; 8];
    let input = [2.0f32; 8];
    use rill_core::traits::Algorithm;
    prog.process(Some(&input), &mut output).unwrap();
    assert_eq!(output[0], 0.0, "level default is 0.0 => 2.0*0.0 = 0.0");
}
