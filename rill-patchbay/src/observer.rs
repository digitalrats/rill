//! Micro-control observer — RT safety monitor with actor-based telemetry.

use parking_lot::RwLock;
use rill_core::queues::telemetry::Telemetry;
use rill_core::traits::ParameterId;
use rill_core_actor::ActorRef;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Component statistics
#[derive(Debug, Clone, Default)]
pub struct ComponentStats {
    /// Number of operations performed.
    pub operations: u64,
    /// Total execution time in nanoseconds.
    pub total_time_ns: u64,
    /// Maximum observed execution time in nanoseconds.
    pub max_time_ns: u64,
    /// Number of timing violations.
    pub violations: u64,
    /// Average execution time in nanoseconds.
    pub avg_time_ns: f64,
}

/// Violation record
#[derive(Debug, Clone)]
pub struct Violation {
    /// Name of the component that violated its time budget.
    pub component: String,
    /// Expected execution time in nanoseconds.
    pub expected_ns: u64,
    /// Actual execution time in nanoseconds.
    pub actual_ns: u64,
    /// When the violation occurred (microseconds since UNIX epoch).
    pub timestamp: u64,
    /// Optional value associated with the violation.
    pub value: Option<f32>,
}

/// Sandbox summary
#[derive(Debug, Default, Clone)]
pub struct SandboxSummary {
    /// Total number of operations across all components.
    pub total_operations: u64,
    /// Total number of violations across all components.
    pub total_violations: u64,
    /// Names of active components.
    pub components: Vec<String>,
    /// Maximum operation time across all components.
    pub max_time_ns: u64,
    /// Name of the component with the maximum operation time.
    pub max_time_component: Option<String>,
    /// Number of recorded violations.
    pub violations_count: usize,
}

/// Micro-control observer using `ActorRef<Telemetry>` for event dispatch.
#[derive(Clone)]
pub struct MicroControlObserver {
    stats: Arc<RwLock<HashMap<String, ComponentStats>>>,
    violations: Arc<RwLock<Vec<Violation>>>,
    telemetry_tx: Option<ActorRef<Telemetry>>,
}

impl Default for MicroControlObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl MicroControlObserver {
    /// Create an observer without telemetry reporting.
    pub fn new() -> Self {
        Self {
            stats: Arc::new(RwLock::new(HashMap::new())),
            violations: Arc::new(RwLock::new(Vec::new())),
            telemetry_tx: None,
        }
    }

    /// Create an observer that sends events to the given actor.
    pub fn with_actor(tx: ActorRef<Telemetry>) -> Self {
        Self {
            stats: Arc::new(RwLock::new(HashMap::new())),
            violations: Arc::new(RwLock::new(Vec::new())),
            telemetry_tx: Some(tx),
        }
    }

    fn send_telemetry(&self, event: Telemetry) {
        if let Some(ref tx) = self.telemetry_tx {
            tx.send(event);
        }
    }

    /// Start observing an operation for the given component.
    pub fn observe_start(&self, component: &str) -> OperationGuard {
        OperationGuard {
            component: component.to_string(),
            start_time: Self::now(),
            observer: self.clone(),
        }
    }

    /// Start observing with parameter context (port + parameter).
    pub fn observe_start_with_params(
        &self,
        component: &str,
        port: String,
        _parameter: &ParameterId,
    ) -> OperationGuard {
        let guard = self.observe_start(component);
        self.send_telemetry(Telemetry::event(
            "observer",
            "micro_start",
            vec![0.0_f32, 0.0_f32],
        ));
        guard
    }

    /// Record a timing violation for a component.
    pub fn record_violation(
        &self,
        component: &str,
        expected_ns: u64,
        actual_ns: u64,
        value: Option<f32>,
    ) {
        let violation = Violation {
            component: component.to_string(),
            expected_ns,
            actual_ns,
            timestamp: Self::now(),
            value,
        };
        self.violations.write().push(violation.clone());
        let mut stats = self.stats.write();
        let comp_stats = stats.entry(component.to_string()).or_default();
        comp_stats.violations += 1;
        self.send_telemetry(Telemetry::violation(
            component,
            expected_ns,
            actual_ns,
            value,
        ));
        println!(
            "⚠️ Violation in {}: {}ns (expected {}ns)",
            component, actual_ns, expected_ns
        );
    }

