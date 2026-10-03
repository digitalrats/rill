//! BackendFactory — constructor registry for I/O backends (moved from rill-graph).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rill_adrift::backend_factory::BackendFactory;
use rill_core::io::{BackendMeta, IoDriver, IoResult};

struct DummyDriver;

impl IoDriver for DummyDriver {
    fn set_callback(&self, _cb: Box<dyn FnMut(&rill_core::time::ClockTick)>) {}

    fn run(&self, _running: Arc<AtomicBool>) -> IoResult<()> {
        Ok(())
    }

    fn stop(&self) -> IoResult<()> {
        Ok(())
    }
}

#[test]
fn rill_io_backends_are_active_sampler_is_passive() {
    let mut f = BackendFactory::new();
    f.register("null", BackendMeta::active(), |_| {
        Ok((
            Arc::new(DummyDriver),
            Some(Arc::new(rill_core::io::NullBackend::new(2))),
            Some(Arc::new(rill_core::io::NullBackend::new(2))),
        ))
    });
    f.register("sampler", BackendMeta::passive(), |_| {
        Err("passive backends produce no driver; not constructed via factory".into())
    });
    assert!(f.contains("null"));
    assert_eq!(f.is_active("null"), Some(true));
    assert_eq!(f.is_active("sampler"), Some(false));
    assert_eq!(f.is_active("missing"), None);
}
