use rill_core::buffer::TapeLoop;
use rill_core::traits::MultichannelAlgorithm;
use rill_sampler::tape::lang::register_tape_builtins;

fn reg() -> rill_lang::builtin::Registry<f32> {
    let mut r = rill_lang::builtin::Registry::new();
    register_tape_builtins(&mut r);
    rill_lang::register::register_core_builtins(&mut r);
    r
}

#[test]
fn write_then_read_through_shared_registry() {
    use rill_core::buffer::ResourceRegistry;
    let mut resources = ResourceRegistry::<f32>::new();
    resources.register_buffer("tape_0", Box::new(TapeLoop::<f32>::new(1024).unwrap()));
    let reg = reg();

    let ws = "tape_0 = TapeLoop 1024\nmain = (_, _) :> write_head tape_0 0.5 0.3";
    let wp =
        rill_lang::parser::parse(&rill_lang::lexer::tokenize(ws).unwrap(), ws.as_bytes()).unwrap();
    let mut weng =
        rill_lang::compile_program_with_resources::<f32, 256>(&wp, &reg, 44100.0, &mut resources)
            .unwrap();

    let rs = "tape_0 = TapeLoop 1024\nmain = read_head tape_0 0.1";
    let rp =
        rill_lang::parser::parse(&rill_lang::lexer::tokenize(rs).unwrap(), rs.as_bytes()).unwrap();
    let mut reng =
        rill_lang::compile_program_with_resources::<f32, 256>(&rp, &reg, 44100.0, &mut resources)
            .unwrap();

    let dry = [1.0f32; 64];
    let fb = [0.0f32; 64];
    let mut wout = [0.0f32; 64];
    for _ in 0..40 {
        MultichannelAlgorithm::process(&mut weng, &[&dry, &fb], &mut [&mut wout[..]]).unwrap();
    }
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut reng, &[], &mut [&mut out[..]]).unwrap();
    assert!(
        out.iter().any(|v| v.abs() > 1e-6),
        "read head must see the write head's samples"
    );
    assert!(out.iter().all(|v| v.is_finite()));
}
