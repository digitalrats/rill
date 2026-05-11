//! Automaton factory — type-registry for automaton construction.
//!
//! Follows the same pattern as [`NodeFactory`](rill_graph::NodeFactory):
//! constructors are registered by type name and produce ready-to-use
//! [`BoxedModule`] instances (Servo + automaton, pre-wired).

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use rill_core::traits::{NodeId, Params};

use crate::engine::{BoxedModule, ParameterMapping};

/// Error during automaton construction.
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

/// Target parameter description for a servo.
#[derive(Debug, Clone)]
pub struct ServoTarget {
    pub target_node: NodeId,
    pub target_param: String,
    pub mapping: ParameterMapping,
    pub min: f64,
    pub max: f64,
}

/// Constructor for a named automaton type.
pub trait AutomatonConstructor: Send + Sync {
    /// Canonical type name (e.g. `"lfo"`, `"sequencer"`).
    fn type_name(&self) -> &'static str;

    /// Build a fully wired [`BoxedModule`] (Servo + automaton).
    fn construct(&self, id: &str, params: &Params, target: &ServoTarget) -> BoxedModule;

    /// Clone this constructor into a boxed trait object.
    fn clone_box(&self) -> Box<dyn AutomatonConstructor>;
}

/// Type-registry for automaton construction by type name.
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
        let name = ctor.type_name();
        self.entries.insert(name, Box::new(ctor));
    }

    pub fn register_fn(
        &mut self,
        type_name: &'static str,
        f: impl Fn(&str, &Params, &ServoTarget) -> BoxedModule + Send + Sync + 'static,
    ) {
        let ctor = ClosureCtor::new(type_name, f);
        self.entries.insert(type_name, Box::new(ctor));
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
