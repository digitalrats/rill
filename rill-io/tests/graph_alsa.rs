//! Integration test: ALSA backend with snd-aloop.
//!
//! Requires `sudo modprobe snd-aloop` for the loopback device.
//! Direct hardware (`hw:2,0`) is held by PipeWire on this system.
//! The `default` PCM goes through pipewire-pulse which may deadlock
//! when pulse plugin tries to connect from a test process.

#[cfg(feature = "alsa")]
mod graph_alsa_it {
    use std::process::Command;
    use std::time::Duration;

    use rill_io::{AlsaBackend, AudioBackend, AudioConfig};

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
            eprintln!("SKIP: snd-aloop not loaded (try: sudo modprobe snd-aloop)");
            return;
        }

        let config = AudioConfig::default()
            .with_sample_rate(48000)
            .with_buffer_size(256)
            .with_channels(2)
            .with_output_device("hw:Loopback,0,0");

        let mut backend = AlsaBackend::new(config).unwrap();
        backend.init().unwrap();
        backend.start().unwrap();
        std::thread::sleep(Duration::from_millis(200));

        let written = backend.write(&[0.0f32; 512]).unwrap();
        assert_eq!(written, 512);

        std::thread::sleep(Duration::from_millis(50));
        backend.stop().unwrap();
    }
}
