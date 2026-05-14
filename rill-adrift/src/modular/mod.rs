//! # ModularSystem — modular audio processing host
//!
//! * GraphDef — signal topology
//! * RackDef — control system (LFO, envelope, sequencer)

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use rill_core::queues::{CommandEnum, MpscQueue, SetParameter};
use rill_core::traits::ParamValue;
use rill_core_actor::{ActorRef, ActorSystem};
use rill_graph::backend_factory::BackendFactory;
use rill_graph::{Graph, GraphBuilder, NodeFactory};
#[cfg(feature = "serialization")]
use rill_patchbay::function_registry::FunctionRegistry;
use rill_patchbay::module_factory::ModuleFactory;

#[cfg(feature = "serialization")]
use crate::modular::serialization::ModularSystemDef;
#[cfg(feature = "serialization")]
use rill_graph::serialization::GraphDef;

mod case;
mod config;
pub mod serialization;
pub use case::RackCase;
pub use config::{LaunchConfig, ModularConfig};
/// Documentation.

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
pub enum ModularError {
    /// Documentation.
    Graph(String),
    /// Documentation.
    Rack(String),
}
/// Documentation.

// ============================================================================
// ModularSystem struct
// ============================================================================

pub struct ModularSystem<const BUF: usize = 64> {
    dead: Arc<MpscQueue<SetParameter>>,
    node_factory: Arc<Mutex<NodeFactory<f32, BUF>>>,
    backend_factory: Arc<BackendFactory<f32>>,
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
    /// Documentation.
    pub fn new(config: ModularConfig) -> Self {
        let mut nf = NodeFactory::new();
        crate::registration::register_all_nodes(&mut nf);
        let bf = {
            #[allow(unused_mut)]
            let mut bf = BackendFactory::new();
            #[cfg(feature = "io")]
            crate::registration::register_backends(&mut bf);
            #[cfg(feature = "lofi")]
            crate::registration::register_lofi_backends(&mut bf);
            bf
        };
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
            backend_factory: Arc::new(bf),
            module_factory: ModuleFactory::new(),
            default_backend,
            actor_system: Arc::new(ActorSystem::new()),
            cases: HashMap::new(),
            config,
            #[cfg(feature = "serialization")]
            tokio_rt: None,
        }
    }
    /// Documentation.

    pub fn set_default_backend(&mut self, name: &str, params: HashMap<String, ParamValue>) {
        self.default_backend = Some((name.to_string(), params));
    }

    pub(crate) fn create_builder(&self) -> GraphBuilder<f32, BUF> {
        let mut builder = GraphBuilder::new(
            Arc::new(self.node_factory.lock().unwrap().clone()),
            self.backend_factory.clone(),
        );
        if let Some((ref name, ref params)) = self.default_backend {
            builder.set_default_backend(name.clone(), params.clone());
        }
        builder
    }
    /// Documentation.

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
            let backend_factory = self.backend_factory.clone();
            let default_backend = self.default_backend.clone();
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
                case.start(move |running| {
                    let mut builder = GraphBuilder::new(
                        Arc::new(node_factory.lock().unwrap().clone()),
                        backend_factory,
                    );
                    if let Some((ref name, ref params)) = default_backend {
                        builder.set_default_backend(name.clone(), params.clone());
                    }
                    builder.set_parent_ref(parent_ref);
                    if let Err(e) = gd.populate(&mut builder) {
                        log::error!("graph populate: {e}");
                        return;
                    }
                    match builder.build(&sys) {
                        Ok(mut graph) => {
                            let _ = graph_tx.send(graph.handle());
                            graph.run(running).ok();
                        }
                        Err(e) => log::error!("graph build: {e:?}"),
                    };
                });
            }

            // 3. Receive graph_ref
            let graph_ref = graph_rx
                .recv()
                .map_err(|e| ModularError::Graph(format!("graph handle: {e}")))?;

            // 4. Build servos + custom modules
            let registry = FunctionRegistry::builtin();
            let servos = rd
                .build_servos(&registry, &self.module_factory, &sys_svc, &graph_ref)
                .map_err(|e| ModularError::Rack(format!("rack '{}': {e}", rd.name)))?;
            *modules.lock().unwrap() = servos;
        }

        self.tokio_rt = Some(tokio_rt);
        Ok(self)
    }
    /// Documentation.

    pub fn stop(&mut self) {
        for case in self.cases.values_mut() {
            case.stop();
        }
        #[cfg(feature = "serialization")]
        {
            self.tokio_rt = None;
        }
    }
    /// Documentation.

    pub fn drain_dead_letters(&self) -> Vec<SetParameter> {
        let mut msgs = Vec::new();
        while let Some(msg) = self.dead.pop() {
            msgs.push(msg);
        }
        msgs
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
