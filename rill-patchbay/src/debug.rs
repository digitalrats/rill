//! Patchbay inspector — runtime introspection of automatons, sensors, and queues.
//!
//! Provides snapshot types and trait interfaces for inspecting the control-path
//! state of servos, sensors, and other patchbay components at runtime.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::engine::Automaton;

/// A snapshot of an automaton's current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomatonSnapshot {
    pub name: String,
    pub enabled: bool,
    pub value: f64,
    pub extra: HashMap<String, f64>,
}

/// A snapshot of a sensor's current status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorSnapshot {
    pub name: String,
    pub kind: String,
    pub connected: bool,
    pub event_count: u64,
    pub last_event: Option<String>,
    pub tracker_active: bool,
}

/// Interface for types that can produce an automaton snapshot.
pub trait AutomatonInspector: Send + Sync {
    fn snapshot(&self) -> AutomatonSnapshot;
}

/// Interface for types that can produce a sensor snapshot.
pub trait SensorInspector: Send + Sync {
    fn snapshot(&self) -> SensorSnapshot;
}

/// Central inspector registry for patchbay control-path components.
pub struct PatchbayInspector {
    automatons: DashMap<String, Box<dyn AutomatonInspector>>,
    sensors: DashMap<String, Box<dyn SensorInspector>>,
}

impl PatchbayInspector {
    pub fn new() -> Self {
        Self {
            automatons: DashMap::new(),
            sensors: DashMap::new(),
        }
    }

    pub fn register_automaton(&self, name: String, inspector: Box<dyn AutomatonInspector>) {
        self.automatons.insert(name, inspector);
    }

    pub fn register_sensor(&self, name: String, inspector: Box<dyn SensorInspector>) {
        self.sensors.insert(name, inspector);
    }

    pub fn list_automatons(&self) -> Vec<String> {
        self.automatons.iter().map(|e| e.key().clone()).collect()
    }

    pub fn get_automaton_snapshot(&self, name: &str) -> Option<AutomatonSnapshot> {
        self.automatons.get(name).map(|entry| entry.snapshot())
    }

    pub fn list_sensors(&self) -> Vec<String> {
        self.sensors.iter().map(|e| e.key().clone()).collect()
    }

    pub fn get_sensor_snapshot(&self, name: &str) -> Option<SensorSnapshot> {
        self.sensors.get(name).map(|entry| entry.snapshot())
    }
}

impl Default for PatchbayInspector {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct ServoInspector<A: Automaton> {
    pub(crate) name: String,
    pub(crate) state: Arc<Mutex<crate::engine::ServoState<A>>>,
}

impl<A: Automaton> AutomatonInspector for ServoInspector<A> {
    fn snapshot(&self) -> AutomatonSnapshot {
        let s = self.state.lock().unwrap();
        let mut extra = HashMap::new();
        extra.insert("time".into(), s.time);
        extra.insert("base".into(), s.base);
        extra.insert("frozen".into(), if s.frozen { 1.0 } else { 0.0 });
        AutomatonSnapshot {
            name: self.name.clone(),
            enabled: s.enabled,
            value: s.value.as_f32().unwrap_or(s.base as f32) as f64,
            extra,
        }
    }
}
