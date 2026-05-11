//! Automaton factory — type-registry for automaton construction.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use rill_core::queues::SetParameter;
use rill_core::traits::{NodeId, Params};
use rill_core_actor::ActorRef;

use crate::engine::{BoxedModule, ParameterMapping};

#[derive(Debug, Clone)]
pub enum FactoryError {
    UnknownType(String),
}

impl fmt::Display for FactoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownType(t) => write!(f, "unknown automaton type: {t}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServoTarget {
    pub target_node: NodeId,
    pub target_param: String,
    pub mapping: ParameterMapping,
    pub min: f64,
    pub max: f64,
}

pub trait AutomatonConstructor: Send + Sync {
    fn type_name(&self) -> &'static str;
    fn construct(&self, id: &str, params: &Params, target: &ServoTarget) -> BoxedModule;
    fn spawn_async(
        &self,
        id: &str,
        params: &Params,
        target: &ServoTarget,
        interval_ms: f64,
        command_queue: ActorRef<SetParameter>,
    ) -> Option<tokio::task::JoinHandle<()>> {
        None
    }
    fn clone_box(&self) -> Box<dyn AutomatonConstructor>;
}

pub struct AutomatonFactory {
    entries: HashMap<&'static str, Box<dyn AutomatonConstructor>>,
}

impl AutomatonFactory {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
    pub fn register(&mut self, ctor: impl AutomatonConstructor + 'static) {
        self.entries.insert(ctor.type_name(), Box::new(ctor));
    }
    pub fn register_fn(
        &mut self,
        type_name: &'static str,
        f: impl Fn(&str, &Params, &ServoTarget) -> BoxedModule + Send + Sync + 'static,
    ) {
        self.entries
            .insert(type_name, Box::new(ClosureCtor::new(type_name, f)));
    }
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
    pub fn spawn_async(
        &self,
        type_name: &str,
        id: &str,
        params: &Params,
        target: &ServoTarget,
        interval_ms: f64,
        command_queue: ActorRef<SetParameter>,
    ) -> Result<Option<tokio::task::JoinHandle<()>>, FactoryError> {
        self.entries
            .get(type_name)
            .ok_or_else(|| FactoryError::UnknownType(type_name.to_string()))
            .map(|ctor| ctor.spawn_async(id, params, target, interval_ms, command_queue))
    }
    pub fn contains(&self, type_name: &str) -> bool {
        self.entries.contains_key(type_name)
    }
    pub fn list_types(&self) -> Vec<&'static str> {
        self.entries.keys().copied().collect()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
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
