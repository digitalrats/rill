//! Наблюдатель за микро-контролем — всевидящее око

use super::telemetry::Telemetry;
use crate::traits::{ParameterId, PortId};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use parking_lot::RwLock;

/// Статистика компонента
#[derive(Debug, Clone, Default)]
pub struct ComponentStats {
    /// Количество операций
    pub operations: u64,
    /// Суммарное время (для среднего)
    pub total_time_ns: u64,
    /// Максимальное время
    pub max_time_ns: u64,
    /// Количество нарушений
    pub violations: u64,
    /// Среднее время
    pub avg_time_ns: f64,
}

/// Запись о нарушении
#[derive(Debug, Clone)]
pub struct Violation {
    /// Компонент, нарушивший закон
    pub component: String,
    /// Ожидаемое время (нс)
    pub expected_ns: u64,
    /// Фактическое время (нс)
    pub actual_ns: u64,
    /// Время нарушения
    pub timestamp: u64,
    /// Примененное значение (если было)
    pub value: Option<f32>,
}

/// Сводка по песочнице
#[derive(Debug, Default, Clone)]
pub struct SandboxSummary {
    /// Всего операций микро-контроля
    pub total_operations: u64,
    /// Всего нарушений
    pub total_violations: u64,
    /// Количество активных компонентов
    pub components: Vec<String>,
    /// Максимальное время операции
    pub max_time_ns: u64,
    /// Компонент с максимальным временем
    pub max_time_component: Option<String>,
    /// Количество записанных нарушений
    pub violations_count: usize,
}

/// Разрешение на микро-контроль
#[derive(Debug, Clone)]
pub struct MicroControlPermit {
    /// Флаг, что разрешено прямое управление
    enabled: Arc<std::sync::atomic::AtomicBool>,
    /// Максимальное время обработки (в наносекундах)
    max_time_ns: u64,
    /// Имя компонента (для отладки)
    component: String,
}

impl MicroControlPermit {
    /// Создать новое разрешение
    pub fn new(component: impl Into<String>, max_time_ns: u64) -> Self {
        Self {
            enabled: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            max_time_ns,
            component: component.into(),
        }
    }
    
    /// Проверить, можно ли использовать микро-контроль
    pub fn is_allowed(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Relaxed)
    }
    
    /// Запретить микро-контроль
    pub fn revoke(&self) {
        self.enabled.store(false, std::sync::atomic::Ordering::Relaxed);
    }
    
    /// Получить максимальное время обработки
    pub fn max_time_ns(&self) -> u64 {
        self.max_time_ns
    }
    
    /// Получить имя компонента
    pub fn component(&self) -> &str {
        &self.component
    }
}

/// Наблюдатель за микро-контролем
#[derive(Clone)]
pub struct MicroControlObserver {
    /// Статистика по компонентам
    stats: Arc<RwLock<HashMap<String, ComponentStats>>>,
    
    /// История нарушений
    violations: Arc<RwLock<Vec<Violation>>>,
    
    /// Отправитель телеметрии
    telemetry_tx: crossbeam_channel::Sender<Telemetry>,
}

impl MicroControlObserver {
    /// Создать нового наблюдателя с очередью телеметрии
    pub fn new(telemetry: super::telemetry::TelemetryQueue) -> Self {
        Self {
            stats: Arc::new(RwLock::new(HashMap::new())),
            violations: Arc::new(RwLock::new(Vec::new())),
            telemetry_tx: telemetry.sender(),
        }
    }
    
    /// Создать нового наблюдателя с отправителем телеметрии
    pub fn with_sender(telemetry_tx: crossbeam_channel::Sender<Telemetry>) -> Self {
        Self {
            stats: Arc::new(RwLock::new(HashMap::new())),
            violations: Arc::new(RwLock::new(Vec::new())),
            telemetry_tx,
        }
    }
    
    /// Наблюдать за началом операции
    pub fn observe_start(&self, component: &str) -> OperationGuard {
        OperationGuard {
            component: component.to_string(),
            start_time: Self::now(),
            observer: self.clone(),
        }
    }
    
    /// Наблюдать за началом операции с параметрами
    pub fn observe_start_with_params(
        &self,
        component: &str,
        port: PortId,
        _parameter: &ParameterId,
    ) -> OperationGuard {
        let guard = self.observe_start(component);
        
        // Отправляем телеметрию о начале операции
        let _ = self.telemetry_tx.send(Telemetry::event(
            "observer",
            "micro_start",
            vec![port.node_id().inner() as f32, port.index() as f32],
        ));
        
        guard
    }
    
    /// Зафиксировать нарушение
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
        
        // Сохраняем в историю
        self.violations.write().push(violation.clone());
        
        // Обновляем статистику
        let mut stats = self.stats.write();
        let comp_stats = stats.entry(component.to_string()).or_default();
        comp_stats.violations += 1;
        
        // Отправляем телеметрию напрямую через Sender
        let _ = self.telemetry_tx.send(Telemetry::violation(
            component,
            expected_ns,
            actual_ns,
            value,
        ));
        
        // Временно используем println вместо log
        println!(
            "⚠️ Нарушение в {}: {}нс (ожидалось {}нс)",
            component, actual_ns, expected_ns
        );
    }
    
    /// Получить статистику по компоненту
    pub fn component_stats(&self, component: &str) -> Option<ComponentStats> {
        self.stats.read().get(component).cloned()
    }
    
    /// Получить все нарушения
    pub fn violations(&self) -> Vec<Violation> {
        self.violations.read().clone()
    }
    
    /// Получить сводку по песочнице
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
    
    /// Получить текущее время в микросекундах
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64
    }
}

/// Guard, который автоматически фиксирует завершение операции
pub struct OperationGuard {
    component: String,
    start_time: u64,
    observer: MicroControlObserver,
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        let duration = (Self::now() - self.start_time) * 1000; // микросекунды -> наносекунды
        
        // Обновляем статистику
        let mut stats = self.observer.stats.write();
        let comp_stats = stats.entry(self.component.clone()).or_default();
        comp_stats.operations += 1;
        comp_stats.total_time_ns += duration;
        if duration > comp_stats.max_time_ns {
            comp_stats.max_time_ns = duration;
        }
        comp_stats.avg_time_ns = comp_stats.total_time_ns as f64 / comp_stats.operations as f64;
        
        // Отправляем телеметрию через Sender
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
    use crate::queues::TelemetryQueue;

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
        
        // Проверяем, что телеметрия была отправлена
        let telemetry = rx.try_recv().unwrap();
        match telemetry {
            Telemetry::Violation { component, expected_ns, actual_ns, value, .. } => {
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
        } // guard автоматически фиксирует завершение при drop
        
        let stats = observer.sandbox_summary();
        assert_eq!(stats.total_operations, 1);
        assert!(stats.max_time_ns > 0);
        
        // Проверяем телеметрию завершения
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