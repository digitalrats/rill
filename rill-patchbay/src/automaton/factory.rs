//! Automaton factory — type-registry for automaton construction.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use rill_core::queues::CommandEnum;
use rill_core::traits::{NodeId, ParamValue, Params};
use rill_core_actor::ActorRef;

use crate::engine::{BoxedModule, ParameterMapping};
/// Errors returned by automaton construction via the type-registry.
#[derive(Debug, Clone)]
pub enum FactoryError {
    /// The requested automaton type name was not registered.
    UnknownType(String),
}

impl fmt::Display for FactoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownType(t) => write!(f, "unknown automaton type: {t}"),
        }
    }
}
/// Destination descriptor for a servo-driven parameter automation.
#[derive(Debug, Clone)]
pub struct ServoTarget {
    /// The graph node that receives the parameter value.
    pub target_node: NodeId,
    /// The parameter name on the target node to update.
    pub target_param: String,
    /// How the automaton output maps to the parameter.
    pub mapping: ParameterMapping,
    /// Minimum output clamp value.
    pub min: f64,
    /// Maximum output clamp value.
    pub max: f64,
    /// Optional value table for index-based automata.
    pub table: Option<Vec<ParamValue>>,
}
/// Constructs automaton modules from type name, params, and target.
pub trait AutomatonConstructor: Send + Sync {
    /// Returns the string key used to register this constructor.
    fn type_name(&self) -> &'static str;
    /// Builds a boxed module from the given id, params, and servo target.
    fn construct(&self, id: &str, params: &Params, target: &ServoTarget) -> BoxedModule;
    /// Optionally spawns an async green-thread for automata that run independently of the graph clock.
    fn spawn_async(
        &self,
        _id: &str,
        _params: &Params,
        _target: &ServoTarget,
        _interval_ms: f64,
        _command_queue: ActorRef<CommandEnum>,
    ) -> Option<tokio::task::JoinHandle<()>> {
        None
    }
    /// Returns a heap-allocated clone of this constructor.
    fn clone_box(&self) -> Box<dyn AutomatonConstructor>;
}
/// Registry that maps automaton type names to constructors.
pub struct AutomatonFactory {
    entries: HashMap<&'static str, Box<dyn AutomatonConstructor>>,
}

impl AutomatonFactory {
    /// Creates an empty automaton factory.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
    /// Adds a typed constructor to the registry, keyed by its type name.
    pub fn register(&mut self, ctor: impl AutomatonConstructor + 'static) {
        self.entries.insert(ctor.type_name(), Box::new(ctor));
    }
    /// Registers a closure-based constructor under the given type name.
    pub fn register_fn(
        &mut self,
        type_name: &'static str,
        f: impl Fn(&str, &Params, &ServoTarget) -> BoxedModule + Send + Sync + 'static,
    ) {
        self.entries
            .insert(type_name, Box::new(ClosureCtor::new(type_name, f)));
    }
    /// Looks up a type name and constructs the corresponding automaton module.
    pub fn construct(
        &self,
        type_name: &str,
        id: &str,
        params: &Params,
        target: &ServoTarget,
    ) -> Result<BoxedModule, FactoryError> {
        self.entries
            .get(type_name)
            .ok_or_else(|| FactoryError::UnknownType(type_name.to_string()))
            .map(|ctor| ctor.construct(id, params, target))
    }
    /// Looks up a type name and spawns its async green-thread if supported.
    pub fn spawn_async(
        &self,
        type_name: &str,
        id: &str,
        params: &Params,
        target: &ServoTarget,
        interval_ms: f64,
        command_queue: ActorRef<CommandEnum>,
    ) -> Result<Option<tokio::task::JoinHandle<()>>, FactoryError> {
        self.entries
            .get(type_name)
            .ok_or_else(|| FactoryError::UnknownType(type_name.to_string()))
            .map(|ctor| ctor.spawn_async(id, params, target, interval_ms, command_queue))
    }
    /// Checks whether a type name is registered.
    pub fn contains(&self, type_name: &str) -> bool {
        self.entries.contains_key(type_name)
    }
    /// Lists all registered automaton type names.
    pub fn list_types(&self) -> Vec<&'static str> {
        self.entries.keys().copied().collect()
    }
    /// Returns the number of registered automaton types.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// Returns true if no automaton types are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for AutomatonFactory {
    fn default() -> Self {
        Self::new()
    }
}

type ClosureCtorFn = Arc<dyn Fn(&str, &Params, &ServoTarget) -> BoxedModule + Send + Sync>;

struct ClosureCtor {
    type_name: &'static str,
    f: ClosureCtorFn,
}

impl ClosureCtor {
    fn new(
        type_name: &'static str,
        f: impl Fn(&str, &Params, &ServoTarget) -> BoxedModule + Send + Sync + 'static,
    ) -> Self {
        Self {
            type_name,
            f: Arc::new(f),
        }
    }
}

impl AutomatonConstructor for ClosureCtor {
    fn type_name(&self) -> &'static str {
        self.type_name
    }
    fn construct(&self, id: &str, params: &Params, target: &ServoTarget) -> BoxedModule {
        (self.f)(id, params, target)
    }
    fn clone_box(&self) -> Box<dyn AutomatonConstructor> {
        Box::new(ClosureCtor {
            type_name: self.type_name,
            f: self.f.clone(),
        })
    }
}
