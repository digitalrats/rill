//! Digital filters for Kama Audio
//!
//! This crate provides digital filter implementations:
//! - Biquad filter (LowPass, HighPass, BandPass, Notch, Peak, LowShelf, HighShelf, AllPass)
//! - More filters coming soon: OnePole, StateVariable, Comb, etc.

#![warn(missing_docs)]

pub mod biquad;

// Re-export main types from kama-dsp-common
pub use biquad::{BiquadFactory, BiquadFilter};
pub use kama_dsp_common::filter::{Filter, FilterFactory, FilterType};

#[cfg(test)]
mod tests {
    use super::*;
    use float_cmp::approx_eq;
    use kama_core_traits::AudioNode;

    #[test]
    fn test_biquad_lowpass() {
        let mut filter = BiquadFilter::new(FilterType::LowPass, 1000.0, 0.707, 0.0);
        filter.init(44100.0);

        // Подаём постоянный сигнал 1.0 и ждём стабилизации
        let mut steady_state: f32 = 0.0;
        for i in 0..1000 {
            let output = filter.process_sample(1.0);
            if i > 900 {
                // Последние 100 семплов
                steady_state += output;
            }
        }
        steady_state /= 100.0; // среднее за последние 100 семплов

        println!("DC steady state output: {}", steady_state);
        // DC должен проходить почти без изменений (чуть меньше 1.0 из-за потерь)
        assert!(
            steady_state > 0.95 && steady_state < 1.05,
            "DC steady state should be near 1.0, got {}",
            steady_state
        );

        // Сброс фильтра
        filter.reset();

        // Подаём высокочастотный сигнал
        let mut max_output: f32 = 0.0;
        let num_samples = 5000;

        for i in 0..num_samples {
            let t = i as f32 / 44100.0;
            let input = (2.0 * std::f32::consts::PI * 5000.0 * t).sin();
            let output = filter.process_sample(input);

            // Пропускаем первые 1000 семплов для стабилизации
            if i > 1000 {
                max_output = max_output.max(output.abs());
            }
        }

        println!("High frequency (5kHz) max output: {}", max_output);
        assert!(
            max_output < 0.5,
            "High frequency should be attenuated, got {}",
            max_output
        );
    }

    #[test]
    fn test_biquad_peak() {
        let mut filter = BiquadFilter::new(FilterType::Peak, 1000.0, 2.0, 6.0);
        filter.init(44100.0);

        // Test that parameters are set correctly
        assert_eq!(filter.cutoff(), 1000.0);
        assert_eq!(filter.q(), 2.0);
        assert_eq!(filter.gain_db(), 6.0);
    }

    #[test]
    fn test_biquad_factory() {
        let factory = BiquadFactory;
        let filter = factory.create_filter(FilterType::LowPass, 500.0, 1.0, 0.0);

        assert_eq!(filter.filter_type(), FilterType::LowPass);
        assert_eq!(filter.cutoff(), 500.0);
    }
}
