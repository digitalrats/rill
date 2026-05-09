//! Integration test: ALSA backend with snd-aloop.

#[cfg(feature = "alsa")]
mod graph_alsa_it {
    use std::process::Command;

    use rill_core::io::IoBackend;
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
        let _ = backend.write(&[&[0.0f32; 256][..]]);
        let _ = backend.stop();
    }
}
