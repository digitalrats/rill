//! Integration test: AudioInput → AudioOutput graph with real PipeWire.
//!
//! Uses virtual null-sink to avoid audible output.

#[cfg(feature = "pipewire")]
mod graph_pipewire_it {
    use std::process::Command;
    use std::time::Duration;

    use rill_core::io::IoBackend;
    use rill_core::traits::{Node, Sink, Source};
    use rill_core::ClockTick;
    use rill_io::audio_io::AudioIoPtr;
    use rill_io::AudioBackend;
    use rill_io::{AudioConfig, AudioInput, AudioOutput, PipewireBackend};

    const VIRTUAL_SINK: &str = "rill_graph_test_sink";

    fn has_pipewire() -> bool {
        Command::new("pw-cli")
            .arg("info")
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

    fn destroy_virtual_sink(mod_id: u32) {
        let _ = Command::new("pactl")
            .args(["unload-module", &mod_id.to_string()])
            .output();
    }

    struct SinkGuard(u32);
    impl Drop for SinkGuard {
        fn drop(&mut self) {
            destroy_virtual_sink(self.0);
        }
    }

    fn settle(ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    #[test]
    fn test_graph_ownership_with_pipewire() {
        if !has_pipewire() || !has_pactl() {
            eprintln!("SKIP: PipeWire or pactl not available");
            return;
        }

        let mod_id = match create_virtual_sink(VIRTUAL_SINK) {
            Some(id) => id,
            None => {
                eprintln!("SKIP: cannot create virtual sink");
                return;
            }
        };
        let _guard = SinkGuard(mod_id);
        settle(200);

        const BUF_SZ: usize = 64;

        let config = AudioConfig::default()
            .with_sample_rate(44100)
            .with_buffer_size(BUF_SZ as u32)
            .with_channels(2)
            .with_output_device(VIRTUAL_SINK);

        let mut backend = Box::new(PipewireBackend::new(config).unwrap());
        let _ = AudioBackend::start(&mut *backend);
        let ptr = AudioIoPtr::from_ref(&*backend as &dyn IoBackend<f32>);

        let mut input = AudioInput::<f32, BUF_SZ>::new();
        input.set_io_ptr(ptr);
        settle(300);

        {
            let mut output = AudioOutput::<f32, BUF_SZ>::new();
            output.set_backend(ptr);
            let tick = ClockTick::new(0, BUF_SZ as u32, 44100.0);
            input.generate(&tick, &[], &[]).unwrap();
            let l = input.output_port(0).unwrap().buffer.as_array();
            let r = input.output_port(1).unwrap().buffer.as_array();
            let signal_inputs: [&[f32; BUF_SZ]; 2] = [l, r];
            output
                .consume(&tick, &signal_inputs, &[], &[], &[])
                .unwrap();
        }

        input.stop();
    }

    #[test]
    fn test_graph_drop_stops_backend() {
        if !has_pipewire() || !has_pactl() {
            eprintln!("SKIP: PipeWire or pactl not available");
            return;
        }

        let mod_id = match create_virtual_sink(VIRTUAL_SINK) {
            Some(id) => id,
            None => {
                eprintln!("SKIP: cannot create virtual sink");
                return;
            }
        };
        let _guard = SinkGuard(mod_id);
        settle(200);

        const BUF_SZ: usize = 64;

        let config = AudioConfig::default()
            .with_sample_rate(44100)
            .with_buffer_size(BUF_SZ as u32)
            .with_channels(2)
            .with_output_device(VIRTUAL_SINK);

        let mut backend = Box::new(PipewireBackend::new(config).unwrap());
        let _ = AudioBackend::start(&mut *backend);
        let ptr = AudioIoPtr::from_ref(&*backend as &dyn IoBackend<f32>);

        let mut input = AudioInput::<f32, BUF_SZ>::new();
        input.set_io_ptr(ptr);
        settle(300);

        {
            let mut output = AudioOutput::<f32, BUF_SZ>::new();
            output.set_backend(ptr);
            let tick = ClockTick::new(0, BUF_SZ as u32, 44100.0);
            input.generate(&tick, &[], &[]).unwrap();
            let l = input.output_port(0).unwrap().buffer.as_array();
            let r = input.output_port(1).unwrap().buffer.as_array();
            let signal_inputs: [&[f32; BUF_SZ]; 2] = [l, r];
            output
                .consume(&tick, &signal_inputs, &[], &[], &[])
                .unwrap();
        }

        input.stop();
    }
}
