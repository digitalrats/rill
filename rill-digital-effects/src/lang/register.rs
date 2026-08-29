use rill_core::builtin::Registry;
use rill_core::math::Transcendental;

pub fn register_lang_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    super::delay::register_delay_builtins(reg);
    super::distortion::register_distortion_builtins(reg);
    super::limiter::register_limiter_builtins(reg);
}
