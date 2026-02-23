/// Базовые тесты для трейтов
use kama_core_traits::*;

// Тестовая структура для проверки трейтов
#[derive(Debug)]
struct TestClock;

impl Clock for TestClock {
    fn sample_rate(&self) -> f64 {
        44100.0
    }
    fn position_samples(&self) -> u64 {
        0
    }
    fn advance(&self, _samples: u64) -> u64 {
        0
    }
    fn reset(&self) {}
}

#[test]
fn test_clock_trait() {
    let clock = TestClock;
    assert_eq!(clock.sample_rate(), 44100.0);
    assert_eq!(clock.position_seconds(), 0.0);
}

#[test]
fn test_param_value() {
    let float_val = ParamValue::Float(0.5);
    match float_val {
        ParamValue::Float(f) => assert_eq!(f, 0.5),
        _ => panic!("Wrong variant"),
    }
}
