//! # rill_adrift::Runtime — data-driven signal processing host
//!
//! Creates a fully configured "rill world" from serialised documents:
//!
//! * [`GraphDocument`] — signal topology (nodes, connections, resources)
//! * [`PatchbayDocument`] — control system (LFO, envelope, mappings)
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

#[cfg(all(feature = "io", feature = "serialization"))]
use crate::registration;

#[cfg(all(feature = "io", feature = "serialization"))]
use crate::io::audio_io::AudioIo;

#[cfg(all(feature = "io", feature = "serialization", feature = "pipewire"))]
use crate::io::PipewireBackend;

#[cfg(feature = "serialization")]
use rill_graph::serialization::GraphDocument;
#[cfg(feature = "serialization")]
use rill_patchbay::document::PatchbayDocument;

mod config;
pub use config::RuntimeConfig;

#[cfg(feature = "io")]
mod engine;
#[cfg(feature = "io")]
#[cfg(feature = "io")]
use engine::AudioHandle;
#[cfg(all(feature = "io", feature = "serialization"))]
use engine::BUF_SIZE;

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
    /// Audio engine / backend error.
    #[cfg(feature = "io")]
    Audio(String),
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
/// [`start`](Runtime::start), or use the free function [`run`] for
/// the all-in-one lifecycle.
pub struct Runtime {
    config: RuntimeConfig,

    /// Lock-free command queue (control → audio thread).
    pub(crate) queue: Arc<MpscQueue<ParameterCommand>>,

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

    /// Running audio engine (audio thread).
    #[cfg(feature = "io")]
    audio: Option<AudioHandle>,

    /// Running OSC server + dispatch task.
    #[cfg(feature = "osc")]
    osc: Option<OscHandle>,
}

impl Runtime {
    /// Create a new (stopped) runtime with the given configuration.
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            queue: Arc::new(MpscQueue::new()),
            control: None,
            config,
            #[cfg(feature = "serialization")]
            graph_doc: None,
            #[cfg(feature = "osc")]
            control_shared: None,
            #[cfg(feature = "osc")]
            osc_surface: Vec::new(),
            #[cfg(feature = "io")]
            audio: None,
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

    /// Start audio, control, and OSC subsystems according to configuration.
    #[cfg(feature = "serialization")]
    pub async fn start(&mut self) -> Result<(), RuntimeError> {
        #[cfg(feature = "io")]
        if let Some(ref backend) = self.config.audio_backend.clone() {
            let doc = self.graph_doc.take().ok_or(RuntimeError::Graph(
                "no graph document loaded".into(),
            ))?;
            self.start_audio(&doc, backend)?;
        }

        #[cfg(feature = "osc")]
        if let Some(ref bind) = self.config.osc_bind.clone() {
            self.start_osc(bind).await?;
        }

        Ok(())
    }

    /// Start the audio engine.
    #[cfg(all(feature = "io", feature = "serialization"))]
    pub fn start_audio(
        &mut self,
        doc: &GraphDocument,
        backend: &str,
    ) -> Result<(), RuntimeError> {
        if self.audio.is_some() {
            return Err(RuntimeError::Audio("already running".into()));
        }

        // Register backend before graph construction (node constructors read it).
        let _reg = create_and_register_backend(
            backend,
            self.config.sample_rate as u32,
            self.config.audio_input.as_deref(),
            self.config.audio_output.as_deref(),
        )?;

        let registry = registration::registry::<BUF_SIZE>();
        let builder = doc
            .clone()
            .into_builder::<f32, BUF_SIZE>(registry)
            .map_err(|e| RuntimeError::Graph(format!("into_builder: {e}")))?;

        registration::clear_audio_backend();

        let handle = AudioHandle::start(
            builder,
            self.config.sample_rate,
        )
        .map_err(|e| RuntimeError::Audio(e))?;

        self.audio = Some(handle);
        log::info!("audio engine started ({backend})");
        Ok(())
    }

    /// Stop the audio engine.
    #[cfg(feature = "io")]
    pub fn stop_audio(&mut self) {
        if self.audio.take().is_some() {
            log::info!("audio engine stopped");
        }
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
            .map_err(|e| RuntimeError::Osc(e))?;

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

        #[cfg(feature = "io")]
        self.stop_audio();

        log::info!("runtime stopped");
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop();
    }
}

// ─── Backend factory ─────────────────────────────────────────────

/// Create and register a backend. The pointer is read by node constructors.
#[cfg(all(feature = "io", feature = "serialization"))]
fn create_and_register_backend(
    name: &str,
    sample_rate: u32,
    input_device: Option<&str>,
    output_device: Option<&str>,
) -> Result<(), RuntimeError> {
    use crate::io::AudioConfig;

    let mut config = AudioConfig::new()
        .with_sample_rate(sample_rate)
        .with_buffer_size(engine::BUF_SIZE as u32)
        .with_channels(2);
    if let Some(d) = input_device {
        config = config.with_input_device(d);
    }
    if let Some(d) = output_device {
        config = config.with_output_device(d);
    }

    #[cfg(feature = "pipewire")]
    if name == "pipewire" {
        let backend = Box::new(
            PipewireBackend::new(config).map_err(|e| RuntimeError::Audio(e.to_string()))?,
        );
        let ptr: *const dyn AudioIo = &*backend;
        registration::set_audio_backend(ptr);
        std::mem::forget(backend);
        return Ok(());
    }

    Err(RuntimeError::Audio(format!("unsupported backend: {name}")))
}
