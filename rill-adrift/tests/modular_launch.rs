//! `ModularSystem::launch` runs each rack's rill-lang program through the IO
//! runner and reports compile, module-construction, and (via the case) run-time
//! failures instead of starting a dead rack.

#[cfg(feature = "serialization")]
use rill_adrift::modular::serialization::{ModularSystemDef, ModuleDef, RackDef};
#[cfg(feature = "serialization")]
use rill_adrift::modular::{ModularConfig, ModularError, ModularSystem};

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
        matches!(err, ModularError::Program(_)),
        "expected a Program error, got {err:?}"
    );
}

/// A module that cannot be constructed must fail the launch: reporting success
/// while the rack has no working module would hide the failure.
#[cfg(feature = "serialization")]
#[test]
fn launch_reports_a_module_construct_error() {
    let mut rack = RackDef::new(
        "noop",
        "main = do { write (Just \"hi\") stdout; return () };",
    );
    rack.modules.push(ModuleDef::Custom {
        type_name: "no_such_module".to_string(),
        params: Default::default(),
    });
    let err = match ModularSystem::<256>::new(ModularConfig::default()).launch(&system_def(rack)) {
        Ok(_) => panic!("an unconstructible module must fail the launch"),
        Err(e) => e,
    };
    assert!(
        matches!(err, ModularError::Rack(_)),
        "expected a Rack error, got {err:?}"
    );
}

/// Compilation is checked before modules are constructed, so a program that does
/// not compile is reported even when a module is also unconstructible.
#[cfg(feature = "serialization")]
#[test]
fn launch_reports_a_compile_error_before_a_module_error() {
    let mut rack = RackDef::new("broken", "main = (");
    rack.modules.push(ModuleDef::Custom {
        type_name: "no_such_module".to_string(),
        params: Default::default(),
    });
    let err = match ModularSystem::<256>::new(ModularConfig::default()).launch(&system_def(rack)) {
        Ok(_) => panic!("the launch must fail"),
        Err(e) => e,
    };
    assert!(
        matches!(err, ModularError::Program(_)),
        "a compile failure takes precedence, got {err:?}"
    );
}

/// `main = 5.0` is a value, not an executable `IO ()` action: `compile`'s
/// entry gate rejects it, so the launch itself fails. This used to compile and
/// fail at run time; the gate moved the failure to launch.
#[cfg(feature = "serialization")]
#[test]
fn launch_reports_a_non_io_main_as_a_compile_error() {
    let def = system_def(RackDef::new("not_io", "main = 5.0;"));
    let err = match ModularSystem::<256>::new(ModularConfig::default()).launch(&def) {
        Ok(_) => panic!("a non-`IO ()` main must fail the launch"),
        Err(e) => e,
    };
    assert!(
        matches!(err, ModularError::Program(_)),
        "expected a Program error, got {err:?}"
    );
    let msg = format!("{err:?}");
    assert!(
        msg.contains("`main` must have type `IO ()`"),
        "unexpected compile error: {msg}"
    );
}

/// A program that compiles can still fail at run time; the failure is reported
/// by the case, not by `launch` (the rack thread is asynchronous). `stop()`
/// joins the thread and the failure is then readable from the case.
#[cfg(feature = "serialization")]
#[test]
fn launch_surfaces_a_run_error_through_the_case() {
    // The index is out of the array's `0..=2` bounds, so the value track
    // faults when `write` forces the rendered string — after `launch` has
    // already reported success.
    let def = system_def(RackDef::new(
        "bad_index",
        "main = do { write (Just (show_double (arrayIndexFlat \
         (arrayFromList (Pair { first: 0, second: 2 }) [10.0, 20.0, 30.0]) 3))) stdout; \
         return () };",
    ));
    let mut system = ModularSystem::<256>::new(ModularConfig::default())
        .launch(&def)
        .expect("the program compiles, so the launch itself succeeds");
    system.stop();
    let err = system
        .rack_run_error("bad_index")
        .expect("the run failure must be reported by the case");
    assert!(
        err.contains("arrayIndexFlat") && err.contains("out of range"),
        "unexpected run error: {err}"
    );
}
