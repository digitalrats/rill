//! Basic tests for rill-router EQ module
use rill_core::{SignalNode, ParamValue, ParameterId};
use rill_core_dsp::filters::{Biquad, FilterParams, FilterType};
use rill_router::{
    BandType, FilterFactory, GraphicEq, GraphicEqProcessor, ParametricEq, ParametricEqProcessor,
};

/// Custom factory for Biquad<f32> that implements FilterFactory
struct BiquadFactory;

impl FilterFactory<Biquad<f32>> for BiquadFactory {
    fn create_filter(
        &self,
        filter_type: FilterType,
        frequency: f32,
        q: f32,
        gain_db: f32,
    ) -> Biquad<f32> {
        Biquad::new(FilterParams {
            filter_type,
            cutoff: frequency,
            q,
            gain_db,
        })
    }
}

#[test]
fn test_parametric_eq_creation() {
    let factory = BiquadFactory;
    let eq = ParametricEq::new(factory, 5, 44100.0);
    assert_eq!(eq.num_bands(), 5);
}

#[test]
fn test_parametric_eq_set_band() {
    let factory = BiquadFactory;
    let mut eq = ParametricEq::new(factory, 3, 44100.0);
    eq.set_band(1, 1000.0, 2.0, 6.0).unwrap();
    assert!(
        eq.get_band_frequency(1).unwrap() > 999.0 && eq.get_band_frequency(1).unwrap() < 1001.0
    );
    assert!(eq.get_band_gain(1).unwrap() > 5.9 && eq.get_band_gain(1).unwrap() < 6.1);
    assert!(eq.get_band_q(1).unwrap() > 1.9 && eq.get_band_q(1).unwrap() < 2.1);
}

#[test]
fn test_graphic_eq_creation() {
    let factory = BiquadFactory;
    let eq = GraphicEq::new_third_octave(factory, 44100.0);
    assert_eq!(eq.num_bands(), 31);
}

#[test]
fn test_graphic_eq_set_gain() {
    let factory = BiquadFactory;
    let mut eq = GraphicEq::new_third_octave(factory, 44100.0);
    eq.set_band_gain(10, 12.0).unwrap();
    // No getter for band gain, just ensure no panic
}

#[test]
fn test_parametric_eq_processor_creation() {
    let processor = ParametricEqProcessor::<f32, 64>::new(44100.0, 4);
    assert_eq!(processor.num_bands(), 4);
    assert_eq!(processor.output_gain, 1.0);
}

#[test]
fn test_parametric_eq_processor_parameters() {
    let mut processor = ParametricEqProcessor::<f32, 64>::new(44100.0, 2);

    // Test output gain
    let param_id = ParameterId::new("output_gain").unwrap();
    processor
        .set_parameter(&param_id, ParamValue::Float(2.5))
        .unwrap();
    assert!((processor.output_gain - 2.5).abs() < 0.001);

    // Test band frequency
    let param_id = ParameterId::new("band_0_freq").unwrap();
    processor
        .set_parameter(&param_id, ParamValue::Float(500.0))
        .unwrap();
    assert!((processor.eq().get_band_frequency(0).unwrap() - 500.0).abs() < 0.001);

    // Test band gain
    let param_id = ParameterId::new("band_1_gain").unwrap();
    processor
        .set_parameter(&param_id, ParamValue::Float(6.0))
        .unwrap();
    assert!((processor.eq().get_band_gain(1).unwrap() - 6.0).abs() < 0.001);

    // Test band enabled
    let param_id = ParameterId::new("band_0_enabled").unwrap();
    processor
        .set_parameter(&param_id, ParamValue::Bool(false))
        .unwrap();
    assert!(!processor.eq().get_band_enabled(0).unwrap());
}

#[test]
fn test_graphic_eq_processor_creation() {
    let processor = GraphicEqProcessor::<f32, 64>::new_third_octave(44100.0);
    assert_eq!(processor.num_bands(), 31);
    assert_eq!(processor.output_gain, 1.0);
}

#[test]
fn test_graphic_eq_processor_parameters() {
    let mut processor = GraphicEqProcessor::<f32, 64>::new_third_octave(44100.0);

    // Test output gain
    let param_id = ParameterId::new("output_gain").unwrap();
    processor
        .set_parameter(&param_id, ParamValue::Float(1.5))
        .unwrap();
    assert!((processor.output_gain - 1.5).abs() < 0.001);

    // Test band gain
    let param_id = ParameterId::new("band_10_gain").unwrap();
    processor
        .set_parameter(&param_id, ParamValue::Float(-3.0))
        .unwrap();
    assert!((processor.eq().get_band_gain(10).unwrap() + 3.0).abs() < 0.001);

    // Test band enabled
    let param_id = ParameterId::new("band_5_enabled").unwrap();
    processor
        .set_parameter(&param_id, ParamValue::Bool(false))
        .unwrap();
    assert!(!processor.eq().get_band_enabled(5).unwrap());
}
