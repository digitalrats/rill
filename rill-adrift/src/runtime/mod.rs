//! # rill_adrift::Runtime — data-driven signal processing host
//!
//! Creates a fully configured "rill world" from serialised documents:
//!
//! * GraphDef — signal topology (nodes, connections, resources)
//! * PatchbayDef — control system (LFO, envelope, mappings)
//!   including the [`OscSurface`] that maps OSC paths to controller IDs
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
//! │                    RILL_ADRIFT::RUNTIME                         │
//! │                                                                │
//! │  ┌──────────────┐   ┌──────────────────────────────────────┐  │
//! │  │  OscServer    │   │  PatchbayEngine                      │  │
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

use std::sync::Arc;
use std::sync::Mutex;

use rill_core::queues::{MpscQueue, SetParameter};
use rill_core::traits::{NodeId, NodeParams, NodeVariant};
use rill_core_actor::ActorRef;
use rill_graph::backend_factory::BackendFactory;
use rill_graph::{GraphBuilder, NodeFactory};
#[cfg(feature = "osc")]
use rill_patchbay::engine::{OscSurface, Patchbay};
#[cfg(feature = "serialization")]
use rill_patchbay::function_registry::FunctionRegistry;

#[cfg(feature = "serialization")]
use rill_graph::serialization::GraphDef;
#[cfg(feature = "serialization")]
use rill_patchbay::serialization::PatchbayDef;

mod config;
pub use config::RuntimeConfig;

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
pub async fn run(config: RuntimeConfig) -> Result<(), RuntimeError> {
    let mut rt = Runtime::<64>::new(config);
    rt.load_files_from_config()?;
    rt.start().await?;
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| RuntimeError::Osc(format!("ctrl+c: {e}")))?;
    rt.stop();
    Ok(())
}

// ============================================================================
// Error type
// ============================================================================

/// Runtime error.
#[derive(Debug)]
pub enum RuntimeError {
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
// Runtime struct
// ============================================================================

/// Fully data-driven signal processing host.
///
/// Create via [`Runtime::new`], start subsystems individually with
/// start, or use the free function run for
/// the all-in-one lifecycle.
pub struct Runtime<const BUF: usize = 64> {
    /// Dead letters — undeliverable commands collected when the graph
    /// is not running (stale queue detected by the application layer).
    dead: Arc<MpscQueue<SetParameter>>,

    /// Shared node factory (populated at construction, extended via
    /// [`register_node_fn`](Self::register_node_fn)).
    node_factory: Arc<Mutex<NodeFactory<f32, BUF>>>,

    /// Shared backend factory (populated at construction).
    backend_factory: Arc<BackendFactory<f32>>,

    /// Host configuration (stored for serialized graph/patchbay loading).
    #[cfg(feature = "serialization")]
    config: RuntimeConfig,

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
}

impl<const BUF: usize> Runtime<BUF> {
    /// Create a new (stopped) runtime with the given configuration.
    ///
    /// Initialises the node and backend factories with all built-in types.
    /// Use [`register_node_fn`](Self::register_node_fn) to add custom node
    /// types before calling [`create_builder`](Self::create_builder).
    pub fn new(#[allow(unused_variables)] config: RuntimeConfig) -> Self {
        let mut nf = NodeFactory::new();
        crate::registration::register_all_nodes(&mut nf);
        let mut bf = BackendFactory::new();
        crate::registration::register_backends(&mut bf);
        Self {
            dead: Arc::new(MpscQueue::new()),
            node_factory: Arc::new(Mutex::new(nf)),
            backend_factory: Arc::new(bf),
            control: None,
            #[cfg(feature = "serialization")]
            config,
            #[cfg(feature = "serialization")]
            graph_doc: None,
            #[cfg(feature = "osc")]
            control_shared: None,
            #[cfg(feature = "osc")]
            osc_surface: Vec::new(),
            #[cfg(feature = "osc")]
            osc: None,
        }
    }

