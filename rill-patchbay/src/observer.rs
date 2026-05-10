//! Micro-control observer — RT safety monitor with actor-based telemetry.

use rill_core::queues::telemetry::Telemetry;
use rill_core::traits::{ParameterId, PortId};
use rill_core_actor::ActorRef;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Component statistics
#[derive(Debug, Clone, Default)]
pub struct ComponentStats {
    pub operations: u64,
    pub total_time_ns: u64,
    pub max_time_ns: u64,
    pub violations: u64,
    pub avg_time_ns: f64,
}

/// Violation record
#[derive(Debug, Clone)]
pub struct Violation {
    pub component: String,
    pub expected_ns: u64,
    pub actual_ns: u64,
    pub timestamp: u64,
    pub value: Option<f32>,
}

/// Sandbox summary
#[derive(Debug, Default, Clone)]
pub struct SandboxSummary {
    pub total_operations: u64,
    pub total_violations: u64,
    pub components: Vec<String>,
    pub max_time_ns: u64,
    pub max_time_component: Option<String>,
    pub violations_count: usize,
}

/// Micro-control observer using `ActorRef<Telemetry>` for event dispatch.
#[derive(Clone)]
pub struct MicroControlObserver {
    stats: Arc<RwLock<HashMap<String, ComponentStats>>>,
    violations: Arc<RwLock<Vec<Violation>>>,
    telemetry_tx: Option<ActorRef<Telemetry>>,
}

impl MicroControlObserver {
    pub fn new() -> Self {
        Self {
            stats: Arc::new(RwLock::new(HashMap::new())),
            violations: Arc::new(RwLock::new(Vec::new())),
            telemetry_tx: None,
        }
    }

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

    pub fn observe_start(&self, component: &str) -> OperationGuard {
        OperationGuard {
            component: component.to_string(),
            start_time: Self::now(),
            observer: self.clone(),
        }
    }

    pub fn observe_start_with_params(
        &self,
        component: &str,
        port: PortId,
        _parameter: &ParameterId,
    ) -> OperationGuard {
        let guard = self.observe_start(component);
        self.send_telemetry(Telemetry::event(
            "observer",
            "micro_start",
            vec![port.node_id().inner() as f32, port.index() as f32],
        ));
        guard
    }

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
        self.send_telemetry(Telemetry::violation(component, expected_ns, actual_ns, value));
        println!(
            "⚠️ Violation in {}: {}ns (expected {}ns)",
            component, actual_ns, expected_ns
        );
    }

    pub fn component_stats(&self, component: &str) -> Option<ComponentStats> {
        self.stats.read().get(component).cloned()
    }

    pub fn violations(&self) -> Vec<Violation> {
        self.violations.read().clone()
    }

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
        self.observer
            .send_telemetry(Telemetry::event("observer", "micro_complete", vec![duration as f32]));
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
    use rill_core_actor::{ActorCell, ActorRef};

    struct CollectActor {
        events: Vec<Telemetry>,
    }
    impl ActorCell for CollectActor {
        type Msg = Telemetry;
        fn receive(&mut self, msg: Telemetry) {
            self.events.push(msg);
        }
    }

    #[test]
    fn test_observer_creation() {
        let observer = MicroControlObserver::new();
        let stats = observer.sandbox_summary();
        assert_eq!(stats.total_operations, 0);
    }

    #[test]
    fn test_observer_record_violation() {
        let (tx, mut rx) = ActorRef::<Telemetry>::new_pair();
        let observer = MicroControlObserver::with_actor(tx);
        observer.record_violation("test_comp", 100, 250, Some(0.5));
        let stats = observer.sandbox_summary();
        assert_eq!(stats.total_violations, 1);
        let violations = observer.violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].component, "test_comp");
        while let Some(evt) = rx.pop() {
            if let Telemetry::Violation { component, .. } = evt {
                assert_eq!(component, "test_comp");
            }
        }
    }

    #[test]
    fn test_observer_operation_guard() {
        let (tx, mut rx) = ActorRef::<Telemetry>::new_pair();
        let observer = MicroControlObserver::with_actor(tx);
        {
            let _guard = observer.observe_start("test_op");
            std::thread::sleep(std::time::Duration::from_micros(10));
        }
        let stats = observer.sandbox_summary();
        assert_eq!(stats.total_operations, 1);
        while let Some(evt) = rx.pop() {
            if let Telemetry::Event { kind, .. } = evt {
                assert_eq!(kind, "micro_complete");
            }
        }
    }
}
