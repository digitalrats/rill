//! # ModularSystem — modular/semi-modular audio processing host
//!
//! Implements the [`Eurorack`](rill_core::Eurorack) archetype — unifies a signal
//! graph with a control/automation patchbay.
//!
//! Creates a fully configured rack from serialised documents:
//!
//! * GraphDef — signal topology (nodes, connections, resources)
//! * PatchbayDef — control system (LFO, envelope, mappings)
//!
//! ## Feature gates
//!
//! | Subsystem | Feature | Optional? |
//! |---|---|---|
//! | Control (queue, automata) | *(always)* | required |
//! | Serialisation (JSON load) | `serialization` | yes |
//! | Audio I/O (audio thread) | `io` | yes |
//! | OSC server (system + surface) | `osc` | yes |
//!
//! ## Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────┐
//! │                   MODULARSYSTEM                                 │
//! │                                                                │
//! │  ┌──────────────┐   ┌──────────────────────────────────────┐  │
//! │  │  OscServer    │   │  Patchbay                        │  │
//! │  │  (tokio)      │   │  (tokio tasks: LFO, envelope, …)    │  │
//! │  │               │   │                                      │  │
//! │  │  /sys/*       │   │  handle_event(event) ──→ mapping    │  │
//! │  │  user paths   │   │       → PortCombiner → Queue        │  │
//! │  └───────┬───────┘   └──────────────┬───────────────────────┘  │
//! │          │                          │ MpscQueue                │
//! │  ┌───────┴──────────────────────────┴───────────────────────┐  │
//! │  │  Audio thread (std::thread)                              │  │
//! │  │  pop() → apply_param() → process_block() → io.write()   │  │
//! │  └──────────────────────────────────────────────────────────┘  │
//! └────────────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

use rill_core::queues::{CommandEnum, MpscQueue, SetParameter};
use rill_core::traits::{NodeId, NodeVariant, ParamValue, Params};
#[cfg(any(feature = "osc", feature = "serialization"))]
use rill_core_actor::ActorRef;
#[cfg(feature = "serialization")]
use rill_core_actor::ActorSystem;
#[cfg(any(feature = "osc", feature = "serialization"))]
use rill_core_actor::Mbox;
use rill_graph::backend_factory::BackendFactory;
use rill_graph::{Graph, GraphBuilder, NodeFactory};
use rill_patchbay::automaton::factory::AutomatonFactory;
#[cfg(feature = "osc")]
use rill_patchbay::engine::OscSurface;
use rill_patchbay::engine::Patchbay;
#[cfg(feature = "serialization")]
use rill_patchbay::function_registry::FunctionRegistry;

#[cfg(feature = "serialization")]
use crate::modular::serialization::ModularSystemDef;
#[cfg(feature = "serialization")]
use rill_graph::serialization::{GraphDef, SerializationError};
#[cfg(feature = "serialization")]
use rill_patchbay::serialization::PatchbayDef;

mod case;
mod config;
pub mod serialization;
pub use case::RackCase;
pub use config::{LaunchConfig, ModularConfig};

#[cfg(feature = "osc")]
mod dispatch;
#[cfg(feature = "osc")]
use dispatch::OscHandle;

// ============================================================================
// Public API
// ============================================================================

/// Start everything and wait for Ctrl+C.
///
/// Convenience wrapper for the common case: build once, run until
/// interrupted.  Subsystems are driven entirely by OSC commands and
/// the loaded documents.
///
/// If `config.graph_path` or `config.patchbay_path` are set, the
/// corresponding files are loaded before starting subsystems.
#[cfg(feature = "serialization")]
pub async fn run(config: ModularConfig) -> Result<(), ModularError> {
    let mut system = ModularSystem::<64>::new(config);
    system.load_files_from_config()?;
    system.start().await?;
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| ModularError::Osc(format!("ctrl+c: {e}")))?;
    system.stop();
    Ok(())
}

