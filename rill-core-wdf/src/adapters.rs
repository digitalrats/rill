use std::sync::Arc;
use parking_lot::RwLock;
use crate::WdfElement;

/// Series adapter — connects WDF elements in series
///
/// Total port resistance is the sum of all element port resistances.
/// Current is equal through all elements, voltage sums.
#[derive(Clone)]
pub struct SeriesAdapter {
    elements: Vec<Arc<RwLock<dyn WdfElement>>>,
    port_resistance: f64,
}

impl std::fmt::Debug for SeriesAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeriesAdapter")
            .field("num_elements", &self.elements.len())
            .field("port_resistance", &self.port_resistance)
            .finish()
    }
}

impl SeriesAdapter {
    /// Create a new series adapter from WDF elements
    pub fn new(elements: Vec<Arc<RwLock<dyn WdfElement>>>) -> Self {
        let port_resistance: f64 = elements.iter()
            .map(|e| e.read().port_resistance())
            .sum();

        Self {
            elements,
            port_resistance,
        }
    }

    /// Get a reference to the inner elements
    pub fn elements(&self) -> &[Arc<RwLock<dyn WdfElement>>] {
        &self.elements
    }
}

impl WdfElement for SeriesAdapter {
    fn port_resistance(&self) -> f64 {
        self.port_resistance
    }

    fn process_incident(&mut self, a: f64) -> f64 {
        let total_r = self.port_resistance;
        let mut b_total = 0.0;

        for element in &self.elements {
            let r_i = element.read().port_resistance();
            let a_i = a * (r_i / total_r);

            let b_i = element.write().process_incident(a_i);
            b_total += b_i * (r_i / total_r);
        }

        b_total
    }

    fn update_state(&mut self) {
        for element in &self.elements {
            element.write().update_state();
        }
    }

    fn voltage(&self) -> f64 {
        self.elements.iter()
            .map(|e| e.read().voltage())
            .sum()
    }

    fn current(&self) -> f64 {
        self.elements.first()
            .map(|e| e.read().current())
            .unwrap_or(0.0)
    }

    fn reset(&mut self) {
        for element in &self.elements {
            element.write().reset();
        }
    }
}

/// Parallel adapter — connects WDF elements in parallel
///
/// Total conductance is the sum of all element conductances.
/// Voltage is equal across all elements, current sums.
#[derive(Clone)]
pub struct ParallelAdapter {
    elements: Vec<Arc<RwLock<dyn WdfElement>>>,
    port_resistance: f64,
}

impl std::fmt::Debug for ParallelAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParallelAdapter")
            .field("num_elements", &self.elements.len())
            .field("port_resistance", &self.port_resistance)
            .finish()
    }
}

impl ParallelAdapter {
    /// Create a new parallel adapter from WDF elements
    pub fn new(elements: Vec<Arc<RwLock<dyn WdfElement>>>) -> Self {
        let inv_port_resistance: f64 = elements.iter()
            .map(|e| 1.0 / e.read().port_resistance())
            .sum();

        let port_resistance = 1.0 / inv_port_resistance;

        Self {
            elements,
            port_resistance,
        }
    }

    /// Get a reference to the inner elements
    pub fn elements(&self) -> &[Arc<RwLock<dyn WdfElement>>] {
        &self.elements
    }
}

impl WdfElement for ParallelAdapter {
    fn port_resistance(&self) -> f64 {
        self.port_resistance
    }

    fn process_incident(&mut self, a: f64) -> f64 {
        let total_g: f64 = self.elements.iter()
            .map(|e| 1.0 / e.read().port_resistance())
            .sum();

        let alpha: Vec<f64> = self.elements.iter()
            .map(|e| {
                let g_i = 1.0 / e.read().port_resistance();
                2.0 * g_i / total_g
            })
            .collect();

        let mut sum_alpha_b = 0.0;
        for (i, element) in self.elements.iter().enumerate() {
            let b_i = element.write().process_incident(a);
            sum_alpha_b += alpha[i] * b_i;
        }

        sum_alpha_b - a
    }

    fn update_state(&mut self) {
        for element in &self.elements {
            element.write().update_state();
        }
    }

    fn voltage(&self) -> f64 {
        self.elements.first()
            .map(|e| e.read().voltage())
            .unwrap_or(0.0)
    }

    fn current(&self) -> f64 {
        self.elements.iter()
            .map(|e| e.read().current())
            .sum()
    }

    fn reset(&mut self) {
        for element in &self.elements {
            element.write().reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{Resistor, Capacitor};

    #[test]
    fn test_series_adapter() {
        let sample_rate = 44100.0;

        let resistor: Arc<RwLock<dyn WdfElement>> = Arc::new(RwLock::new(Resistor::new(1000.0)));
        let capacitor: Arc<RwLock<dyn WdfElement>> = Arc::new(RwLock::new(Capacitor::new(1e-6, sample_rate)));

        let elements = vec![resistor.clone(), capacitor.clone()];
        let adapter = SeriesAdapter::new(elements);

        let total_r = adapter.port_resistance();
        let r1 = resistor.read().port_resistance();
        let r2 = capacitor.read().port_resistance();

        assert!((total_r - (r1 + r2)).abs() < 1e-10);
    }
}
