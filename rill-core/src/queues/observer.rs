//! Micro-control observer — all-seeing eye

use super::telemetry::Telemetry;
use crate::traits::{ParameterId, PortId};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Component statistics
#[derive(Debug, Clone, Default)]
pub struct ComponentStats {
    /// Number of operations
    pub operations: u64,
    /// Total time (for average)
    pub total_time_ns: u64,
    /// Maximum time
    pub max_time_ns: u64,
    /// Number of violations
    pub violations: u64,
    /// Average time
    pub avg_time_ns: f64,
}

/// Violation record
#[derive(Debug, Clone)]
pub struct Violation {
    /// Component that violated the law
    pub component: String,
    /// Expected time (ns)
    pub expected_ns: u64,
    /// Actual time (ns)
    pub actual_ns: u64,
    /// Violation timestamp
    pub timestamp: u64,
    /// Applied value (if any)
    pub value: Option<f32>,
}

/// Sandbox summary
#[derive(Debug, Default, Clone)]
pub struct SandboxSummary {
    /// Total micro-control operations
    pub total_operations: u64,
    /// Total violations
    pub total_violations: u64,
    /// Number of active components
    pub components: Vec<String>,
    /// Maximum operation time
    pub max_time_ns: u64,
    /// Component with maximum time
    pub max_time_component: Option<String>,
    /// Number of recorded violations
    pub violations_count: usize,
}

/// Micro-control permit
#[derive(Debug, Clone)]
pub struct MicroControlPermit {
    /// Flag indicating direct control is allowed
    enabled: Arc<std::sync::atomic::AtomicBool>,
    /// Maximum processing time (in nanoseconds)
    max_time_ns: u64,
    /// Component name (for debugging)
    component: String,
}

impl MicroControlPermit {
    /// Create a new permit
    pub fn new(component: impl Into<String>, max_time_ns: u64) -> Self {
        Self {
            enabled: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            max_time_ns,
            component: component.into(),
        }
    }

