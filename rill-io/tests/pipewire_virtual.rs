//! Integration tests for PipeWire backend using virtual null-sink devices.
//!
//! Tests lifecycle (create/stop) and `DeinterleavedView` creation.
//! Full I/O round-trip tests require the graph processing callback flow,
//! which is tested at the graph level.

#[cfg(feature = "pipewire")]
mod pipewire_it {
    use rill_core::io::IoBackend;
    use rill_core::time::ClockTick;
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
        let view = backend.create_view();
        assert!(view.num_input_channels() > 0 || view.num_output_channels() > 0);

        backend.set_process_callback(Box::new(move |_: &ClockTick| {}));
        settle(100);
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

        let view = backend.create_view();
        assert!(view.num_input_channels() > 0 || view.num_output_channels() > 0);

        backend.set_process_callback(Box::new(move |_: &ClockTick| {}));
        let _ = backend.stop();
    }
}
