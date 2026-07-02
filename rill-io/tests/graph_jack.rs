//! Integration test: JACK backend lifecycle with new IoBackend API.
//!
//! May produce audible click if jack ports auto-connect.
//! Use `pw-loopback` or a null-sink to isolate.

#[cfg(feature = "jack")]
mod graph_jack_it {
    use std::time::Duration;

    use rill_core::io::{IoCapture, IoDriver, IoPlayback};
    use rill_core::time::ClockTick;
    use rill_io::{AudioConfig, JackBackend};

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

        let config = AudioConfig::default()
            .with_sample_rate(48000)
            .with_buffer_size(64)
            .with_channels(2);

        let backend = JackBackend::new(config).unwrap();
        settle(300);

        assert!(backend.num_input_channels() > 0 || backend.num_output_channels() > 0);

        backend.set_process_callback(Box::new(move |_: &ClockTick| {}));
        let _ = backend.stop();
        drop(backend);
    }

    #[test]
    fn test_jack_ownership_drop() {
        if !has_jack() {
            eprintln!("SKIP: JACK not available");
            return;
        }

        let config = AudioConfig::default()
            .with_sample_rate(48000)
            .with_buffer_size(64)
            .with_channels(2);

        let backend = JackBackend::new(config).unwrap();
        settle(300);

        assert!(backend.num_input_channels() > 0 || backend.num_output_channels() > 0);

        let _ = backend.stop();
        drop(backend);
    }
}