    /// Check if micro-control is allowed
    pub fn is_allowed(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Revoke micro-control
    pub fn revoke(&self) {
        self.enabled
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// Get the maximum processing time
    pub fn max_time_ns(&self) -> u64 {
        self.max_time_ns
    }

    /// Get the component name
    pub fn component(&self) -> &str {
        &self.component
    }
}

/// Micro-control observer
#[derive(Clone)]
pub struct MicroControlObserver {
    /// Statistics by component
    stats: Arc<RwLock<HashMap<String, ComponentStats>>>,

    /// Violation history
    violations: Arc<RwLock<Vec<Violation>>>,

    /// Telemetry sender
    telemetry_tx: crossbeam_channel::Sender<Telemetry>,
}

impl MicroControlObserver {
    /// Create a new observer with a telemetry queue
    pub fn new(telemetry: super::telemetry::TelemetryQueue) -> Self {
        Self {
            stats: Arc::new(RwLock::new(HashMap::new())),
            violations: Arc::new(RwLock::new(Vec::new())),
            telemetry_tx: telemetry.sender(),
        }
    }

    /// Create a new observer with a telemetry sender
    pub fn with_sender(telemetry_tx: crossbeam_channel::Sender<Telemetry>) -> Self {
        Self {
            stats: Arc::new(RwLock::new(HashMap::new())),
            violations: Arc::new(RwLock::new(Vec::new())),
            telemetry_tx,
        }
    }

    /// Observe the start of an operation
    pub fn observe_start(&self, component: &str) -> OperationGuard {
        OperationGuard {
            component: component.to_string(),
            start_time: Self::now(),
            observer: self.clone(),
        }
    }

    /// Observe the start of an operation with parameters
    pub fn observe_start_with_params(
        &self,
        component: &str,
        port: PortId,
        _parameter: &ParameterId,
    ) -> OperationGuard {
        let guard = self.observe_start(component);

        // Send telemetry about operation start
        let _ = self.telemetry_tx.send(Telemetry::event(
            "observer",
            "micro_start",
            vec![port.node_id().inner() as f32, port.index() as f32],
        ));

        guard
    }

    /// Record a violation
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

        // Save to history
        self.violations.write().push(violation.clone());

        // Update statistics
        let mut stats = self.stats.write();
        let comp_stats = stats.entry(component.to_string()).or_default();
        comp_stats.violations += 1;

        // Send telemetry directly via Sender
        let _ = self.telemetry_tx.send(Telemetry::violation(
            component,
            expected_ns,
            actual_ns,
            value,
        ));

        // Temporarily using println instead of log
        println!(
            "⚠️ Violation in {}: {}ns (expected {}ns)",
            component, actual_ns, expected_ns
        );
    }

    /// Get statistics for a component
    pub fn component_stats(&self, component: &str) -> Option<ComponentStats> {
        self.stats.read().get(component).cloned()
    }

    /// Get all violations
    pub fn violations(&self) -> Vec<Violation> {
        self.violations.read().clone()
    }

    /// Get a sandbox summary
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

    /// Get the current time in microseconds
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
        let duration = (Self::now() - self.start_time) * 1000; // microseconds -> nanoseconds

        // Update statistics
        let mut stats = self.observer.stats.write();
        let comp_stats = stats.entry(self.component.clone()).or_default();
        comp_stats.operations += 1;
        comp_stats.total_time_ns += duration;
        if duration > comp_stats.max_time_ns {
            comp_stats.max_time_ns = duration;
        }
        comp_stats.avg_time_ns = comp_stats.total_time_ns as f64 / comp_stats.operations as f64;

        // Send telemetry via Sender
        let _ = self.observer.telemetry_tx.send(Telemetry::event(
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
    #[test]
    fn test_observer_creation() {
        let (tx, _rx) = crossbeam_channel::unbounded();
        let observer = MicroControlObserver::with_sender(tx);

        let stats = observer.sandbox_summary();
        assert_eq!(stats.total_operations, 0);
        assert_eq!(stats.total_violations, 0);
    }

    #[test]
    fn test_observer_record_violation() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let observer = MicroControlObserver::with_sender(tx);

        observer.record_violation("test_comp", 100, 250, Some(0.5));

        let stats = observer.sandbox_summary();
        assert_eq!(stats.total_violations, 1);

        let violations = observer.violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].component, "test_comp");
        assert_eq!(violations[0].expected_ns, 100);
        assert_eq!(violations[0].actual_ns, 250);
        assert_eq!(violations[0].value, Some(0.5));

        // Verify that telemetry was sent
        let telemetry = rx.try_recv().unwrap();
        match telemetry {
            Telemetry::Violation {
                component,
                expected_ns,
                actual_ns,
                value,
                ..
            } => {
                assert_eq!(component, "test_comp");
                assert_eq!(expected_ns, 100);
                assert_eq!(actual_ns, 250);
                assert_eq!(value, Some(0.5));
            }
            _ => panic!("Expected violation telemetry"),
        }
    }

    #[test]
    fn test_observer_operation_guard() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let observer = MicroControlObserver::with_sender(tx);

        {
            let _guard = observer.observe_start("test_op");
            std::thread::sleep(std::time::Duration::from_micros(10));
        } // guard automatically records completion on drop

        let stats = observer.sandbox_summary();
        assert_eq!(stats.total_operations, 1);
        assert!(stats.max_time_ns > 0);

        // Verify completion telemetry
        let telemetry = rx.try_recv().unwrap();
        match telemetry {
            Telemetry::Event { kind, .. } => {
                assert_eq!(kind, "micro_complete");
            }
            _ => panic!("Expected event telemetry"),
        }
    }

    #[test]
    fn test_observer_component_stats() {
        let (tx, _rx) = crossbeam_channel::unbounded();
        let observer = MicroControlObserver::with_sender(tx);

        for i in 0..5 {
            let _guard = observer.observe_start("comp1");
            std::thread::sleep(std::time::Duration::from_micros(i * 10));
        }

        for i in 0..3 {
            let _guard = observer.observe_start("comp2");
            std::thread::sleep(std::time::Duration::from_micros(i * 20));
        }

        let stats = observer.sandbox_summary();
        assert_eq!(stats.total_operations, 8);
        assert_eq!(stats.components.len(), 2);

        let comp1_stats = observer.component_stats("comp1").unwrap();
        assert_eq!(comp1_stats.operations, 5);
        assert!(comp1_stats.avg_time_ns > 0.0);

        let comp2_stats = observer.component_stats("comp2").unwrap();
        assert_eq!(comp2_stats.operations, 3);
    }
}