// ============================================================================
// Error type
// ============================================================================

/// Modular system error.
#[derive(Debug)]
pub enum ModularError {
    /// Graph document could not be loaded or built.
    #[cfg(feature = "serialization")]
    Graph(String),
    /// Patchbay document could not be loaded or applied.
    #[cfg(feature = "serialization")]
    Patchbay(String),
    /// OSC server error.
    #[cfg(feature = "osc")]
    Osc(String),
}

// ============================================================================
// ModularSystem struct
// ============================================================================

/// Fully data-driven modular processing host.
///
/// Implements the [`Rack`](rill_core::Rack) archetype — combines an audio
/// signal graph with a control/automation patchbay in a single rack.
/// Create via [`ModularSystem::new`] and configure with
/// [`set_default_backend`](Self::set_default_backend) before creating builders.
pub struct ModularSystem<const BUF: usize = 64> {
    /// Dead letters — undeliverable commands collected when the graph
    /// is not running (stale queue detected by the application layer).
    dead: Arc<MpscQueue<SetParameter>>,

    /// Shared node factory (populated at construction, extended via
    /// [`register_node_fn`](Self::register_node_fn)).
    node_factory: Arc<Mutex<NodeFactory<f32, BUF>>>,

    /// Shared automaton factory for Patchbay custom type dispatch.
    automaton_factory: Arc<Mutex<AutomatonFactory>>,

    /// Shared backend factory (populated at construction).
    backend_factory: Arc<BackendFactory<f32>>,

    /// Actor system for inter-case message routing.
    /// Each [`RackCase`] registered via [`create_case`](Self::create_case)
    /// gets a named mailbox in this system.
    #[cfg(feature = "serialization")]
    actor_system: ActorSystem<CommandEnum>,

    /// Registered Eurorack cases (name → case).
    #[cfg(feature = "serialization")]
    cases: HashMap<String, RackCase<BUF>>,

    /// Default backend configuration. When set, every
    /// [rill_graph::GraphBuilder]
    /// created by [`create_builder`](Self::create_builder) is pre-configured
    /// via [`GraphBuilder::with_backend`].
    default_backend: Option<(String, HashMap<String, ParamValue>)>,

    /// Host configuration (stored for serialized graph/patchbay loading).
    #[cfg(feature = "serialization")]
    config: ModularConfig,

    /// Current graph document (loaded, not yet built).
    #[cfg(feature = "serialization")]
    graph_doc: Option<GraphDef>,

    /// Control engine: automata, mappings, port combiners.
    control: Option<Patchbay>,

    /// Shared Patchbay reference (for OSC surface dispatch).
    #[cfg(feature = "osc")]
    control_shared: Option<Arc<std::sync::Mutex<Patchbay>>>,

    /// Current osc_surface (set by load_patchbay).
    #[cfg(feature = "osc")]
    osc_surface: OscSurface,

    /// Running OSC server + dispatch task.
    #[cfg(feature = "osc")]
    osc: Option<OscHandle>,

    // ── Fields set by [`launch`](Self::launch) ──────────────
    /// Shared Patchbay (single instance for MIDI + automata).
    #[cfg(feature = "serialization")]
    control_arc: Option<Arc<Mutex<Patchbay>>>,

    /// Tokio runtime for the control rack.
    #[cfg(feature = "serialization")]
    tokio_rt: Option<tokio::runtime::Runtime>,
}

