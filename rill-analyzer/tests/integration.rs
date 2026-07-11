#[test]
fn probe_frame_roundtrip() {
    let frame = rill_lang::debug::ProbeFrame {
        value_bits: 1.5_f64.to_bits(),
        block_index: 42,
    };
    assert_eq!(f64::from_bits(frame.value_bits), 1.5);
    assert_eq!(frame.block_index, 42);
}

#[test]
fn cmdstr_roundtrip() {
    let s = rill_lang::debug::CmdStr::<32>::new("SetParameter");
    assert_eq!(s.as_str(), "SetParameter");
}

#[test]
fn debug_control_pause_cont() {
    let dc = rill_lang::debug::DebugControl::new();
    assert!(!dc.global_pause.load(std::sync::atomic::Ordering::Relaxed));
    dc.pause();
    assert!(dc.global_pause.load(std::sync::atomic::Ordering::Relaxed));
    dc.cont();
    assert!(!dc.global_pause.load(std::sync::atomic::Ordering::Relaxed));
}
