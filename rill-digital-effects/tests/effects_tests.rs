use rill_core::traits::algorithm::Algorithm;
use rill_digital_effects::{Delay, Distortion, DistortionType, Limiter};

#[test]
fn test_delay_algorithm() {
    let mut delay = Delay::<f32, 64>::new(44100.0);
    let mut out = [0.0f32; 64];
    delay.process(None, &mut out).unwrap();
    assert!(out.iter().all(|&x| x == 0.0));
}

#[test]
fn test_delay_with_input() {
    let mut delay = Delay::<f32, 64>::new(44100.0);
    let input = [1.0f32; 64];
    let mut out = [0.0f32; 64];
    delay.process(Some(&input), &mut out).unwrap();
}

#[test]
fn test_distortion_algorithm() {
    let mut dist = Distortion::<f32, 64>::new();
    let input = [0.1f32; 64];
    let mut out = [0.0f32; 64];
    dist.process(Some(&input), &mut out).unwrap();
}

#[test]
fn test_distortion_hard_clip() {
    let mut dist = Distortion::<f32, 64>::with_params(DistortionType::HardClip, 10.0, 1.0);
    let input = [5.0f32; 64];
    let mut out = [0.0f32; 64];
    dist.process(Some(&input), &mut out).unwrap();
    assert!(out.iter().all(|&x| x >= -1.0 && x <= 1.0));
}

#[test]
fn test_limiter_algorithm() {
    let mut lim = Limiter::<f32, 64>::new(44100.0, -6.0, 0.01, 0.05, 1.0);
    let mut out = [0.0f32; 64];
    lim.force_ready();
    let input = [0.1f32; 64];
    lim.process(Some(&input), &mut out).unwrap();
}