impl<const BUF: usize> ModularSystem<BUF> {
    /// Create a new modular system with the given configuration.
    pub fn new(#[allow(unused_variables)] config: ModularConfig) -> Self {
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
            automaton_factory: Arc::new(Mutex::new(AutomatonFactory::new())),
            backend_factory: Arc::new(bf),
            default_backend,
            control: None,
            #[cfg(feature = "serialization")]
            config,
            #[cfg(feature = "serialization")]
            actor_system: ActorSystem::new(),
            #[cfg(feature = "serialization")]
            cases: HashMap::new(),
            #[cfg(feature = "serialization")]
            graph_doc: None,
            #[cfg(feature = "osc")]
            control_shared: None,
            #[cfg(feature = "osc")]
            osc_surface: Vec::new(),
            #[cfg(feature = "osc")]
            osc: None,
            #[cfg(feature = "serialization")]
            control_arc: None,
            #[cfg(feature = "serialization")]
            tokio_rt: None,
        }
    }

    /// Register a custom automaton constructor.
    ///
    /// The closure receives `(id: &str, params: &Params, target: &ServoTarget)`
    /// and must return a ready-made [`BoxedModule`] (Servo + automaton).
    #[cfg(feature = "serialization")]
    pub fn register_automaton_fn(
        &self,
        type_name: &'static str,
        f: impl Fn(
                &str,
                &rill_core::traits::Params,
                &rill_patchbay::automaton::factory::ServoTarget,
            ) -> rill_patchbay::engine::BoxedModule
            + Send
            + Sync
            + 'static,
    ) {
        self.automaton_factory
            .lock()
            .unwrap()
            .register_fn(type_name, move |id, params, target| f(id, params, target));
    }
    /// Register a custom node type via a closure.
    ///
    /// Must be called before [`create_builder`](Self::create_builder).
    /// The closure receives `(NodeId, &Params)` and must return a
    /// fully initialised [`NodeVariant`].
    pub fn register_node_fn(
        &self,
        type_name: &'static str,
        f: impl Fn(NodeId, &Params) -> NodeVariant<f32, BUF> + Send + Sync + 'static,
    ) {
        self.node_factory.lock().unwrap().register_fn(type_name, f);
    }

    /// Set the default audio backend for all future builders.
    ///
    /// When set, every [rill_graph::GraphBuilder] returned by
    /// [`create_builder`](Self::create_builder)
    /// is pre-configured with [`GraphBuilder::with_backend`] using the given
    /// name and parameters.
    pub fn set_default_backend(&mut self, name: &str, params: HashMap<String, ParamValue>) {
        self.default_backend = Some((name.to_string(), params));
    }

    /// Create a new Eurorack case and register it in the actor system.
    ///
    /// The case gets a named mailbox in the system's [`ActorSystem`].
    /// Other cases (or external actors) can send [`CommandEnum`]
    /// messages to this case via [`ModularSystem::route`] or
    /// [`ModularSystem::broadcast`].
    ///
    /// Returns a mutable reference to the newly created case.
    #[cfg(feature = "serialization")]
    pub fn create_case(&mut self, name: &str, sample_rate: f32) -> &mut RackCase<BUF> {
        let mbox = self.actor_system.create_mbox(name);
        let case = RackCase::new(name.to_string(), sample_rate, mbox);
        self.cases.insert(name.to_string(), case);
        self.cases.get_mut(name).expect("just inserted")
    }

    /// Route a command to a named case.
    ///
    /// If the case is registered, the command is pushed to its mailbox.
    /// Otherwise it goes to dead letters.
    #[cfg(feature = "serialization")]
    pub fn route(&mut self, case_name: &str, cmd: CommandEnum) {
        self.actor_system.route(case_name, cmd);
    }

    /// Broadcast a command to all registered cases.
    #[cfg(feature = "serialization")]
    pub fn broadcast(&mut self, cmd: CommandEnum) {
        self.actor_system.broadcast(cmd);
    }

    /// Drain dead letters — undeliverable messages for unregistered actors.
    #[cfg(feature = "serialization")]
    pub fn drain_dead(&self) -> Vec<CommandEnum> {
        self.actor_system.drain_dead()
    }

    /// Return the number of registered cases (actors).
    #[cfg(feature = "serialization")]
    pub fn case_count(&self) -> usize {
        self.actor_system.actor_count()
    }

    /// Access a case by name.
    #[cfg(feature = "serialization")]
    pub fn case(&self, name: &str) -> Option<&RackCase<BUF>> {
        self.cases.get(name)
    }

    /// Access a case mutably by name.
    #[cfg(feature = "serialization")]
    pub fn case_mut(&mut self, name: &str) -> Option<&mut RackCase<BUF>> {
        self.cases.get_mut(name)
    }

    /// Process one frame across all cases.
    ///
    /// For each case (in registration order):
    /// 1. Drain incoming mailbox — dispatch commands to local Graph/Patchbay
    /// 2. Process audio frame (if Graph is running)
    /// 3. Collect outgoing commands destined for other cases
    /// 4. Route outgoing commands through the actor system
    #[cfg(feature = "serialization")]
    pub fn tick(&mut self) {
        let mut all_outgoing: Vec<(String, CommandEnum)> = Vec::new();

        for (case_name, case) in self.cases.iter_mut() {
            // 1. Drain incoming commands
            let incoming = case.drain();
            // Dispatch will be handled by the case when Graph/Patchbay are running
            for _cmd in incoming {
                // TODO: dispatch to Graph (SetParameter) or Patchbay (AutomatonCommand)
            }

            // 2. Process audio frame — placeholder
            // case.process_frame();

            // 3. Collect outgoing commands
            let outgoing = case.take_outgoing();
            for cmd in outgoing {
                all_outgoing.push((case_name.clone(), cmd));
            }
        }

        // 4. Route outgoing commands through the actor system
        for (_from_case, cmd) in all_outgoing {
            // Determine target from command metadata or routing table
            // For now, broadcast to all cases (the receiver filters)
            self.actor_system.broadcast(cmd);
        }
    }

    /// Validate a system definition and create cases.
    ///
    /// Launch the modular system — for each case, spawn an audio thread
    /// and activate the patchbay (control rack).
    ///
    /// The signal graph is built inside the case's audio thread
    /// (Graph is not Send) and the run loop begins immediately.
    /// The patchbay (automata, mappings) is created synchronously
    /// and connected to the same command queue the graph will drain.
    #[cfg(feature = "serialization")]
    pub fn launch(mut self, def: &ModularSystemDef) -> Result<Self, ModularError> {
        for cd in &def.cases {
            self.create_case(&cd.name, def.sample_rate);
        }

        let node_factory = self.node_factory.clone();
        let backend_factory = self.backend_factory.clone();
        let default_backend = self.default_backend.clone();

        // Spawn one audio thread per case, wire up patchbay if present
        for cd in &def.cases {
            if let Some(case) = self.cases.get_mut(&cd.name) {
                let nf = node_factory.clone();
                let bf = backend_factory.clone();
                let db = default_backend.clone();
                let gd = cd.graph.clone();

                // Oneshot to receive the graph's ActorRef after build
                let (graph_tx, graph_rx) = std::sync::mpsc::channel();

                // Spawn audio thread with graph
                let parent_ref = case.handle();
                case.start(move |running| {
                    let mut builder = GraphBuilder::new(Arc::new(nf.lock().unwrap().clone()), bf);
                    if let Some((ref name, ref params)) = db {
                        builder.set_default_backend(name.clone(), params.clone());
                    }
                    builder.set_parent_ref(parent_ref);
                    if let Err(e) = gd.populate(&mut builder) {
                        log::error!("graph populate: {e}");
                        return;
                    }
                    match builder.build() {
                        Ok(mut graph) => {
                            let _ = graph_tx.send(graph.handle());
                            graph.run(running).ok();
                        }
                        Err(e) => log::error!("graph build: {e:?}"),
                    };
                });

                // Receive graph handle and create patchbay
                let graph_ref = graph_rx
                    .recv()
                    .map_err(|e| ModularError::Graph(format!("graph handle: {e}")))?;

                if let Some(ref patchbay_def) = cd.patchbay {
                    case.create_patchbay(patchbay_def, graph_ref)
                        .map_err(ModularError::Patchbay)?;
                }
            }
        }

        Ok(self)
    }

    /// Create a [rill_graph::GraphBuilder] sharing this runtime's factories.
    ///
    /// The builder uses the runtime's pre-populated node and backend
    /// factories. If a default backend was set via
    /// [`set_default_backend`](Self::set_default_backend), the builder
    /// is pre-configured with it.
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

    /// Create a [rill_graph::GraphBuilder] from a serialised [GraphDef].
    ///
    /// This is the canonical way to turn a deserialised (and possibly
    /// modified) graph document into a runnable graph.  The builder
    /// inherits all node and backend registrations from the runtime.
    #[cfg(feature = "serialization")]
    #[allow(dead_code)] // will be used by ModularSystemDef
    pub(crate) fn create_builder_from_graphdef(
        &self,
        def: &rill_graph::serialization::GraphDef,
    ) -> Result<GraphBuilder<f32, BUF>, SerializationError> {
        let mut builder = self.create_builder();
        def.populate(&mut builder)?;
        Ok(builder)
    }

    /// Build a graph from a [`GraphDef`] document.
    ///
    /// This is the canonical way to construct a signal graph from a
    /// serialised definition.  The graph is built inside the current
    /// thread and returned to the caller.
    ///
    /// # Errors
    ///
    /// Returns `String` with a human-readable error if the document
    /// is invalid or a node type is not registered.
    pub fn build_graph(
        &self,
        def: &GraphDef,
    ) -> Result<Graph<f32, BUF>, Box<dyn std::error::Error>> {
        let mut builder = self.create_builder();
        def.populate(&mut builder)
            .map_err(|e| format!("populate: {e}"))?;
        builder.build().map_err(|e| format!("build: {e}").into())
    }

    // ─── File loading ───────────────────────────────────────────────

    /// Load graph and/or patchbay documents from paths in `ModularConfig`.
    #[cfg(feature = "serialization")]
    fn load_files_from_config(&mut self) -> Result<(), ModularError> {
        if let Some(ref path) = self.config.graph_path {
            let json = std::fs::read_to_string(path)
                .map_err(|e| ModularError::Graph(format!("read '{:?}': {e}", path)))?;
            let doc: GraphDef = serde_json::from_str(&json)
                .map_err(|e| ModularError::Graph(format!("parse '{:?}': {e}", path)))?;
            self.load_graph(doc);
        }

        Ok(())
    }

    // ─── Public API ─────────────────────────────────────────────────

    /// Load a [rill_graph::serialization::GraphDef] into the runtime.
    ///
    /// The graph is **not** built or started until the graph is started via
    /// `Graph::run` or `Graph::stop`.
    #[cfg(feature = "serialization")]
    pub(crate) fn load_graph(&mut self, doc: GraphDef) {
        self.graph_doc = Some(doc);
        log::info!(
            "graph document loaded ({} nodes)",
            self.graph_doc.as_ref().map(|d| d.nodes.len()).unwrap_or(0),
        );
    }

    /// Load and apply a [`PatchbayDef`] with the given command channel.
    ///
    /// The `cmd_queue` is typically obtained from a built [`Graph`](rill_graph::Graph)
    /// via [`Graph::handle`](rill_graph::Graph::handle).
    /// Creates/replaces the [`Patchbay`] and updates the OSC surface.
    #[cfg(feature = "serialization")]
    #[allow(dead_code)] // will be used by ModularSystemDef
    pub(crate) fn load_patchbay(
        &mut self,
        doc: PatchbayDef,
        graph_ref: ActorRef<CommandEnum>,
    ) -> Result<(), ModularError> {
        let mut control = Patchbay::new(Arc::new(Mbox::new(64)), graph_ref.clone());
        let registry = FunctionRegistry::builtin();
        doc.apply_to_async(&mut control, &registry)
            .map_err(ModularError::Patchbay)?;

        self.control = Some(control);

        #[cfg(feature = "osc")]
        {
            self.osc_surface = doc.osc_surface.clone();
            let mut ctrl = Patchbay::new(Arc::new(Mbox::new(64)), graph_ref);
            doc.apply_to(&mut ctrl, &registry)
                .map_err(ModularError::Patchbay)?;
            self.control_shared = Some(Arc::new(std::sync::Mutex::new(ctrl)));
        }

        log::info!("patchbay loaded ({} automata)", doc.automata.len());
        Ok(())
    }

    /// Drain the dead letters queue, returning all undeliverable messages.
    pub fn drain_dead_letters(&self) -> Vec<SetParameter> {
        let mut msgs = Vec::new();
        while let Some(msg) = self.dead.pop() {
            msgs.push(msg);
        }
        msgs
    }

    // ─── Lifecycle ─────────────────────────────────────────────────

    /// Start control and OSC subsystems according to configuration.
    #[cfg(feature = "serialization")]
    pub async fn start(&mut self) -> Result<(), ModularError> {
        #[cfg(feature = "osc")]
        if let Some(ref _bind) = self.config.osc_bind.clone() {
            // OSC server needs a command queue — provide via start_osc
            // or use the dead letters queue as a sink.
        }

        Ok(())
    }

    /// Start the OSC server with system and user surface handlers.
    #[cfg(feature = "osc")]
    pub async fn start_osc(
        &mut self,
        bind: &str,
        cmd_queue: ActorRef<CommandEnum>,
    ) -> Result<(), ModularError> {
        if self.osc.is_some() {
            return Err(ModularError::Osc("already running".into()));
        }

        let control = self.control_shared.clone().unwrap_or_else(|| {
            Arc::new(std::sync::Mutex::new(Patchbay::new(
                Arc::new(Mbox::new(64)),
                cmd_queue.clone(),
            )))
        });
        let surface = self.osc_surface.clone();

        let handle = OscHandle::start(bind, cmd_queue, control, surface)
            .await
            .map_err(ModularError::Osc)?;

        self.osc = Some(handle);
        log::info!("OSC server started on {bind}");
        Ok(())
    }

    /// Stop all subsystems.
    pub fn stop(&mut self) {
        log::info!("stopping runtime…");

        // Stop control rack — automata, MIDI, sequencer, PortCombiners.
        #[cfg(feature = "serialization")]
        if let Some(ref shared) = self.control_arc {
            if let Ok(mut pb) = shared.lock() {
                pb.stop_all();
            }
        }

        // Legacy control (non-Arc, used by load_patchbay without launch).
        if let Some(ref mut ctrl) = self.control {
            ctrl.stop_all();
        }

        #[cfg(feature = "osc")]
        if let Some(ref o) = self.osc {
            o.task.abort();
        }

        // Stop all cases (each owns its audio thread).
        #[cfg(feature = "serialization")]
        for (_name, case) in self.cases.iter_mut() {
            case.stop();
        }

        // Drop tokio runtime — remaining green threads cancelled.
        #[cfg(feature = "serialization")]
        {
            self.tokio_rt = None;
        }

        log::info!("runtime stopped");
    }

    // ─── Launch (two‑rack, one command) ─────────────────────────

    /// Build and start both racks in one call.
    ///
    /// The signal graph is constructed on the audio thread (Graph is not Send).
    /// The control rack (Patchbay, MIDI) runs on a separate tokio runtime.
    /// An `ActorRef<SetParameter>` channel bridges the two racks.
    ///
    /// Requires `serialization` feature.
    #[cfg(feature = "serialization")]
    pub fn run(&mut self, _running: Arc<AtomicBool>) {
        // TODO: run loop for existing cases
    }
}

/// Convert a string from config to [`ParamValue`].
///
/// Tries i32, f32, bool, then falls back to string.
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
