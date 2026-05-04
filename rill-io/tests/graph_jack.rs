//! Integration test: AudioInput → AudioOutput with JACK (pipewire-jack bridge).
//!
//! May produce audible click if jack ports auto-connect to the default sink.
//! Use `pw-loopback` or a null-sink to isolate.

#[cfg(feature = "jack")]
mod graph_jack_it {
    use std::time::Duration;

    use rill_core::ClockTick;
    use rill_core::traits::{SignalNode, Sink, Source};
    use rill_io::audio_io::{AudioIo, AudioIoPtr};
    use rill_io::{AudioConfig, AudioInput, AudioOutput, JackBackend};

    fn has_jack() -> bool {
        // Check that JACK libraries are available at the system level.
        // JackBackend::new is not conclusive — it doesn't connect until start().
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

        let backend = JackBackend::new(config).unwrap();
        let _ = backend.start();
        let ptr = AudioIoPtr::from_ref(&backend as &dyn AudioIo);

        let mut input = AudioInput::<f32, BUF_SZ>::new();
        input.set_backend(Box::new(backend));
        settle(300);

        let mut output = AudioOutput::<f32, BUF_SZ>::new();
        output.set_backend(ptr);

        let tick = ClockTick::new(0, BUF_SZ as u32, 48000.0);
        input.generate(&tick, &[], &[]).unwrap();

        let l = input.output_port(0).unwrap().buffer.as_array();
        let r = input.output_port(1).unwrap().buffer.as_array();
        let signal_inputs: [&[f32; BUF_SZ]; 2] = [l, r];
        output.consume(&tick, &signal_inputs, &[], &[], &[]).unwrap();

        input.stop();
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

        let backend = JackBackend::new(config).unwrap();
        let _ = backend.start();
        let ptr = AudioIoPtr::from_ref(&backend as &dyn AudioIo);

        let mut input = AudioInput::<f32, BUF_SZ>::new();
        input.set_backend(Box::new(backend));
        settle(300);

        {
            let mut output = AudioOutput::<f32, BUF_SZ>::new();
            output.set_backend(ptr);
            let tick = ClockTick::new(0, BUF_SZ as u32, 48000.0);
            input.generate(&tick, &[], &[]).unwrap();
            let l = input.output_port(0).unwrap().buffer.as_array();
            let r = input.output_port(1).unwrap().buffer.as_array();
            let signal_inputs: [&[f32; BUF_SZ]; 2] = [l, r];
            output.consume(&tick, &signal_inputs, &[], &[], &[]).unwrap();
        }

        input.stop();
    }
}
