//! # ModularSystem — modular signal processing host
//!
//! * GraphDef — signal topology
//! * RackDef — control system (LFO, envelope, sequencer)

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rill_core::traits::ParamValue;
use rill_core_actor::ActorSystem;
use rill_graph::GraphBuilder;
use rill_patchbay::module_factory::ModuleFactory;

#[cfg(feature = "serialization")]
use crate::modular::serialization::ModularSystemDef;
#[cfg(feature = "serialization")]
use crate::modular::serialization::ModuleDef;
#[cfg(feature = "serialization")]
use rill_graph::serialization::GraphDef;

#[cfg(feature = "serialization")]
use rill_core::queues::CommandEnum;
#[cfg(feature = "serialization")]
use rill_core_actor::ActorRef;

use rill_core_actor::Mailbox;

mod case;
mod config;
#[cfg(feature = "serialization")]
pub mod serialization;
pub use case::RackCase;
#[cfg(feature = "serialization")]
pub use config::LaunchConfig;
pub use config::ModularConfig;
// Re-exports.

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
/// Errors that can occur during modular system setup and operation.
pub enum ModularError {
    /// Error during signal graph construction or processing.
    Graph(String),
    /// Error during rack initialization or operation.
    Rack(String),
}

// ============================================================================
// ModularSystem struct
// ============================================================================

/// A modular signal processing host that manages one or more [`RackCase`] instances.
///
/// Each rack has its own signal graph and control modules (automatons, servos, sensors).
/// The system provides shared infrastructure: actor system, backend factory,
/// and a module factory for custom rack modules.
pub struct ModularSystem<const BUF: usize = 64> {
    actor_system: Arc<ActorSystem>,
    module_factory: ModuleFactory,
    cases: HashMap<String, RackCase<BUF>>,
    default_backend: Option<(String, HashMap<String, ParamValue>)>,
    #[allow(dead_code)]
    config: ModularConfig,
    #[cfg(feature = "serialization")]
    tokio_rt: Option<tokio::runtime::Runtime>,
}

impl<const BUF: usize> ModularSystem<BUF> {
    /// Create a new `ModularSystem` with the given configuration.
    pub fn new(config: ModularConfig) -> Self {
        let mut module_factory = ModuleFactory::new();
        crate::registration::register_modules(&mut module_factory);
        let default_backend = config.backend_name.clone().map(|n| {
            let params = config
                .backend_params
                .iter()
                .map(|(k, v)| (k.clone(), str_to_param(v)))
                .collect();
            (n, params)
        });
        Self {
            module_factory,
            default_backend,
            actor_system: Arc::new(ActorSystem::new()),
            cases: HashMap::new(),
            config,
            #[cfg(feature = "serialization")]
            tokio_rt: None,
        }
    }

    /// Set the default I/O backend name and parameters for all graphs.
    pub fn set_default_backend(&mut self, name: &str, params: HashMap<String, ParamValue>) {
        self.default_backend = Some((name.to_string(), params));
    }

    pub(crate) fn create_builder(&self) -> GraphBuilder<f32, BUF> {
        GraphBuilder::new()
    }

    /// Build a `RillGraphEngine` from a `GraphDef` using the rill-lang compilation pipeline.
    pub fn build_engine(
        &self,
        def: &GraphDef,
        buf_size: usize,
    ) -> Result<rill_lang::graph_engine::RillGraphEngine<f32>, Box<dyn std::error::Error>> {
        let mut builder = self.create_builder();
        def.populate(&mut builder)
            .map_err(|e| format!("populate: {e}"))?;

        #[cfg(not(feature = "lofi"))]
        let registry = crate::lang_builtins::full_registry::<f32>();
        #[cfg(feature = "lofi")]
        let registry = crate::lang_builtins::full_registry_f32();
        let ir = builder
            .build_ir(&registry)
            .map_err(|e| format!("build_ir: {e}"))?;

        let scheduled = rill_lang::graph_lower::lower(&ir);

        let programs: Vec<rill_lang::RillProgram<f32>> = ir
            .topo_order
            .iter()
            .filter_map(|name| ir.nodes.get(name))
            .map(|node| rill_lang::RillProgram::<f32>::new(node.ir.clone()))
            .collect();

        let mailbox = Arc::new(Mailbox::new(64));

        Ok(rill_lang::graph_engine::RillGraphEngine::new(
            scheduled, programs, mailbox, buf_size,
        ))
    }

    /// Access the module factory for registering custom rack module types before launch.
    pub fn module_factory_mut(&mut self) -> &mut ModuleFactory {
        &mut self.module_factory
    }

