//! Automaton factory — type-registry for automaton construction.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use rill_core::queues::CommandEnum;
use rill_core::traits::{NodeId, ParamValue, Params};
use rill_core_actor::ActorRef;

use crate::engine::{BoxedModule, ParameterMapping};
/// Documentation.

#[derive(Debug, Clone)]
pub enum FactoryError {
    /// Documentation.
    UnknownType(String),
}

impl fmt::Display for FactoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownType(t) => write!(f, "unknown automaton type: {t}"),
        }
    }
}
/// Documentation.

#[derive(Debug, Clone)]
pub struct ServoTarget {
    /// Documentation.
    pub target_node: NodeId,
    /// Documentation.
    pub target_param: String,
    /// Documentation.
    pub mapping: ParameterMapping,
    /// Documentation.
    pub min: f64,
    /// Documentation.
    pub max: f64,
    /// Optional value table for index-based automata.
    pub table: Option<Vec<ParamValue>>,
}
/// Documentation.

pub trait AutomatonConstructor: Send + Sync {
    /// Documentation.
    fn type_name(&self) -> &'static str;
    /// Documentation.
    fn construct(&self, id: &str, params: &Params, target: &ServoTarget) -> BoxedModule;
    /// Documentation.
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
    /// Documentation.
    fn clone_box(&self) -> Box<dyn AutomatonConstructor>;
}
/// Documentation.

pub struct AutomatonFactory {
    entries: HashMap<&'static str, Box<dyn AutomatonConstructor>>,
}

impl AutomatonFactory {
    /// Documentation.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
    /// Documentation.
    pub fn register(&mut self, ctor: impl AutomatonConstructor + 'static) {
        self.entries.insert(ctor.type_name(), Box::new(ctor));
    }
    /// Documentation.
    pub fn register_fn(
        &mut self,
        type_name: &'static str,
        f: impl Fn(&str, &Params, &ServoTarget) -> BoxedModule + Send + Sync + 'static,
    ) {
        self.entries
            .insert(type_name, Box::new(ClosureCtor::new(type_name, f)));
    }
    /// Documentation.
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
    /// Documentation.
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
    /// Documentation.
    pub fn contains(&self, type_name: &str) -> bool {
        self.entries.contains_key(type_name)
    }
    /// Documentation.
    pub fn list_types(&self) -> Vec<&'static str> {
        self.entries.keys().copied().collect()
    }
    /// Documentation.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// Documentation.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for AutomatonFactory {
    fn default() -> Self {
        Self::new()
    }
}

struct ClosureCtor {
    type_name: &'static str,
    f: Arc<dyn Fn(&str, &Params, &ServoTarget) -> BoxedModule + Send + Sync>,
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
