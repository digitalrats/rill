//! # ModularSystem — modular program host
//!
//! A [`ModularSystemDef`] describes one or more racks. Each rack owns a
//! rill-lang program (`main : (String…) -> IO ()`) that the IO runner executes
//! to completion on the rack's own thread, plus optional control modules
//! (servos, sensors, clocks) registered through the patchbay module factory.

use std::collections::HashMap;
use std::sync::Arc;
#[cfg(feature = "serialization")]
use std::sync::Mutex;

#[cfg(feature = "serialization")]
use rill_core::queues::CommandEnum;
#[cfg(feature = "serialization")]
use rill_core_actor::ActorRef;
use rill_core_actor::ActorSystem;
use rill_patchbay::module_factory::ModuleFactory;

#[cfg(feature = "serialization")]
use crate::modular::serialization::ModularSystemDef;
#[cfg(feature = "serialization")]
use crate::modular::serialization::ModuleDef;

mod case;
mod config;
#[cfg(feature = "serialization")]
pub mod serialization;
pub use case::RackCase;
pub use config::ModularConfig;

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
/// Errors that can occur during modular system setup and operation.
pub enum ModularError {
    /// Error during program compilation or execution.
    Program(String),
    /// Error during rack initialization or operation.
    Rack(String),
}

// ============================================================================
// ModularSystem struct
// ============================================================================

/// A modular program host that manages one or more [`RackCase`] instances.
///
/// Each rack runs its own rill-lang program and owns its control modules
/// (automatons, servos, sensors). The system provides shared infrastructure:
/// the actor system and a module factory for custom rack modules.
pub struct ModularSystem<const BUF: usize = 64> {
    #[cfg_attr(not(feature = "serialization"), allow(dead_code))]
    actor_system: Arc<ActorSystem>,
    module_factory: ModuleFactory,
    cases: HashMap<String, RackCase<BUF>>,
    config: ModularConfig,
}

impl<const BUF: usize> ModularSystem<BUF> {
    /// Create a new `ModularSystem` with the given configuration.
    pub fn new(config: ModularConfig) -> Self {
        let mut module_factory = ModuleFactory::new();
        crate::registration::register_modules(&mut module_factory);
        Self {
            module_factory,
            actor_system: Arc::new(ActorSystem::new()),
            cases: HashMap::new(),
            config,
        }
    }

    /// The host-level configuration this system was created with.
    pub fn config(&self) -> &ModularConfig {
        &self.config
    }

    /// Access the module factory for registering custom rack module types before launch.
    pub fn module_factory_mut(&mut self) -> &mut ModuleFactory {
        &mut self.module_factory
    }

