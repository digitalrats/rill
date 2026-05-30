//! Module factory — type-registry for modular rack module construction.
//!
//! `ModuleFactory` is the single creation point for all rack modules:
//! Servo, Sensor, Graph, and Custom. Each archetype registers a
//! [`ModuleConstructor`] that receives a [`ModuleDef`] descriptor
//! and returns an [`ActorRef<CommandEnum>`] for the rack actor fan-out.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use rill_core::queues::CommandEnum;
use rill_core::traits::ParamValue;
use rill_core_actor::{ActorRef, ActorSystem};

use crate::module_def::{AutomatonDef, ModuleDef};

/// Errors returned by module construction via the type-registry.
#[derive(Debug, Clone)]
pub enum ModuleError {
    /// The requested module type name was not registered.
    UnknownType(String),
    /// Construction failed with a user-readable reason.
    ConstructionFailed(String),
}

impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownType(t) => write!(f, "unknown module type: {t}"),
            Self::ConstructionFailed(e) => write!(f, "module construction failed: {e}"),
        }
    }
}

/// Constructs rack modules from a [`ModuleDef`] descriptor.
///
/// Each constructor receives the full module descriptor plus the list of
/// automaton definitions needed by `Servo` modules. Custom and registered
/// constructors return an [`ActorRef<CommandEnum>`] that the rack actor
/// uses for fan-out.
pub trait ModuleConstructor: Send + Sync {
    /// Returns the string key used to register this constructor.
    fn type_name(&self) -> &'static str;

    /// Build the module and return its actor handle.
    ///
    /// `automaton_defs` provides the automaton definitions referenced
    /// by `ModuleDef::Servo`. Other module types ignore this parameter.
    fn construct(
        &self,
        module: &ModuleDef,
        automaton_defs: &[AutomatonDef],
        system: &Arc<ActorSystem>,
        graph_ref: &ActorRef<CommandEnum>,
    ) -> Result<ActorRef<CommandEnum>, ModuleError>;

    /// Returns a heap-allocated clone of this constructor.
    fn clone_box(&self) -> Box<dyn ModuleConstructor>;
}

/// How the actor's drain loop is spawned.
#[derive(Debug, Clone, Copy)]
pub enum Drain {
    /// OS thread with periodic drain (handler: !Send).
    OsThread {
        /// Drain interval in milliseconds.
        interval_ms: u64,
    },
    /// Tokio task with periodic drain (handler: Send).
    TokioTask {
        /// Drain interval in milliseconds.
        interval_ms: u64,
    },
    /// I/O callback drain — handler drained inline in the backend callback.
    /// Factory spawns the I/O thread, construction closures run inside it.
    IoCallback,
}
/// Registry that maps module type names to constructors.
pub struct ModuleFactory {
    entries: HashMap<String, Box<dyn ModuleConstructor>>,
}

impl ModuleFactory {
    /// Creates an empty module factory.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
    /// Adds a typed constructor to the registry, keyed by its type name.
    pub fn register(&mut self, ctor: impl ModuleConstructor + 'static) {
        self.entries
            .insert(ctor.type_name().to_string(), Box::new(ctor));
    }

    /// Register a closure-based constructor (handler: !Send).
    ///
    /// `make_handler` receives module params and the graph handle, and returns
    /// the message handler closure. The factory calls it **inside the drain thread**,
    /// so the handler does not need `Send`.
    #[allow(dead_code)]
    pub fn register_fn(
        &mut self,
        type_name: impl Into<String>,
        drain: Drain,
        make_handler: impl Fn(
                &str,
                &HashMap<String, ParamValue>,
                &ActorRef<CommandEnum>,
            ) -> Box<dyn FnMut(CommandEnum) + 'static>
            + Send
            + Sync
            + 'static,
    ) {
        self.entries.insert(
            type_name.into(),
            Box::new(ClosureCtor::new_erased(drain, make_handler)),
        );
    }

    /// Register a closure-based constructor (handler: `Send`, for [`Drain::TokioTask`]).
    ///
    /// The returned handler must be `Send` so it can be stored in a tokio future.
    #[allow(dead_code)]
    pub fn register_fn_send(
        &mut self,
        type_name: impl Into<String>,
        drain: Drain,
        make_handler: impl Fn(
                &str,
                &HashMap<String, ParamValue>,
                &ActorRef<CommandEnum>,
            ) -> Box<dyn FnMut(CommandEnum) + Send + 'static>
            + Send
            + Sync
            + 'static,
    ) {
        self.entries.insert(
            type_name.into(),
            Box::new(ClosureCtor::new_send(drain, make_handler)),
        );
    }

    /// Looks up a type name and constructs the corresponding module actor.
    pub fn construct(
        &self,
        module: &ModuleDef,
        automaton_defs: &[AutomatonDef],
        system: &Arc<ActorSystem>,
        graph_ref: &ActorRef<CommandEnum>,
    ) -> Result<ActorRef<CommandEnum>, ModuleError> {
        let type_name = module.type_name();
        self.entries
            .get(type_name)
            .ok_or_else(|| ModuleError::UnknownType(type_name.to_string()))
            .and_then(|ctor| ctor.construct(module, automaton_defs, system, graph_ref))
    }

