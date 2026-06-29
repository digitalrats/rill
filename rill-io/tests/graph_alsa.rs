//! Integration test: ALSA backend with snd-aloop.

#[cfg(feature = "alsa")]
mod graph_alsa_it {
    use std::process::Command;

    use rill_core::io::{IoCapture, IoDriver, IoPlayback};
    use rill_core::time::ClockTick;
    use rill_io::{AlsaBackend, AudioConfig};

    fn alsa_loopback_available() -> bool {
        Command::new("aplay")
            .args(["-l"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("Loopback"))
            .unwrap_or(false)
    }

    #[test]
    fn test_alsa_loopback() {
        if !alsa_loopback_available() {
            eprintln!("SKIP: snd-aloop not loaded");
            return;
        }

        let config = AudioConfig::default()
            .with_sample_rate(48000)
            .with_buffer_size(256)
            .with_channels(2)
            .with_output_device("hw:Loopback,0,0");

        let backend = AlsaBackend::new(config).unwrap();

        assert!(backend.num_input_channels() > 0 || backend.num_output_channels() > 0);
        let _tick = ClockTick::new(0, 256, 48000.0, "test".into());
        backend.set_process_callback(Box::new(move |_: &ClockTick| {}));

        let _ = backend.stop();
    }
}