    /// Launch — build graph engines, set up IO, register modules, start processing.
    #[cfg(feature = "serialization")]
    pub fn launch(mut self, def: &ModularSystemDef) -> Result<Self, ModularError> {
        let tokio_rt = tokio::runtime::Runtime::new()
            .map_err(|e| ModularError::Graph(format!("tokio: {e}")))?;
        let _guard = tokio_rt.enter();

        for rd in &def.racks {
            let buf_size = def.block_size.max(64);
            let sys = self.actor_system.clone();
            let gd = rd.graph.clone();

            let modules: Arc<Mutex<HashMap<String, ActorRef<CommandEnum>>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let tasks: Vec<std::thread::JoinHandle<()>> = Vec::new();

            let (graph_tx, graph_rx) = std::sync::mpsc::channel::<ActorRef<CommandEnum>>();

            // 1. Rack actor — forwards messages to registered modules
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
            let mut case = RackCase::new(rd.name.clone(), def.sample_rate, actor_ref, tasks);
            self.cases.insert(rd.name.clone(), case);

            // 2. Build engine + ProgramRunner on signal thread
            let backend_name = self.default_backend.clone();

            // 2. Build engine + ProgramRunner on signal thread
            let backend_name = self.default_backend.clone();
            let registry = crate::lang_builtins::full_registry::<f32>();
            #[cfg(feature = "lofi")]
            let registry = crate::lang_builtins::full_registry_f32();

            if let Some(case) = self.cases.get_mut(&rd.name) {
                let graph_def = gd.clone();
                case.start(move |running| {
                    let mut builder: GraphBuilder<f32, BUF> = GraphBuilder::new();
                    if let Err(e) = graph_def.populate(&mut builder) {
                        log::error!("graph populate: {e}");
                        return;
                    }
                    let ir = match builder.build_ir(&registry) {
                        Ok(ir) => ir,
                        Err(e) => {
                            log::error!("build_ir: {e:?}");
                            return;
                        }
                    };
                    let scheduled = rill_lang::graph_lower::lower(&ir);
                    let programs: Vec<rill_lang::RillProgram<f32>> = ir
                        .topo_order
                        .iter()
                        .filter_map(|name| ir.nodes.get(name))
                        .map(|node| rill_lang::RillProgram::<f32>::new(node.ir.clone()))
                        .collect();
                    let mailbox = Arc::new(Mailbox::new(64));
                    let engine = rill_lang::graph_engine::RillGraphEngine::new(
                        scheduled, programs, mailbox, buf_size,
                    );

                    let _ = graph_tx.send(engine.handle());

                    let mut runner = rill_lang::program_runner::ProgramRunner::new(
                        engine,
                        Some(parent_ref),
                        buf_size,
                    );

                    if let Some((ref name, ref params)) = backend_name {
                        let mut bf: rill_graph::backend_factory::BackendFactory =
                            Default::default();
                        crate::registration::register_backends(&mut bf);
                        match bf.create_any(name, params) {
                            Ok((driver, capture, playback)) => {
                                runner.wire_backends(capture, playback);
                                let _ = runner.run_with_driver(driver, running);
                            }
                            Err(e) => log::error!("backend create '{}': {e}", name),
                        }
                    }
                });
            }

            // 3. Receive engine handle for module SetParameter routing
            let graph_ref = graph_rx
                .recv()
                .map_err(|e| ModularError::Graph(format!("graph handle: {e}")))?;

            // 4. Build modules via ModuleFactory
            let automaton_defs = &rd.automatons;
            for module_def in &rd.modules {
                let pb_def = to_pb_module(module_def);
                // Skip graph modules — they're handled by the signal thread above
                if pb_def.type_name() == "Graph" {
                    continue;
                }
                let graph_ref = graph_ref.clone();
                match self.module_factory.construct(
                    &pb_def,
                    automaton_defs,
                    &self.actor_system,
                    &graph_ref,
                ) {
                    Ok(actor_ref) => {
                        modules
                            .lock()
                            .unwrap()
                            .insert(pb_def.type_name().to_string(), actor_ref);
                    }
                    Err(e) => log::warn!("module construct '{}': {e}", pb_def.type_name()),
                }
            }
        }

        self.tokio_rt = Some(tokio_rt);
        Ok(self)
    }

    /// Stop all processing — terminates signal loops and drops the tokio runtime.
    pub fn stop(&mut self) {
        for case in self.cases.values_mut() {
            case.stop();
        }
        #[cfg(feature = "serialization")]
        {
            self.tokio_rt = None;
        }
    }
}

/// Convert the adrift [`ModuleDef`] (which includes `Graph`) to the
/// patchbay [`ModuleDef`] (which does not). Panics on `Graph` variant.
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
        ModuleDef::Graph { .. } => panic!("Graph modules are not handled by ModuleFactory"),
    }
}

fn str_to_param(s: &str) -> ParamValue {
    if let Ok(i) = s.parse::<i32>() {
        return ParamValue::Int(i);
    }
    if let Ok(f) = s.parse::<f32>() {
        return ParamValue::Float(f);
    }
    match s {
        "true" | "yes" | "1" => return ParamValue::Bool(true),
        "false" | "no" | "0" => return ParamValue::Bool(false),
        _ => {}
    }
    ParamValue::String(s.to_string())
}

impl<const BUF: usize> Drop for ModularSystem<BUF> {
    fn drop(&mut self) {
        self.stop();
    }
}
