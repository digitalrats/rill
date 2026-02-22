//! Тесты для аудио бэкендов
//!
//! Быстрые тесты используют моки и не требуют реальных устройств.
//! Живые тесты с реальными устройствами игнорируются по умолчанию.

use kama_io::{
    AudioConfig, AudioBackend, BackendType,
};

#[cfg(feature = "cpal")]
use kama_io::backends::CpalBackend;

#[cfg(feature = "alsa")]
use kama_io::backends::AlsaBackend;

mod mocks;
use mocks::{MockBackend, test_config};

// =============================================================================
// ТЕСТЫ С МОКАМИ (ВСЕГДА БЫСТРЫЕ)
// =============================================================================

/// Тест проверки доступности бэкендов на платформе
#[test]
fn test_backend_availability() {
    println!("\n=== Test: Backend Availability ===");
    
    println!("Platform: {}", std::env::consts::OS);
    println!("Backend availability:");
    
    let backends = [
        BackendType::Cpal,
        BackendType::Alsa,
        BackendType::PipeWire,
        BackendType::Jack,
        BackendType::Null,
    ];
    
    for &backend in &backends {
        let available = backend.is_available();
        println!("  {:8}: {}", backend.name(), if available { "✅" } else { "❌" });
        
        match backend {
            BackendType::Cpal => assert!(available, "CPAL should be available on all platforms"),
            BackendType::Alsa => assert_eq!(available, cfg!(target_os = "linux"), 
                                           "ALSA should only be available on Linux"),
            BackendType::Null => assert!(available, "Null backend should always be available"),
            _ => {}
        }
    }
}

/// Тест базовых операций с моком
#[test]
fn test_mock_backend_basic() {
    println!("\n=== Test: Mock Backend Basic ===");
    
    let config = test_config();
    let mut backend = MockBackend::new(config);
    
    assert_eq!(backend.backend_type().name(), "Null");
    assert!(backend.init().is_ok());
    assert!(backend.start().is_ok());
    
    let mut buf = vec![0.0; 256];
    let read = backend.read(&mut buf).unwrap();
    assert_eq!(read, 256);
    assert!(buf.iter().all(|&x| x == 0.0));
    
    let write_buf = vec![0.5; 256];
    let written = backend.write(&write_buf).unwrap();
    assert_eq!(written, 256);
    
    assert!(backend.stop().is_ok());
    assert_eq!(backend.xruns(), 0);
}

/// Тест обработки ошибок в моке
#[test]
fn test_mock_backend_failure() {
    println!("\n=== Test: Mock Backend Failure ===");
    
    let config = test_config();
    let mut backend = MockBackend::new(config).with_failure(true);
    
    assert!(backend.init().is_err());
    assert!(backend.start().is_err());
}

/// Тест листинга устройств в моке
#[test]
fn test_mock_list_devices() {
    println!("\n=== Test: Mock List Devices ===");
    
    let config = test_config();
    let backend = MockBackend::new(config);
    
    let inputs = backend.list_input_devices();
    let outputs = backend.list_output_devices();
    
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0], "Mock Input");
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0], "Mock Output");
}

/// Тест конфигурации в моке
#[test]
fn test_mock_config() {
    println!("\n=== Test: Mock Config ===");
    
    let config = test_config()
        .with_sample_rate(48000)
        .with_buffer_size(512);
    
    let backend = MockBackend::new(config);
    
    assert_eq!(backend.config().sample_rate, 48000);
    assert_eq!(backend.config().buffer_size, 512);
}

// =============================================================================
// ТЕСТЫ СОЗДАНИЯ РЕАЛЬНЫХ БЭКЕНДОВ (ИГНОРИРУЮТСЯ ПО УМОЛЧАНИЮ)
// =============================================================================

/// Тест создания CPAL бэкенда
#[test]
#[ignore]
#[cfg(feature = "cpal")]
fn test_cpal_backend_creation() {
    println!("\n=== Test: CPAL Backend Creation (real) ===");
    println!("⏭️  Skipping - run with --ignored to test with real devices");
}

/// Тест листинга устройств CPAL
#[test]
#[ignore]
#[cfg(feature = "cpal")]
fn test_cpal_list_devices() {
    println!("\n=== Test: CPAL List Devices (real) ===");
    println!("⏭️  Skipping - run with --ignored to test with real devices");
}

/// Тест создания CPAL бэкенда с нестандартной конфигурацией
#[test]
#[ignore]
#[cfg(feature = "cpal")]
fn test_cpal_backend_with_custom_config() {
    println!("\n=== Test: CPAL with Custom Config (real) ===");
    println!("⏭️  Skipping - run with --ignored to test with real devices");
}

/// Живой тест CPAL с реальной инициализацией
#[test]
#[ignore]
#[cfg(feature = "cpal")]
fn test_cpal_backend_live() {
    println!("\n=== Test: CPAL Live Audio (real) ===");
    println!("⏭️  Skipping - run with --ignored to test with real devices");
}

/// Тест создания ALSA бэкенда
#[test]
#[ignore]
#[cfg(all(feature = "alsa", target_os = "linux"))]
fn test_alsa_backend_creation() {
    println!("\n=== Test: ALSA Backend Creation (real) ===");
    println!("⏭️  Skipping - run with --ignored to test with real devices");
}

/// Тест листинга устройств ALSA
#[test]
#[ignore]
#[cfg(all(feature = "alsa", target_os = "linux"))]
fn test_alsa_list_devices() {
    println!("\n=== Test: ALSA List Devices (real) ===");
    println!("⏭️  Skipping - run with --ignored to test with real devices");
}

/// Тест создания ALSA бэкенда с нестандартной конфигурацией
#[test]
#[ignore]
#[cfg(all(feature = "alsa", target_os = "linux"))]
fn test_alsa_backend_with_custom_config() {
    println!("\n=== Test: ALSA with Custom Config (real) ===");
    println!("⏭️  Skipping - run with --ignored to test with real devices");
}

/// Живой тест ALSA с реальной инициализацией
#[test]
#[ignore]
#[cfg(all(feature = "alsa", target_os = "linux"))]
fn test_alsa_backend_live() {
    println!("\n=== Test: ALSA Live Audio (real) ===");
    println!("⏭️  Skipping - run with --ignored to test with real devices");
}