    /// Checks whether a type name is registered.
    pub fn contains(&self, type_name: &str) -> bool {
        self.entries.contains_key(type_name)
    }
    /// Returns the number of registered module types.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// Returns true if no module types are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for ModuleFactory {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// ClosureCtor
// ============================================================================

type ErasedCtorFn = Arc<
    dyn Fn(
            &str,
            &HashMap<String, ParamValue>,
            &ActorRef<CommandEnum>,
        ) -> Box<dyn FnMut(CommandEnum) + 'static>
        + Send
        + Sync,
>;

type SendCtorFn = Arc<
    dyn Fn(
            &str,
            &HashMap<String, ParamValue>,
            &ActorRef<CommandEnum>,
        ) -> Box<dyn FnMut(CommandEnum) + Send + 'static>
        + Send
        + Sync,
>;

enum ClosureCtorKind {
    Erased { f: ErasedCtorFn },
    Send { f: SendCtorFn },
}

struct ClosureCtor {
    drain: Drain,
    kind: ClosureCtorKind,
}

impl ClosureCtor {
    fn new_erased(
        drain: Drain,
        f: impl Fn(
                &str,
                &HashMap<String, ParamValue>,
                &ActorRef<CommandEnum>,
            ) -> Box<dyn FnMut(CommandEnum) + 'static>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            drain,
            kind: ClosureCtorKind::Erased { f: Arc::new(f) },
        }
    }

    fn new_send(
        drain: Drain,
        f: impl Fn(
                &str,
                &HashMap<String, ParamValue>,
                &ActorRef<CommandEnum>,
            ) -> Box<dyn FnMut(CommandEnum) + Send + 'static>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            drain,
            kind: ClosureCtorKind::Send { f: Arc::new(f) },
        }
    }
}

impl ModuleConstructor for ClosureCtor {
    fn type_name(&self) -> &'static str {
        ""
    }
    fn construct(
        &self,
        module: &ModuleDef,
        _automaton_defs: &[AutomatonDef],
        system: &Arc<ActorSystem>,
        graph_ref: &ActorRef<CommandEnum>,
    ) -> Result<ActorRef<CommandEnum>, ModuleError> {
        let ModuleDef::Custom {
            type_name: _,
            params,
        } = module
        else {
            return Err(ModuleError::ConstructionFailed(
                "ClosureCtor only supports Custom modules".into(),
            ));
        };

        let id_owned = String::new(); // Custom modules use type_name as id
        let name = "custom".to_string();
        let graph_ref = graph_ref.clone();
        let params = params.clone();

        match (&self.kind, self.drain) {
            (ClosureCtorKind::Erased { f }, Drain::OsThread { interval_ms }) => {
                let f = f.clone();
                let actor_ref = system.spawn_detached(
                    &name,
                    move || f(&id_owned, &params, &graph_ref),
                    interval_ms,
                );
                Ok(actor_ref)
            }
            (ClosureCtorKind::Send { f }, Drain::OsThread { interval_ms }) => {
                let f = f.clone();
                let actor_ref = system.spawn_detached(
                    &name,
                    move || f(&id_owned, &params, &graph_ref),
                    interval_ms,
                );
                Ok(actor_ref)
            }
            (ClosureCtorKind::Send { f }, Drain::TokioTask { interval_ms }) => {
                let f = f.clone();
                let actor_ref = system.spawn_detached_tokio(
                    &name,
                    move || f(&id_owned, &params, &graph_ref),
                    interval_ms,
                );
                Ok(actor_ref)
            }
            (ClosureCtorKind::Erased { .. }, Drain::TokioTask { .. }) => {
                Err(ModuleError::ConstructionFailed(
                    "TokioTask drain requires a Send handler; use register_fn_send()".into(),
                ))
            }
            (ClosureCtorKind::Erased { .. }, Drain::IoCallback) => {
                Err(ModuleError::ConstructionFailed(
                    "IoCallback drain not supported via register_fn(); use Graph constructor directly".into(),
                ))
            }
            (ClosureCtorKind::Send { .. }, Drain::IoCallback) => {
                Err(ModuleError::ConstructionFailed(
                    "IoCallback drain not supported via register_fn_send(); use Graph constructor directly".into(),
                ))
            }
        }
    }
    fn clone_box(&self) -> Box<dyn ModuleConstructor> {
        match &self.kind {
            ClosureCtorKind::Erased { f } => Box::new(ClosureCtor {
                drain: self.drain,
                kind: ClosureCtorKind::Erased { f: f.clone() },
            }),
            ClosureCtorKind::Send { f } => Box::new(ClosureCtor {
                drain: self.drain,
                kind: ClosureCtorKind::Send { f: f.clone() },
            }),
        }
    }
}