    /// Register a custom node type via a closure.
    ///
    /// Must be called before [`create_builder`](Self::create_builder).
    /// The closure receives `(NodeId, &NodeParams)` and must return a
    /// fully initialised [`NodeVariant`].
    pub fn register_node_fn(
        &self,
        type_name: &'static str,
        f: impl Fn(NodeId, &NodeParams) -> NodeVariant<f32, BUF> + Send + Sync + 'static,
    ) {
        self.node_factory.lock().unwrap().register_fn(type_name, f);
    }

    /// Create a [`GraphBuilder`] sharing this runtime's factories.
    ///
    /// The builder uses the runtime's pre-populated node and backend
    /// factories. Call [`register_node_fn`](Self::register_node_fn)
    /// first if you need custom node types.
    pub fn create_builder(&self) -> GraphBuilder<f32, BUF> {
        GraphBuilder::new(
            // Clone the Arc'd Mutex contents to get an independent copy
            // of the factory for the builder.
            Arc::new(self.node_factory.lock().unwrap().clone()),
            self.backend_factory.clone(),
        )
    }

    // ─── File loading ───────────────────────────────────────────────

    /// Load graph and/or patchbay documents from paths in `RuntimeConfig`.
    #[cfg(feature = "serialization")]
    fn load_files_from_config(&mut self) -> Result<(), RuntimeError> {
        if let Some(ref path) = self.config.graph_path {
            let json = std::fs::read_to_string(path)
                .map_err(|e| RuntimeError::Graph(format!("read '{:?}': {e}", path)))?;
            let doc: GraphDef = serde_json::from_str(&json)
                .map_err(|e| RuntimeError::Graph(format!("parse '{:?}': {e}", path)))?;
            self.load_graph(doc);
        }

        Ok(())
    }

    // ─── Public API ─────────────────────────────────────────────────

    /// Load a [`GraphDef`] into the runtime.
    ///
    /// The graph is **not** built or started until [`start_audio`] or
    /// `/sys/graph/start` is received.
    #[cfg(feature = "serialization")]
    pub fn load_graph(&mut self, doc: GraphDef) {
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
    pub fn load_patchbay(
        &mut self,
        doc: PatchbayDef,
        cmd_queue: ActorRef<SetParameter>,
    ) -> Result<(), RuntimeError> {
        let mut control = Patchbay::new(cmd_queue.clone());
        let registry = FunctionRegistry::builtin();
        doc.apply_to_async(&mut control, &registry)
            .map_err(RuntimeError::Patchbay)?;

        self.control = Some(control);

        #[cfg(feature = "osc")]
        {
            self.osc_surface = doc.osc_surface.clone();
            let mut ctrl = Patchbay::new(cmd_queue);
            doc.apply_to(&mut ctrl, &registry)
                .map_err(RuntimeError::Patchbay)?;
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
    pub async fn start(&mut self) -> Result<(), RuntimeError> {
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
        cmd_queue: ActorRef<SetParameter>,
    ) -> Result<(), RuntimeError> {
        if self.osc.is_some() {
            return Err(RuntimeError::Osc("already running".into()));
        }

        let control = self.control_shared.clone().unwrap_or_else(|| {
            Arc::new(std::sync::Mutex::new(Patchbay::new(ActorRef::new(
                &Arc::new(MpscQueue::with_capacity(64)),
            ))))
        });
        let surface = self.osc_surface.clone();

        let handle = OscHandle::start(bind, cmd_queue, control, surface)
            .await
            .map_err(RuntimeError::Osc)?;

        self.osc = Some(handle);
        log::info!("OSC server started on {bind}");
        Ok(())
    }

    /// Stop all subsystems.
    pub fn stop(&mut self) {
        log::info!("stopping runtime…");

        #[cfg(feature = "osc")]
        if let Some(ref o) = self.osc {
            o.task.abort();
        }

        if let Some(ref mut ctrl) = self.control {
            ctrl.stop_all();
        }

        log::info!("runtime stopped");
    }
}

impl<const BUF: usize> Drop for Runtime<BUF> {
    fn drop(&mut self) {
        self.stop();
    }
}
