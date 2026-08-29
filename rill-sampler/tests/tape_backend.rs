use rill_sampler::tape::backend::{ReadHeadConfig, TapeBackend, TapeBackendSpec, WriteHeadConfig};

#[test]
fn write_then_read_delivers_delayed_taps() {
    let spec = TapeBackendSpec {
        name: "tape_0".into(),
        capacity: 1024,
        write: WriteHeadConfig { feedback: 0.0 },
        reads: vec![
            ReadHeadConfig { delay: 0.001 },
            ReadHeadConfig { delay: 0.002 },
        ],
    };
    let mut tape = TapeBackend::<f32>::new(&spec, 44100.0);
    let block = [1.0f32; 64];
    for _ in 0..40 {
        tape.write_block(&block);
    }
    let mut a = [0.0f32; 64];
    let mut b = [0.0f32; 64];
    let mut outs: [&mut [f32]; 2] = [&mut a, &mut b];
    tape.read_blocks(&mut outs);
    assert!(a.iter().any(|v| v.abs() > 1e-6));
    assert!(b.iter().any(|v| v.abs() > 1e-6));
    assert!(a.iter().chain(b.iter()).all(|v| v.is_finite()));
}