    /// Get statistics for a specific component.
    pub fn component_stats(&self, component: &str) -> Option<ComponentStats> {
        self.stats.read().get(component).cloned()
    }

    /// Get all recorded violations.
    pub fn violations(&self) -> Vec<Violation> {
        self.violations.read().clone()
    }

    /// Get a summary of the entire sandbox.
    pub fn sandbox_summary(&self) -> SandboxSummary {
        let stats = self.stats.read();
        let mut summary = SandboxSummary::default();
        for (component, comp_stats) in stats.iter() {
            summary.total_operations += comp_stats.operations;
            summary.total_violations += comp_stats.violations;
            summary.components.push(component.clone());
            if comp_stats.max_time_ns > summary.max_time_ns {
                summary.max_time_ns = comp_stats.max_time_ns;
                summary.max_time_component = Some(component.clone());
            }
        }
        summary.violations_count = self.violations.read().len();
        summary
    }

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64
    }
}

/// Guard that automatically records operation completion
pub struct OperationGuard {
    component: String,
    start_time: u64,
    observer: MicroControlObserver,
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        let duration = (Self::now() - self.start_time) * 1000;
        let mut stats = self.observer.stats.write();
        let comp_stats = stats.entry(self.component.clone()).or_default();
        comp_stats.operations += 1;
        comp_stats.total_time_ns += duration;
        if duration > comp_stats.max_time_ns {
            comp_stats.max_time_ns = duration;
        }
        comp_stats.avg_time_ns = comp_stats.total_time_ns as f64 / comp_stats.operations as f64;
        self.observer.send_telemetry(Telemetry::event(
            "observer",
            "micro_complete",
            vec![duration as f32],
        ));
    }
}

impl OperationGuard {
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core_actor::ActorSystem;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_observer_creation() {
        let observer = MicroControlObserver::new();
        let stats = observer.sandbox_summary();
        assert_eq!(stats.total_operations, 0);
    }

    #[test]
    fn test_observer_record_violation() {
        let system = ActorSystem::new();
        let received = Arc::new(Mutex::new(Vec::new()));
        let recv = received.clone();
        let mut actor = system.spawn("telemetry", move |msg: Telemetry| {
            recv.lock().unwrap().push(msg);
        });
        let observer = MicroControlObserver::with_actor(actor.actor_ref());
        observer.record_violation("test_comp", 100, 250, Some(0.5));
        let stats = observer.sandbox_summary();
        assert_eq!(stats.total_violations, 1);
        let violations = observer.violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].component, "test_comp");
        actor.drain();
        let events = received.lock().unwrap();
        for evt in events.iter() {
            if let Telemetry::Violation { component, .. } = evt {
                assert_eq!(component, "test_comp");
            }
        }
    }

    #[test]
    fn test_observer_operation_guard() {
        let system = ActorSystem::new();
        let received = Arc::new(Mutex::new(Vec::new()));
        let recv = received.clone();
        let mut actor = system.spawn("telemetry", move |msg: Telemetry| {
            recv.lock().unwrap().push(msg);
        });
        let observer = MicroControlObserver::with_actor(actor.actor_ref());
        {
            let _guard = observer.observe_start("test_op");
            std::thread::sleep(std::time::Duration::from_micros(10));
        }
        let stats = observer.sandbox_summary();
        assert_eq!(stats.total_operations, 1);
        actor.drain();
        let events = received.lock().unwrap();
        for evt in events.iter() {
            if let Telemetry::Event { kind, .. } = evt {
                assert_eq!(kind, "micro_complete");
            }
        }
    }
}
