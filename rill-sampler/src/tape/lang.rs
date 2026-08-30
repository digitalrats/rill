//! Resource-backed tape head builtins: `write_head` (2-in: dry + feedback,
//! 1-out, writes the shared tape) and `read_head` (0-in, 1-out, reads a delayed
//! tap). They reference a named shared buffer via the generic resource registry.

use rill_core::builtin::{BuiltinKind, BuiltinSig, ParamType, Registry};
use rill_core::math::Transcendental;
use rill_core::traits::Algorithm;

use crate::tape::read_head::ReadHead;
use crate::tape::write_head::WriteHead;

/// Register the tape write/read head builtins.
pub fn register_tape_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    reg.register_resource_multichannel_block(
        BuiltinSig {
            name: "write_head",
            params: vec![
                ParamType::Signal,
                ParamType::Signal,
                ParamType::Resource,
                ParamType::Float,
                ParamType::Float,
            ],
            signal_outs: 1,
            kind: BuiltinKind::Block,
            param_names: vec!["delay_time", "feedback"],
        },
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

    reg.register_resource_block(
        BuiltinSig {
            name: "read_head",
            params: vec![ParamType::Resource, ParamType::Float],
            signal_outs: 1,
            kind: BuiltinKind::Block,
            param_names: vec!["delay"],
        },
        |p, sr, registry, resource| {
            let mut rh = ReadHead::<T, 64>::new();
            rh.set_delay(p[0] as f32);
            rh.init(sr);
            if let Some(reader) = registry.reader(resource) {
                rh.set_reader(reader);
            }
            Box::new(rh)
        },
    );
}

impl<T: Transcendental, const B: usize> rill_core::builtin::BlockBuiltin<T> for ReadHead<T, B> {}
impl<T: Transcendental, const B: usize> rill_core::builtin::MultichannelBlockBuiltin<T>
    for WriteHead<T, B>
{
}
