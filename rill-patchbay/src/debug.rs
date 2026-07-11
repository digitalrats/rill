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
    /// Automaton identifier (servo name).
    pub name: String,
    /// Whether the automaton is enabled.
    pub enabled: bool,
    /// Current output value.
    pub value: f64,
    /// Additional state fields (time, base, frozen, etc.).
    pub extra: HashMap<String, f64>,
}

/// A snapshot of a sensor's current status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorSnapshot {
    /// Sensor identifier.
    pub name: String,
    /// Sensor type ("osc", "midi").
    pub kind: String,
    /// Whether the sensor is connected and polling.
    pub connected: bool,
    /// Total events received.
    pub event_count: u64,
    /// Last event description, if any.
    pub last_event: Option<String>,
    /// Whether a MIDI clock tracker is active.
    pub tracker_active: bool,
}

/// Interface for types that can produce an automaton snapshot.
pub trait AutomatonInspector: Send + Sync {
    /// Capture the current automaton state.
    fn snapshot(&self) -> AutomatonSnapshot;
}

/// Interface for types that can produce a sensor snapshot.
pub trait SensorInspector: Send + Sync {
    /// Capture the current sensor state.
    fn snapshot(&self) -> SensorSnapshot;
}

/// Central inspector registry for patchbay control-path components.
pub struct PatchbayInspector {
    automatons: DashMap<String, Box<dyn AutomatonInspector>>,
    sensors: DashMap<String, Box<dyn SensorInspector>>,
}

impl PatchbayInspector {
    /// Create an empty inspector registry.
    pub fn new() -> Self {
        Self {
            automatons: DashMap::new(),
            sensors: DashMap::new(),
        }
    }

    /// Register an automaton state provider.
    pub fn register_automaton(&self, name: String, inspector: Box<dyn AutomatonInspector>) {
        self.automatons.insert(name, inspector);
    }

    /// Register a sensor state provider.
    pub fn register_sensor(&self, name: String, inspector: Box<dyn SensorInspector>) {
        self.sensors.insert(name, inspector);
    }

    /// List names of all registered automatons.
    pub fn list_automatons(&self) -> Vec<String> {
        self.automatons.iter().map(|e| e.key().clone()).collect()
    }

    /// Get a snapshot of a specific automaton's state.
    pub fn get_automaton_snapshot(&self, name: &str) -> Option<AutomatonSnapshot> {
        self.automatons.get(name).map(|entry| entry.snapshot())
    }

    /// List names of all registered sensors.
    pub fn list_sensors(&self) -> Vec<String> {
        self.sensors.iter().map(|e| e.key().clone()).collect()
    }

    /// Get a snapshot of a specific sensor's state.
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
