//! `ModularSystem::launch` runs each rack's rill-lang program through the IO
//! runner and reports compile failures instead of starting a dead rack.

#[cfg(feature = "serialization")]
use rill_adrift::modular::serialization::{ModularSystemDef, RackDef};
#[cfg(feature = "serialization")]
use rill_adrift::modular::{ModularConfig, ModularSystem};

#[cfg(feature = "serialization")]
fn system_def(rack: RackDef) -> ModularSystemDef {
    ModularSystemDef {
        format_version: "1.0".to_string(),
        sample_rate: 44_100.0,
        block_size: 256,
        racks: vec![rack],
        description: None,
    }
}

#[cfg(feature = "serialization")]
#[test]
fn launch_runs_a_program_to_completion() {
    let def = system_def(RackDef::new(
        "noop",
        "main = do { write (Just \"hi\") stdout; return () };",
    ));
    let mut system = ModularSystem::<256>::new(ModularConfig::default())
        .launch(&def)
        .expect("a valid program must launch");
    system.stop();
}

#[cfg(feature = "serialization")]
#[test]
fn launch_reports_a_program_compile_error() {
    let def = system_def(RackDef::new("broken", "main = ("));
    let err = match ModularSystem::<256>::new(ModularConfig::default()).launch(&def) {
        Ok(_) => panic!("a broken program must fail the launch"),
        Err(e) => e,
    };
    assert!(
        matches!(err, rill_adrift::modular::ModularError::Program(_)),
        "expected a Program error, got {err:?}"
    );
}

#[cfg(feature = "serialization")]
#[test]
fn launch_binds_rack_args() {
    // `main name` binds the first rack argument; a missing argument defaults to
    // the empty string rather than failing.
    let mut rack = RackDef::new(
        "greeter",
        "main name = do { write (Just (concat_string \"hi \" name)) stdout; return () };",
    );
    rack.args.push("rill".to_string());
    let def = system_def(rack);
    let mut system = ModularSystem::<256>::new(ModularConfig::default())
        .launch(&def)
        .expect("a bound program must launch");
    system.stop();
}
