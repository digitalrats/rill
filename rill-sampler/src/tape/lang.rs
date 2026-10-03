//! Resource-backed tape head builtins: `write_head` (2-in: dry + feedback,
//! 1-out, writes the shared tape) and `read_head` (0-in, 1-out, reads a delayed
//! tap). They reference a named shared buffer via the generic resource registry.
//!
//! Two registration paths coexist during the SP-3b transition:
//! - [`register_tape_builtins`] — the legacy name-based registry (graph duplex
//!   path; the shared `ResourceRegistry` is handed to both sub-programs).
//! - [`register_tape_ffi`] — the new `tape_loop` path: heads take a `Tape f32`
//!   resource param whose value is a tape INDEX into the program's
//!   `Vec<SharedCell>`; the factories receive the cell's writer/reader handles.

use rill_core::math::Transcendental;
use rill_core::traits::Algorithm;
use rill_lang::builtin::Registry;

use crate::tape::read_head::ReadHead;
use crate::tape::write_head::WriteHead;

/// Register the tape write/read head builtins (legacy factory-only path used by
/// the graph-compile/duplex path). Signatures come from the FFI catalog; this
/// registers only the factories.
pub fn register_tape_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    reg.register_resource_multichannel_block(
        "write_head",
        |_signal_ins, p, sr, registry, resource| {
            let mut wh = WriteHead::<T, 64>::new(sr);
            wh.set_delay_time(p[0] as f32);
            wh.set_feedback(p[1] as f32);
            if let Some(writer) = registry.writer(resource) {
                wh.set_writer(writer);
            }
            Box::new(wh)
        },
    );

    reg.register_resource_block("read_head", |p, sr, registry, resource| {
        let mut rh = ReadHead::<T, 64>::new();
        rh.set_delay(p[0] as f32);
        rh.init(sr);
        if let Some(reader) = registry.reader(resource) {
            rh.set_reader(reader);
        }
        Box::new(rh)
    });
}

/// Register the tape write/read head builtins as FFI resource factories (the
/// `tape_loop` path). The `Tape f32` resource param resolves to a tape index;
/// the build hands the factories the shared cell's writer/reader handles.
pub fn register_tape_ffi<T: Transcendental + 'static>(
    ffi: &mut rill_lang::ffi::ForeignRegistry<T>,
) {
    ffi.register_resource_multichannel_block(
        "write_head",
        |_signal_ins, p, sr, writer, _reader| {
            let mut wh = WriteHead::<T, 64>::new(sr);
            wh.set_delay_time(p[0] as f32);
            wh.set_feedback(p[1] as f32);
            wh.set_writer(writer);
            Box::new(wh)
        },
    );
    ffi.register_resource_block("read_head", |p, sr, _writer, reader| {
        let mut rh = ReadHead::<T, 64>::new();
        rh.set_delay(p[0] as f32);
        rh.init(sr);
        rh.set_reader(reader);
        Box::new(rh)
    });
}

impl<T: Transcendental, const B: usize> rill_lang::builtin::BlockBuiltin<T> for ReadHead<T, B> {}
impl<T: Transcendental, const B: usize> rill_lang::builtin::MultichannelBlockBuiltin<T>
    for WriteHead<T, B>
{
}
