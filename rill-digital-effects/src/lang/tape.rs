use rill_core::builtin::{BuiltinKind, BuiltinSig, ParamType, Registry};
use rill_core::math::Transcendental;
use rill_core::traits::Algorithm;

/// Register the tape write/read head built-ins as resource-backed blocks.
///
/// These reference a named [`TapeLoop`](rill_core::buffer::TapeLoop) resolved
/// through the resource registry at compile time: the write head acquires the
/// unique `TapeWriter`, each read head clones a `TapeReader`.
///
/// Signal inputs are wired via the enclosing combinator; the symbolic resource
/// reference is a positional `ParamType::Resource` argument.
pub fn register_tape_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    reg.register_resource_block(
        BuiltinSig {
            name: "write_head",
            params: vec![
                ParamType::Signal,
                ParamType::Resource,
                ParamType::Float,
                ParamType::Float,
            ],
            signal_outs: 1,
            kind: BuiltinKind::Block,
            param_names: vec!["delay_time", "feedback"],
        },
        |p, sr, registry, resource| {
            let mut wh = crate::WriteHead::<T, 64>::new(sr);
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
            let mut rh = crate::ReadHead::<T, 64>::new();
            rh.set_delay(p[0] as f32);
            rh.init(sr);
            if let Some(reader) = registry.reader(resource) {
                rh.set_reader(reader);
            }
            Box::new(rh)
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::buffer::{ResourceRegistry, TapeLoop};
    use rill_core::traits::ProcessResult;

    #[test]
    fn tape_heads_share_a_tape_via_registry() {
        let mut reg = Registry::<f32>::new();
        register_tape_builtins(&mut reg);

        let mut resources = ResourceRegistry::<f32>::new();
        resources.register_tape("tape_0", TapeLoop::<f32>::new(1024).unwrap());

        let mut wh = reg
            .get("write_head")
            .unwrap()
            .build_resource_block(&[0.5, 0.3], 44100.0, &mut resources, "tape_0")
            .expect("write_head resource block");
        let mut rh = reg
            .get("read_head")
            .unwrap()
            .build_resource_block(&[0.5], 44100.0, &mut resources, "tape_0")
            .expect("read_head resource block");

        let input = [1.0f32, 2.0, 3.0, 4.0];
        let mut pass = [0.0f32; 4];
        Algorithm::process(wh.as_mut(), Some(&input), &mut pass).expect("write");

        let mut out = [0.0f32; 4];
        Algorithm::process(rh.as_mut(), None, &mut out).expect("read");
        assert!(out.iter().all(|v| v.is_finite()));
    }
}
