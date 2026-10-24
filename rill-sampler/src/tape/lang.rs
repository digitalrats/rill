//! Resource-backed tape head builtins: `write_head` (2-in: dry + feedback,
//! 1-out, writes the shared tape) and `read_head` (0-in, 1-out, reads a delayed
//! tap). They reference a shared tape cell by index.
//!
//! [`register_tape_ffi`] is the only registration path: heads take a `Tape f32`
//! resource param whose value is a tape INDEX into the program's
//! `Vec<SharedCell>`; the factories receive the cell's writer/reader handles.

use rill_core::math::Transcendental;
use rill_core::traits::Algorithm;

use crate::tape::read_head::ReadHead;
use crate::tape::write_head::WriteHead;

/// Register the tape write/read head builtins as FFI resource factories (the
/// `tape_loop` path). The `Tape f32` resource param resolves to a tape index;
/// the build hands the factories the shared cell's writer/reader handles.
pub fn register_tape_ffi<T: Transcendental + 'static>(
    ffi: &mut rill_lang::ffi::ForeignRegistry<T>,
) {
    ffi.register_resource_multichannel_block(
        "write_head",
        |_signal_ins, p, sr, writer, _reader, _alloc| {
            let mut wh = WriteHead::<T, 64>::new(sr);
            wh.set_delay_time(p[0] as f32);
            wh.set_feedback(p[1] as f32);
            wh.set_writer(writer);
            Box::new(wh)
        },
    );
    ffi.register_resource_block("read_head", |p, sr, _writer, reader, _alloc| {
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