    /// Launch — compile and run every rack's program, register rack modules.
    #[cfg(feature = "serialization")]
    pub fn launch(mut self, def: &ModularSystemDef) -> Result<Self, ModularError> {
        for rd in &def.racks {
            log::info!(
                "rill-adrift: launching rack '{}' — {} program bytes, {} modules",
                rd.name,
                rd.program.len(),
                rd.modules.len()
            );

            let sys = self.actor_system.clone();
            let modules: Arc<Mutex<HashMap<String, ActorRef<CommandEnum>>>> =
                Arc::new(Mutex::new(HashMap::new()));

            // 1. Rack actor — forwards commands to the registered modules.
            let m = modules.clone();
            let actor_ref = sys.spawn_detached(
                &format!("rack_{}", rd.name),
                move || {
                    Box::new(move |msg: CommandEnum| {
                        for module_ref in m.lock().unwrap().values() {
                            module_ref.send(msg.clone());
                        }
                    })
                },
                1,
            );

            let parent_ref = actor_ref.clone();
            let case = RackCase::new(rd.name.clone(), def.sample_rate, actor_ref, Vec::new());
            self.cases.insert(rd.name.clone(), case);

            // 2. Run the rack's program on its own thread. The thread compiles
            //    the program itself (a `RillProgram` is not `Send`) and reports
            //    the compile result back, so a bad program fails `launch`.
            let program = rd.program.clone();
            let args = rd.args.clone();
            let rack_name = rd.name.clone();
            let sample_rate = def.sample_rate;
            let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();

            if let Some(case) = self.cases.get_mut(&rd.name) {
                let rack_name = rack_name.clone();
                case.start(move |_running| {
                    #[cfg(feature = "lofi")]
                    let registry = crate::lang_builtins::full_registry_f32();
                    #[cfg(not(feature = "lofi"))]
                    let registry = crate::lang_builtins::full_registry::<f32>();

                    let mut runner = match rill_lang::compile_runner::<f32, BUF>(
                        &program,
                        &registry,
                        sample_rate,
                    ) {
                        Ok(runner) => {
                            let _ = ready_tx.send(Ok(()));
                            runner
                        }
                        Err(e) => {
                            log::error!("rill-adrift: rack '{}' compile: {e}", rack_name);
                            let _ = ready_tx.send(Err(e.to_string()));
                            return;
                        }
                    };
                    log::info!("rill-adrift: rack '{}' program compiled", rack_name);
                    runner.set_args(&args);

                    match runner.run() {
                        Ok(()) => log::info!("rill-adrift: rack '{}' program finished", rack_name),
                        Err(e) => log::error!("rill-adrift: rack '{}' run: {e}", rack_name),
                    }
                    for line in runner.stdout() {
                        log::info!("rill-adrift: rack '{}': {line}", rack_name);
                    }
                    for line in runner.stderr() {
                        log::warn!("rill-adrift: rack '{}': {line}", rack_name);
                    }
                });
            }

            match ready_rx.recv() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(ModularError::Program(e)),
                Err(e) => {
                    return Err(ModularError::Program(format!(
                        "rack '{}' program thread: {e}",
                        rack_name
                    )))
                }
            }

            // 3. Build modules via ModuleFactory.
            for module_def in &rd.modules {
                let pb_def = to_pb_module(module_def);
                match self.module_factory.construct(
                    &pb_def,
                    &rd.automatons,
                    &self.actor_system,
                    &parent_ref,
                    #[cfg(feature = "debug")]
                    None,
                ) {
                    Ok(actor_ref) => {
                        modules
                            .lock()
                            .unwrap()
                            .insert(pb_def.type_name().to_string(), actor_ref);
                    }
                    Err(e) => log::warn!(
                        "rill-adrift: rack '{}' module construct '{}': {e}",
                        rd.name,
                        pb_def.type_name()
                    ),
                }
            }
        }

        log::info!(
            "rill-adrift: system launched with {} rack(s)",
            def.racks.len()
        );

        Ok(self)
    }

    /// Stop all processing — joins every rack's program thread.
    pub fn stop(&mut self) {
        log::info!("rill-adrift: stopping system");
        for (name, case) in self.cases.iter_mut() {
            log::info!("rill-adrift: stopping rack '{}'", name);
            case.stop();
        }
        log::info!("rill-adrift: system stopped");
    }
}

/// Convert the adrift [`ModuleDef`] to the patchbay [`ModuleDef`].
#[cfg(feature = "serialization")]
fn to_pb_module(m: &ModuleDef) -> rill_patchbay::module_def::ModuleDef {
    match m {
        ModuleDef::Clock(c) => rill_patchbay::module_def::ModuleDef::Clock(c.clone()),
        ModuleDef::Servo(s) => rill_patchbay::module_def::ModuleDef::Servo(s.clone()),
        ModuleDef::Sensor(s) => rill_patchbay::module_def::ModuleDef::Sensor(s.clone()),
        ModuleDef::Custom { type_name, params } => rill_patchbay::module_def::ModuleDef::Custom {
            type_name: type_name.clone(),
            params: params.clone(),
        },
    }
}

impl<const BUF: usize> Drop for ModularSystem<BUF> {
    fn drop(&mut self) {
        self.stop();
    }
}
