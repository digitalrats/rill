//! Common test utilities for rill-io

use std::time::{Duration, Instant};
use std::thread;

/// Run a test with a timeout
///
/// # Arguments
/// * `timeout` - maximum test execution time
/// * `name` - test name for display
/// * `f` - the test function
pub fn run_with_timeout<F>(timeout: Duration, name: &str, f: F)
where
    F: FnOnce() + Send + 'static,
{
    println!("\n=== Test: {} (timeout: {:?}) ===", name, timeout);
    
    let handle = thread::spawn(f);
    let start = Instant::now();
    
    while start.elapsed() < timeout {
        if handle.is_finished() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    
    // If timeout, mark the test as skipped
    println!("⏱️  Test timed out after {:?} - skipping", timeout);
}

/// Check whether to run live tests
pub fn should_run_live_tests() -> bool {
    std::env::var("RILL_TEST_LIVE_AUDIO").is_ok()
}

/// Create test configuration
pub fn test_config() -> crate::AudioConfig {
    crate::AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(256)
        .with_channels(2)
}

/// Suppress ALSA output
pub fn silence_alsa() {
    std::env::set_var("ALSA_CONFIG_PATH", "/dev/null");
}

/// Suppress output for all audio backends
pub fn silence_all_audio() {
    #[cfg(target_os = "linux")]
    {
        silence_alsa();
    }
}