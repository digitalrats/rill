//! Integration test: AudioInput → AudioOutput graph with real PipeWire.

#[cfg(feature = "pipewire")]
mod graph_pipewire_it {
    use std::time::Duration;

    use rill_core::io::IoBackend;
    use rill_core::traits::{Node, Sink, Source};
    use rill_core::RenderContext;
    use rill_io::{AudioConfig, AudioInput, AudioOutput, PipewireBackend};

    fn settle(ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    #[test]
    fn test_pipewire_lifecycle() {
        const BUF_SZ: usize = 64;

        let config = AudioConfig::default()
            .with_sample_rate(48000)
            .with_buffer_size(BUF_SZ as u32)
            .with_channels(2);

        let backend = PipewireBackend::new(config).unwrap();
        let backend: Box<dyn IoBackend<f32>> = Box::new(backend);

        let mut input = AudioInput::<f32, BUF_SZ>::new();
        settle(100);

        let mut output = AudioOutput::<f32, BUF_SZ>::new();
        output.resolve_backend(backend);

        let ctx = RenderContext::new(0, BUF_SZ as u32, 48000.0);
        input.generate(&ctx, &[], &[]).unwrap();

        let l = input.output_port(0).unwrap().buffer.as_array();
        let r = input.output_port(1).unwrap().buffer.as_array();
        let _ = output.consume(&ctx, &[l, r], &[], &[], &[]);
    }

    #[test]
    fn test_pipewire_ownership() {
        const BUF_SZ: usize = 64;

        let config = AudioConfig::default()
            .with_sample_rate(48000)
            .with_buffer_size(BUF_SZ as u32)
            .with_channels(2);

        let backend = PipewireBackend::new(config).unwrap();
        let backend: Box<dyn IoBackend<f32>> = Box::new(backend);

        let mut input = AudioInput::<f32, BUF_SZ>::new();
        settle(100);

        {
            let mut output = AudioOutput::<f32, BUF_SZ>::new();
            output.resolve_backend(backend);
            let ctx = RenderContext::new(0, BUF_SZ as u32, 48000.0);
            input.generate(&ctx, &[], &[]).unwrap();
            let l = input.output_port(0).unwrap().buffer.as_array();
            let r = input.output_port(1).unwrap().buffer.as_array();
        let _ = output.consume(&ctx, &[l, r], &[], &[], &[]);
        }
    }
}
