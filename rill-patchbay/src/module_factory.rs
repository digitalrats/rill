//! Module factory — type-registry for custom rack module construction.
//!
//! Unlike [`crate::automaton::factory::AutomatonFactory`] which is tied to
//! the Servo+Automaton pattern, `ModuleFactory` lets applications register
//! arbitrary modules that receive `ClockTick` via the rack actor.
//!
//! The factory takes care of actor creation, drain loop, and thread spawning
//! via [`rill_core_actor::ActorSystem::spawn_detached`].

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use rill_core::queues::CommandEnum;
use rill_core::traits::ParamValue;
use rill_core_actor::{ActorRef, ActorSystem};

use crate::engine::{BoxedModule, Module};

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
/// Errors returned by module construction via the type-registry.
#[derive(Debug, Clone)]
pub enum FactoryError {
    /// The requested module type name was not registered.
    UnknownType(String),
}

impl fmt::Display for FactoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownType(t) => write!(f, "unknown module type: {t}"),
        }
    }
}
/// Constructs generic rack modules from type name, params, actor system, and graph handle.
pub trait ModuleConstructor: Send + Sync {
    /// Returns the string key used to register this module constructor.
    fn type_name(&self) -> &'static str;
    /// Builds a boxed module, spawning its actor via the provided ActorSystem.
    fn construct(
        &self,
        id: &str,
        params: &HashMap<String, ParamValue>,
        system: &Arc<ActorSystem>,
        graph_ref: &ActorRef<CommandEnum>,
    ) -> BoxedModule;
    /// Returns a heap-allocated clone of this constructor.
    fn clone_box(&self) -> Box<dyn ModuleConstructor>;
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
    /// Looks up a type name and constructs the corresponding module.
    pub fn construct(
        &self,
        type_name: &str,
        id: &str,
        params: &HashMap<String, ParamValue>,
        system: &Arc<ActorSystem>,
        graph_ref: &ActorRef<CommandEnum>,
    ) -> Result<BoxedModule, FactoryError> {
        self.entries
            .get(type_name)
            .ok_or_else(|| FactoryError::UnknownType(type_name.to_string()))
            .map(|ctor| ctor.construct(id, params, system, graph_ref))
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
// GenericModule — returned by factory for custom modules
// ============================================================================

struct GenericModule {
    id: String,
    actor_ref: ActorRef<CommandEnum>,
}

impl Module for GenericModule {
    fn id(&self) -> &str {
        &self.id
    }
    fn handle(&self) -> Option<ActorRef<CommandEnum>> {
        Some(self.actor_ref.clone())
    }
    fn set_enabled(&mut self, _enabled: bool) {}
    fn stop(&mut self) {}
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
        id: &str,
        params: &HashMap<String, ParamValue>,
        system: &Arc<ActorSystem>,
        graph_ref: &ActorRef<CommandEnum>,
    ) -> BoxedModule {
        let id_owned = id.to_string();
        let id_for_mod = id_owned.clone();
        let name = id_owned.clone();
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
                Box::new(GenericModule {
                    id: id_for_mod,
                    actor_ref,
                })
            }
            (ClosureCtorKind::Send { f }, Drain::OsThread { interval_ms }) => {
                let f = f.clone();
                let actor_ref = system.spawn_detached(
                    &name,
                    move || f(&id_owned, &params, &graph_ref),
                    interval_ms,
                );
                Box::new(GenericModule {
                    id: id_for_mod,
                    actor_ref,
                })
            }
            (ClosureCtorKind::Send { f }, Drain::TokioTask { interval_ms }) => {
                let f = f.clone();
                let actor_ref = system.spawn_detached_tokio(
                    &name,
                    move || f(&id_owned, &params, &graph_ref),
                    interval_ms,
                );
                Box::new(GenericModule {
                    id: id_for_mod,
                    actor_ref,
                })
            }
            (ClosureCtorKind::Erased { .. }, Drain::TokioTask { .. }) => {
                panic!("TokioTask drain requires a Send handler; use register_fn_send()")
            }
            (ClosureCtorKind::Erased { .. }, Drain::IoCallback) => {
                panic!("IoCallback drain not supported via register_fn(); use Graph constructor directly")
            }
            (ClosureCtorKind::Send { .. }, Drain::IoCallback) => {
                panic!("IoCallback drain not supported via register_fn_send(); use Graph constructor directly")
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
