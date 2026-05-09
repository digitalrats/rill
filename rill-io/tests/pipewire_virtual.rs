//! Integration tests for PipeWire backend using virtual null-sink devices.

#[cfg(feature = "pipewire")]
mod pipewire_it {
    use rill_core::io::IoBackend;
    use rill_io::{AudioConfig, BackendType, PipewireBackend};
    use std::process::Command;
    use std::time::Duration;

    const VIRTUAL_SINK: &str = "rill_test_sink";

    fn has_pipewire() -> bool {
        Command::new("pw-cli")
            .arg("info")
            .arg("all")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn has_pactl() -> bool {
        Command::new("pactl")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn create_virtual_sink(name: &str) -> Option<u32> {
        let out = Command::new("pactl")
            .args([
                "load-module",
                "module-null-sink",
                &format!("sink_name={name}"),
                &format!("sink_properties=node.name={name}"),
            ])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        s.parse::<u32>().ok()
    }

    fn destroy_virtual_sink(module_id: u32) {
        let _ = Command::new("pactl")
            .args(["unload-module", &module_id.to_string()])
            .output();
    }

    fn settle(ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    struct Cleanup(u32);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            destroy_virtual_sink(self.0);
        }
    }

    #[test]
    fn test_lifecycle() {
        if !has_pipewire() {
            eprintln!("SKIP: PipeWire not available");
            return;
        }

        let config = AudioConfig::default()
            .with_backend(BackendType::PipeWire)
            .with_sample_rate(44100)
            .with_buffer_size(256)
            .with_channels(2);

        let backend = PipewireBackend::new(config).unwrap();
        let buf = [0.0f32; 256];
        let _ = backend.write(&[&buf[..]]);
        settle(100);
        let mut out = [0.0f32; 256];
        let _ = backend.read(&mut [&mut out[..]]);
        let _ = backend.stop();
    }

    #[test]
    fn test_with_virtual_sink() {
        if !has_pipewire() || !has_pactl() {
            eprintln!("SKIP: PipeWire or pactl not available");
            return;
        }

        let mod_id = match create_virtual_sink(VIRTUAL_SINK) {
            Some(id) => id,
            None => {
                eprintln!("SKIP: could not create virtual sink");
                return;
            }
        };

        let _guard = Cleanup(mod_id);
        settle(200);

        let config = AudioConfig::default()
            .with_backend(BackendType::PipeWire)
            .with_sample_rate(44100)
            .with_buffer_size(256)
            .with_channels(2)
            .with_output_device(VIRTUAL_SINK);

        let backend = PipewireBackend::new(config).unwrap();
        settle(300);

        let test_data: Vec<f32> = (0..256).map(|i| (i as f32) / 256.0).collect();
        let _ = backend.write(&[&test_data[..]]);

        settle(300);
        let mut read_buf = vec![0.0f32; 256];
        let _ = backend.read(&mut [&mut read_buf[..]]);
        let _ = backend.stop();
    }
}
