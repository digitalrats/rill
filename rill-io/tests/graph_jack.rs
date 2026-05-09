//! Integration test: AudioInput → AudioOutput with JACK (pipewire-jack bridge).
//!
//! May produce audible click if jack ports auto-connect to the default sink.
//! Use `pw-loopback` or a null-sink to isolate.

#[cfg(feature = "jack")]
mod graph_jack_it {
    use std::time::Duration;

    use rill_core::io::IoBackend;
    use rill_core::traits::{Node, Sink, Source};
    use rill_core::ClockTick;
    use rill_io::{AudioConfig, AudioInput, AudioOutput, JackBackend};

    fn has_jack() -> bool {
        std::process::Command::new("pactl")
            .args(["list", "modules"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("pipewire-jack"))
            .unwrap_or(false)
    }

    fn settle(ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    #[test]
    fn test_jack_lifecycle() {
        if !has_jack() {
            eprintln!("SKIP: JACK not available (pipewire-jack not running)");
            return;
        }

        const BUF_SZ: usize = 64;

        let config = AudioConfig::default()
            .with_sample_rate(48000)
            .with_buffer_size(BUF_SZ as u32)
            .with_channels(2);

        let mut backend = JackBackend::new(config).unwrap();
        let backend: Box<dyn rill_core::io::IoBackend<f32>> = Box::new(backend);

        let mut input = AudioInput::<f32, BUF_SZ>::new();
        settle(300);

        let mut output = AudioOutput::<f32, BUF_SZ>::new();
        output.resolve_backend(backend);

        let tick = ClockTick::new(0, BUF_SZ as u32, 48000.0);
        input.generate(&tick, &[], &[]).unwrap();

        let l = input.output_port(0).unwrap().buffer.as_array();
        let r = input.output_port(1).unwrap().buffer.as_array();
        let _ = output.consume(&tick, &[l, r], &[], &[], &[]);
    }

    #[test]
    fn test_jack_ownership_drop() {
        if !has_jack() {
            eprintln!("SKIP: JACK not available");
            return;
        }

        const BUF_SZ: usize = 64;

        let config = AudioConfig::default()
            .with_sample_rate(48000)
            .with_buffer_size(BUF_SZ as u32)
            .with_channels(2);

        let mut backend = JackBackend::new(config).unwrap();
        let backend: Box<dyn rill_core::io::IoBackend<f32>> = Box::new(backend);

        let mut input = AudioInput::<f32, BUF_SZ>::new();
        settle(300);

        {
            let mut output = AudioOutput::<f32, BUF_SZ>::new();
            output.resolve_backend(backend);
            let tick = ClockTick::new(0, BUF_SZ as u32, 48000.0);
            input.generate(&tick, &[], &[]).unwrap();
            let l = input.output_port(0).unwrap().buffer.as_array();
            let r = input.output_port(1).unwrap().buffer.as_array();
            let _ = output.consume(&tick, &[l, r], &[], &[], &[]);
        }
    }
}
