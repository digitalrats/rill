//! rill-lang built-in bindings — thin aggregation over crate-level registries.

use rill_core::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry};
use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::{ParamValue, ProcessResult};

/// Build a complete builtin registry: DSP primitives, oscillators, complex
/// arithmetic, mixer, EQ, dry/wet, and optionally analog models and FFT nodes.
pub fn full_registry<T: Transcendental + 'static>() -> rill_core::builtin::Registry<T> {
    let mut reg = rill_core::builtin::Registry::new();

    // Always available
    rill_core_dsp::lang::register::register_lang_builtins(&mut reg);
    rill_lang::register::register_core_builtins(&mut reg);
    rill_router::register::register_lang_builtins(&mut reg);
    rill_digital_effects::register::register_lang_builtins(&mut reg);

    // Feature-gated
    #[cfg(feature = "fft")]
    rill_fft::register::register_lang_builtins(&mut reg);
    #[cfg(feature = "analog")]
    rill_core_model::register::register_lang_builtins(&mut reg);
    #[cfg(feature = "analog")]
    rill_analog_effects::register::register_lang_builtins(&mut reg);
    #[cfg(feature = "sampler")]
    rill_sampler::register::register_lang_builtins(&mut reg);

    // IO pass-through nodes (identity — real IO handled by ProgramRunner)
    register_io_nodes(&mut reg);
    // Tape loop pass-through nodes (replaced by tape_bridge in future)
    register_tape_nodes(&mut reg);

    reg
}

/// Register IO graph nodes as identity pass-through built-ins.
/// Real I/O is handled by ProgramRunner — these just pass signal through.
fn register_io_nodes<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig::simple("rill/input", 0, 2, 0, BuiltinKind::Block),
        |_p, _sr| Box::new(IdentityAlgo::<T>::new(2)),
    );
    reg.register_block(
        BuiltinSig::simple("rill/output", 2, 0, 0, BuiltinKind::Block), // 2→0: stereo sink
        |_p, _sr| Box::new(IdentityAlgo::<T>::new(0)),
    );
}

struct IdentityAlgo<T: Transcendental> {
    channels: usize,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Transcendental> IdentityAlgo<T> {
    fn new(channels: usize) -> Self {
        Self {
            channels,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T: Transcendental> Algorithm<T> for IdentityAlgo<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        if let Some(inp) = input {
            let n = inp.len().min(output.len());
            output[..n].copy_from_slice(&inp[..n]);
        } else {
            output.fill(T::ZERO);
        }
        Ok(())
    }
    fn init(&mut self, _sr: f32) {}
    fn reset(&mut self) {}
}

impl<T: Transcendental> BlockBuiltin<T> for IdentityAlgo<T> {
    fn set_param(&mut self, _index: usize, _value: &ParamValue) {}
}

/// Register tape loop pass-through nodes.
fn register_tape_nodes<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig::simple("write_head", 2, 1, 0, BuiltinKind::Block),
        |_p, _sr| Box::new(IdentityAlgo::<T>::new(1)),
    );
    reg.register_block(
        BuiltinSig::simple("read_head", 0, 1, 0, BuiltinKind::Block),
        |_p, _sr| Box::new(IdentityAlgo::<T>::new(1)),
    );
}

/// Build a lofi-capable registry (concrete `f32`).
#[cfg(feature = "lofi")]
pub fn full_registry_f32() -> rill_core::builtin::Registry<f32> {
    let mut reg = full_registry::<f32>();
    rill_lofi::register::register_lang_builtins(&mut reg);
    reg
}
