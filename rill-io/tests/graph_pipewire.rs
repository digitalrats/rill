//! Integration test: PipeWire backend lifecycle with new IoBackend API.

#[cfg(feature = "pipewire")]
mod graph_pipewire_it {
    use std::time::Duration;

    use rill_core::io::IoBackend;
    use rill_core::time::ClockTick;
    use rill_io::{AudioConfig, PipewireBackend};

    fn settle(ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    #[test]
    fn test_pipewire_lifecycle() {
        let config = AudioConfig::default()
            .with_sample_rate(48000)
            .with_buffer_size(64)
            .with_channels(2);

        let backend = PipewireBackend::new(config).unwrap();
        settle(100);

        // Verify backend creates a valid view
        let view = backend.create_view();
        assert!(view.num_input_channels() > 0 || view.num_output_channels() > 0);

        let tick = ClockTick::new(0, 64, 48000.0, "test".into(), view);
        backend.set_process_callback(Box::new(move |_: &ClockTick| {}));
        let _ = backend.stop();
        // Drop backend — runs Drop cleanup
        drop(backend);
        drop(tick);
    }

    #[test]
    fn test_pipewire_ownership() {
        let config = AudioConfig::default()
            .with_sample_rate(48000)
            .with_buffer_size(64)
            .with_channels(2);

        let backend = PipewireBackend::new(config).unwrap();
        settle(100);

        let view = backend.create_view();
        assert!(view.num_input_channels() > 0 || view.num_output_channels() > 0);

        // Backend can be stopped and dropped
        let _ = backend.stop();
        drop(backend);
    }
}
