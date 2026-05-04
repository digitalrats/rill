//! # rill_adrift::Runtime — data-driven signal processing host
//!
//! Creates a fully configured "rill world" from serialised documents:
//!
//! * GraphDocument — signal topology (nodes, connections, resources)
//! * PatchbayDocument — control system (LFO, envelope, mappings)
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

use rill_core::queues::MpscQueue;
use rill_core::NodeId;
use rill_patchbay::control::{OscSurface, ParameterCommand, PatchbayControl};
use rill_patchbay::engine::PatchbayEngine;
#[cfg(feature = "serialization")]
use rill_patchbay::function_registry::FunctionRegistry;

#[cfg(feature = "serialization")]
use rill_graph::serialization::GraphDocument;
#[cfg(feature = "serialization")]
use rill_patchbay::document::PatchbayDocument;

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
    let mut rt = Runtime::new(config);
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
pub struct Runtime {
    /// Lock-free command queue (control → audio thread).
    pub(crate) queue: Arc<MpscQueue<ParameterCommand>>,

    /// Host configuration (stored for serialized graph/patchbay loading).
    #[cfg(feature = "serialization")]
    config: RuntimeConfig,

    /// Current graph document (loaded, not yet built).
    #[cfg(feature = "serialization")]
    graph_doc: Option<GraphDocument>,

    /// Control engine: automata, mappings, port combiners.
    control: Option<PatchbayEngine>,

    /// Shared PatchbayControl reference (for OSC surface dispatch).
    #[cfg(feature = "osc")]
    control_shared: Option<Arc<std::sync::Mutex<PatchbayControl>>>,

    /// Current osc_surface (set by load_patchbay).
    #[cfg(feature = "osc")]
    osc_surface: OscSurface,

    /// Running OSC server + dispatch task.
    #[cfg(feature = "osc")]
    osc: Option<OscHandle>,
}

impl Runtime {
    /// Create a new (stopped) runtime with the given configuration.
    pub fn new(#[allow(unused_variables)] config: RuntimeConfig) -> Self {
        Self {
            queue: Arc::new(MpscQueue::new()),
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

    // ─── File loading ───────────────────────────────────────────────

    /// Load graph and/or patchbay documents from paths in `RuntimeConfig`.
    #[cfg(feature = "serialization")]
    fn load_files_from_config(&mut self) -> Result<(), RuntimeError> {
        if let Some(ref path) = self.config.graph_path {
            let json = std::fs::read_to_string(path)
                .map_err(|e| RuntimeError::Graph(format!("read '{:?}': {e}", path)))?;
            let doc: GraphDocument = serde_json::from_str(&json)
                .map_err(|e| RuntimeError::Graph(format!("parse '{:?}': {e}", path)))?;
            self.load_graph(doc);
        }

        if let Some(ref path) = self.config.patchbay_path {
            let json = std::fs::read_to_string(path)
                .map_err(|e| RuntimeError::Patchbay(format!("read '{:?}': {e}", path)))?;
            let doc = rill_patchbay::document::from_json(&json)
                .map_err(|e| RuntimeError::Patchbay(format!("parse '{:?}': {e}", path)))?;
            self.load_patchbay(doc)?;
        }

        Ok(())
    }

    // ─── Public API ─────────────────────────────────────────────────

    /// Load a [`GraphDocument`] into the runtime.
    ///
    /// The graph is **not** built or started until [`start_audio`] or
    /// `/sys/graph/start` is received.
    #[cfg(feature = "serialization")]
    pub fn load_graph(&mut self, doc: GraphDocument) {
        self.graph_doc = Some(doc);
        log::info!(
            "graph document loaded ({} nodes)",
            self.graph_doc.as_ref().map(|d| d.nodes.len()).unwrap_or(0),
        );
    }

    /// Load and apply a [`PatchbayDocument`].
    ///
    /// Creates/replaces the [`PatchbayEngine`] and updates the OSC surface.
    #[cfg(feature = "serialization")]
    pub fn load_patchbay(&mut self, doc: PatchbayDocument) -> Result<(), RuntimeError> {
        let queue = self.queue.clone();
        let mut engine = PatchbayEngine::new(queue.clone());
        let registry = FunctionRegistry::builtin();
        engine
            .load_document(&doc, &registry)
            .map_err(|e| RuntimeError::Patchbay(e))?;

        self.control = Some(engine);

        #[cfg(feature = "osc")]
        {
            self.osc_surface = doc.osc_surface.clone();
            let mut ctrl = PatchbayControl::new(self.queue.clone());
            doc.apply_to(&mut ctrl, &registry)
                .map_err(|e| RuntimeError::Patchbay(e))?;
            self.control_shared = Some(Arc::new(std::sync::Mutex::new(ctrl)));
        }

        log::info!("patchbay loaded ({} automata)", doc.automata.len());
        Ok(())
    }

    /// Push a parameter command to the audio thread's queue.
    ///
    /// Lock-free; safe from any thread.
    pub fn set_param(&self, node: NodeId, param: &str, value: f32) {
        let _ = self.queue.push(ParameterCommand::new(node, param, value));
    }

    /// Access the shared command queue.
    pub fn queue(&self) -> Arc<MpscQueue<ParameterCommand>> {
        self.queue.clone()
    }

    // ─── Lifecycle ─────────────────────────────────────────────────

    /// Start control and OSC subsystems according to configuration.
    #[cfg(feature = "serialization")]
    pub async fn start(&mut self) -> Result<(), RuntimeError> {
        #[cfg(feature = "osc")]
        if let Some(ref bind) = self.config.osc_bind.clone() {
            self.start_osc(bind).await?;
        }

        Ok(())
    }

    /// Start the OSC server with system and user surface handlers.
    #[cfg(feature = "osc")]
    pub async fn start_osc(&mut self, bind: &str) -> Result<(), RuntimeError> {
        if self.osc.is_some() {
            return Err(RuntimeError::Osc("already running".into()));
        }

        let control = self.control_shared.clone().unwrap_or_else(|| {
            Arc::new(std::sync::Mutex::new(PatchbayControl::new(self.queue.clone())))
        });
        let surface = self.osc_surface.clone();

        let handle = OscHandle::start(bind, self.queue.clone(), control, surface)
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

        if let Some(ref mut pe) = self.control {
            pe.stop();
        }

        log::info!("runtime stopped");
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop();
    }
}


