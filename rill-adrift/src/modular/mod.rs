//! # ModularSystem — modular signal processing host
//!
//! * GraphDef — signal topology
//! * RackDef — control system (LFO, envelope, sequencer)

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

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
#[cfg(feature = "serialization")]
use rill_graph::backend_factory::BackendFactory;

#[cfg(feature = "lang")]
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
    #[cfg(feature = "lang")]
    pub fn build_engine(
        &self,
        def: &GraphDef,
        buf_size: usize,
    ) -> Result<rill_lang::graph_engine::RillGraphEngine<f32>, Box<dyn std::error::Error>> {
        let mut builder = self.create_builder();
        def.populate(&mut builder)
            .map_err(|e| format!("populate: {e}"))?;

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

    /// Launch — build graph engines, set up I/O. Returns self for chaining.
    /// Call `run()` after launch to start the I/O loop.
    #[cfg(all(feature = "serialization", feature = "lang"))]
    pub fn launch(mut self, def: &ModularSystemDef) -> Result<Self, ModularError> {
        let tokio_rt = tokio::runtime::Runtime::new()
            .map_err(|e| ModularError::Graph(format!("tokio: {e}")))?;
        let _guard = tokio_rt.enter();

        for rd in &def.racks {
            let buf_size = def.block_size.max(64);

            let actor_ref = self.actor_system.spawn_detached(
                &format!("rack_{}", rd.name),
                move || Box::new(move |_msg: CommandEnum| {}),
                1,
            );

            // Build engine using shared build_engine method
            let _engine = self
                .build_engine(&rd.graph, buf_size)
                .map_err(|e| ModularError::Graph(format!("{e:?}")))?;

            let _case = RackCase::new(rd.name.clone(), def.sample_rate, actor_ref.clone(), vec![]);
            self.cases.insert(rd.name.clone(), _case);
        }

        self.tokio_rt = Some(tokio_rt);
        Ok(self)
    }

    /// Launch — legacy stub (requires both lang and serialization).
    #[cfg(all(feature = "serialization", not(feature = "lang")))]
    pub fn launch(mut self, def: &ModularSystemDef) -> Result<Self, ModularError> {
        let tokio_rt = tokio::runtime::Runtime::new()
            .map_err(|e| ModularError::Graph(format!("tokio: {e}")))?;
        let _guard = tokio_rt.enter();

        for rd in &def.racks {
            let actor_ref = self.actor_system.spawn_detached(
                &format!("rack_{}", rd.name),
                move || Box::new(move |_msg: CommandEnum| {}),
                1,
            );
            let _case = RackCase::new(rd.name.clone(), def.sample_rate, actor_ref, vec![]);
            self.cases.insert(rd.name.clone(), _case);
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
