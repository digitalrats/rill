//! # ModularSystem — modular signal processing host
//!
//! * GraphDef — signal topology
//! * RackDef — control system (LFO, envelope, sequencer)

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;

#[cfg(feature = "serialization")]
use rill_core::queues::CommandEnum;
use rill_core::queues::{MpscQueue, SetParameter};
#[cfg(feature = "serialization")]
use rill_core::time::ClockTick;
use rill_core::traits::ParamValue;
#[cfg(feature = "serialization")]
use rill_core_actor::ActorRef;
use rill_core_actor::ActorSystem;
#[cfg(feature = "serialization")]
use rill_graph::backend_factory::BackendFactory;
#[cfg(feature = "serialization")]
use rill_graph::Graph;
use rill_graph::{GraphBuilder, NodeFactory};
use rill_patchbay::module_factory::ModuleFactory;

#[cfg(feature = "serialization")]
use crate::modular::serialization::ModularSystemDef;
#[cfg(feature = "serialization")]
use crate::modular::serialization::ModuleDef;
#[cfg(feature = "serialization")]
use rill_graph::serialization::GraphDef;

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
/// The system provides shared infrastructure: actor system, node factory, backend factory,
/// and a module factory for custom rack modules.
pub struct ModularSystem<const BUF: usize = 64> {
    dead: Arc<MpscQueue<SetParameter>>,
    node_factory: Arc<Mutex<NodeFactory<f32, BUF>>>,
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
        let mut nf = NodeFactory::new();
        crate::registration::register_all_nodes(&mut nf);
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
            dead: Arc::new(MpscQueue::new()),
            node_factory: Arc::new(Mutex::new(nf)),
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
        GraphBuilder::new(Arc::new(self.node_factory.lock().unwrap().clone()))
    }
    /// Build a signal graph from a GraphDef.
    #[cfg(feature = "serialization")]
    pub fn build_graph(
        &self,
        def: &GraphDef,
    ) -> Result<Graph<f32, BUF>, Box<dyn std::error::Error>> {
        let mut builder = self.create_builder();
        def.populate(&mut builder)
            .map_err(|e| format!("populate: {e}"))?;
        builder
            .build(&self.actor_system)
            .map_err(|e| format!("build: {e}").into())
    }

    /// Access the module factory for registering custom rack module types before launch.
    pub fn module_factory_mut(&mut self) -> &mut ModuleFactory {
        &mut self.module_factory
    }

    /// Access the node factory for registering custom node types before launch.
    pub fn node_factory_mut(&self) -> std::sync::MutexGuard<'_, NodeFactory<f32, BUF>> {
        self.node_factory.lock().unwrap()
    }

    /// Launch — build graph, spawn servos, start threads.
    #[cfg(feature = "serialization")]
    pub fn launch(mut self, def: &ModularSystemDef) -> Result<Self, ModularError> {
        let tokio_rt = tokio::runtime::Runtime::new()
            .map_err(|e| ModularError::Graph(format!("tokio: {e}")))?;
        let _guard = tokio_rt.enter();

        for rd in &def.racks {
            let node_factory = self.node_factory.clone();
            let sys = self.actor_system.clone();
            let sys_svc = self.actor_system.clone();
            let gd = rd.graph.clone();

            let (graph_tx, graph_rx) = std::sync::mpsc::channel();

            let modules: Arc<Mutex<HashMap<String, ActorRef<CommandEnum>>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let tasks: Vec<std::thread::JoinHandle<()>> = Vec::new();

            // 1. Rack actor — forwards to modules
            let case_name = rd.name.clone();
            let m = modules.clone();
            let actor_ref = sys.spawn_detached(
                &format!("rack_{case_name}"),
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
            let case = RackCase::new(rd.name.clone(), def.sample_rate, actor_ref, tasks);
            self.cases.insert(rd.name.clone(), case);

            // 2. Build graph on I/O thread
            if let Some(case) = self.cases.get_mut(&rd.name) {
                let backend_name = self.default_backend.clone();
                #[cfg(feature = "io")]
                let mut bf = {
                    let mut f = BackendFactory::new();
                    crate::registration::register_backends(&mut f);
                    f
                };
                #[cfg(not(feature = "io"))]
                let bf = BackendFactory::new();
                let graph_def = gd.clone();
                case.start(move |running| {
                    let mut builder =
                        GraphBuilder::new(Arc::new(node_factory.lock().unwrap().clone()));
                    builder.set_parent_ref(parent_ref);
                    if let Err(e) = graph_def.populate(&mut builder) {
                        log::error!("graph populate: {e}");
                        return;
                    }
                    match builder.build(&sys) {
                        Ok(graph) => {
                            let _ = graph_tx.send(graph.handle());
                            let mut state = graph.into_processing_state();

                            // Create backend and wire to nodes
                            if let Some((ref name, ref params)) = backend_name {
                                match bf.create_any(name, params) {
                                    Ok((driver, capture, playback)) => {
                                        state.wire_backends(capture, playback);
                                        if let Err(e) =
                                            state.run_with_driver(driver, running.clone())
                                        {
                                            log::error!("driver run: {e}");
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("backend create '{}': {e}", name);
                                        let tick = ClockTick::default();
                                        let _ = state.process_block(&tick);
                                        while running.load(Ordering::Acquire) {
                                            std::thread::park();
                                        }
                                    }
                                }
                            } else {
                                let tick = ClockTick::default();
                                let _ = state.process_block(&tick);
                                while running.load(Ordering::Acquire) {
                                    std::thread::park();
                                }
                            }
                        }
                        Err(e) => log::error!("graph build: {e:?}"),
                    };
                });
            }

            // 3. Receive graph_ref
            let graph_ref = graph_rx
                .recv()
                .map_err(|e| ModularError::Graph(format!("graph handle: {e}")))?;

            // 4. Build modules via ModuleFactory
            let automaton_defs = &rd.automatons;
            let mut servos = HashMap::new();
            for module_def in &rd.modules {
                match module_def {
                    ModuleDef::Graph { .. } => continue,
                    _ => {
                        use rill_patchbay::module_def::{
                            AutomatonDef as PbAutomatonDef, ModuleDef as PbModuleDef, SensorDef,
                        };

                        let pb_module = to_pb_module(module_def);
                        let automaton_defs_slice: Vec<PbAutomatonDef> = automaton_defs.to_vec();
                        let actor_ref = self
                            .module_factory
                            .construct(&pb_module, &automaton_defs_slice, &sys_svc, &graph_ref)
                            .map_err(|e| ModularError::Rack(e.to_string()))?;
                        let id = match &pb_module {
                            PbModuleDef::Clock(c) => {
                                format!("clock_{}", c.port_name)
                            }
                            PbModuleDef::Servo(s) => s.automaton_id.clone(),
                            PbModuleDef::Sensor(s) => match s {
                                SensorDef::Midi { port_name, .. } => {
                                    format!("midi_{port_name}")
                                }
                                SensorDef::Osc { port, .. } => {
                                    format!("osc_{port}")
                                }
                            },
                            PbModuleDef::Custom { type_name, .. } => type_name.clone(),
                        };
                        servos.insert(id, actor_ref);
                    }
                }
            }
            *modules.lock().unwrap() = servos;
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
    /// Drain undelivered `SetParameter` messages from the dead letter queue.
    pub fn drain_dead_letters(&self) -> Vec<SetParameter> {
        let mut msgs = Vec::new();
        while let Some(msg) = self.dead.pop() {
            msgs.push(msg);
        }
        msgs
    }
}

/// Convert the adrift [`ModuleDef`] (which includes `Graph`) to the
/// patchbay [`ModuleDef`] (which does not). Panics on `Graph` variant.
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
