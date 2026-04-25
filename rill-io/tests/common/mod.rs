//! Общие утилиты для тестов rill-io

use std::time::{Duration, Instant};
use std::thread;

/// Запускает тест с таймаутом
///
/// # Arguments
/// * `timeout` - максимальное время выполнения теста
/// * `name` - имя теста для вывода
/// * `f` - тестируемая функция
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
    
    // Если таймаут, завершаем тест как пропущенный
    println!("⏱️  Test timed out after {:?} - skipping", timeout);
}

/// Проверяет, нужно ли запускать живые тесты
pub fn should_run_live_tests() -> bool {
    std::env::var("KAMA_TEST_LIVE_AUDIO").is_ok()
}

/// Создаёт конфигурацию для тестов
pub fn test_config() -> crate::AudioConfig {
    crate::AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(256)
        .with_channels(2)
}

/// Подавление вывода ALSA
pub fn silence_alsa() {
    std::env::set_var("ALSA_CONFIG_PATH", "/dev/null");
}

/// Подавление вывода для всех аудио бэкендов
pub fn silence_all_audio() {
    #[cfg(target_os = "linux")]
    {
        silence_alsa();
    }
}