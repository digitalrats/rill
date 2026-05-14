//! Module factory — type-registry for custom rack module construction.
//!
//! Unlike [`crate::automaton::factory::AutomatonFactory`] which is tied to
//! the Servo+Automaton pattern, `ModuleFactory` lets applications register
//! arbitrary [`crate::engine::Module`] constructors (e.g. STC players,
//! external sequencers) that receive `ClockTick` via the rack actor.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use rill_core::queues::CommandEnum;
use rill_core::traits::ParamValue;
use rill_core_actor::{ActorRef, ActorSystem};

use crate::engine::BoxedModule;

#[derive(Debug, Clone)]
pub enum FactoryError {
    UnknownType(String),
}

impl fmt::Display for FactoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownType(t) => write!(f, "unknown module type: {t}"),
        }
    }
}

/// Constructor for a custom rack module.
///
/// Implementors are called from [`ModuleFactory::construct`] with the
/// module identifier, parameters, actor system, and graph handle so the
/// constructor can spawn actors and send commands to the audio graph.
pub trait ModuleConstructor: Send + Sync {
    fn type_name(&self) -> &'static str;
    fn construct(
        &self,
        id: &str,
        params: &HashMap<String, ParamValue>,
        system: &Arc<ActorSystem>,
        graph_ref: &ActorRef<CommandEnum>,
    ) -> BoxedModule;
    fn clone_box(&self) -> Box<dyn ModuleConstructor>;
}

/// Registry of custom module constructors.
///
/// # Example
///
/// ```no_run
/// # use std::collections::HashMap;
/// # use std::sync::Arc;
/// # use rill_core::traits::ParamValue;
/// # use rill_core::queues::CommandEnum;
/// # use rill_core_actor::{ActorRef, ActorSystem};
/// # use rill_patchbay::engine::BoxedModule;
/// # use rill_patchbay::module_factory::ModuleFactory;
/// let mut factory = ModuleFactory::new();
/// factory.register_fn("stc_player", move |id, params, system, graph_ref| {
///     // ... spawn actor, return BoxedModule ...
/// #   unimplemented!()
/// });
/// ```
pub struct ModuleFactory {
    entries: HashMap<String, Box<dyn ModuleConstructor>>,
}

impl ModuleFactory {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Register a constructor trait object.
    pub fn register(&mut self, ctor: impl ModuleConstructor + 'static) {
        self.entries
            .insert(ctor.type_name().to_string(), Box::new(ctor));
    }

    /// Register a closure-based constructor.
    pub fn register_fn(
        &mut self,
        type_name: impl Into<String>,
        f: impl Fn(
                &str,
                &HashMap<String, ParamValue>,
                &Arc<ActorSystem>,
                &ActorRef<CommandEnum>,
            ) -> BoxedModule
            + Send
            + Sync
            + 'static,
    ) {
        self.entries
            .insert(type_name.into(), Box::new(ClosureCtor::new(f)));
    }

    /// Construct a module by type name.
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

    /// Check whether a type is registered.
    pub fn contains(&self, type_name: &str) -> bool {
        self.entries.contains_key(type_name)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for ModuleFactory {
    fn default() -> Self {
        Self::new()
    }
}

struct ClosureCtor {
    f: Arc<
        dyn Fn(
                &str,
                &HashMap<String, ParamValue>,
                &Arc<ActorSystem>,
                &ActorRef<CommandEnum>,
            ) -> BoxedModule
            + Send
            + Sync,
    >,
}

impl ClosureCtor {
    fn new(
        f: impl Fn(
                &str,
                &HashMap<String, ParamValue>,
                &Arc<ActorSystem>,
                &ActorRef<CommandEnum>,
            ) -> BoxedModule
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self { f: Arc::new(f) }
    }
}

impl ModuleConstructor for ClosureCtor {
    fn type_name(&self) -> &'static str {
        "" // not needed — keyed by string, not trait
    }
    fn construct(
        &self,
        id: &str,
        params: &HashMap<String, ParamValue>,
        system: &Arc<ActorSystem>,
        graph_ref: &ActorRef<CommandEnum>,
    ) -> BoxedModule {
        (self.f)(id, params, system, graph_ref)
    }
    fn clone_box(&self) -> Box<dyn ModuleConstructor> {
        Box::new(ClosureCtor { f: self.f.clone() })
    }
}
