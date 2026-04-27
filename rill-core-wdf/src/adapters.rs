use crate::WdfElement;
use parking_lot::RwLock;
use rill_core::AudioNum;
use std::sync::Arc;

/// Series adapter — connects WDF elements in series
///
/// Total port resistance is the sum of all element port resistances.
/// Current is equal through all elements, voltage sums.
#[derive(Clone)]
pub struct SeriesAdapter<T: AudioNum> {
    elements: Vec<Arc<RwLock<dyn WdfElement<T>>>>,
    port_resistance: T,
}

impl<T: AudioNum> std::fmt::Debug for SeriesAdapter<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeriesAdapter")
            .field("num_elements", &self.elements.len())
            .field("port_resistance", &self.port_resistance)
            .finish()
    }
}

impl<T: AudioNum> SeriesAdapter<T> {
    /// Create a new series adapter from WDF elements
    pub fn new(elements: Vec<Arc<RwLock<dyn WdfElement<T>>>>) -> Self {
        let port_resistance: T = elements
            .iter()
            .map(|e| e.read().port_resistance())
            .fold(T::ZERO, |a, b| a + b);

        Self {
            elements,
            port_resistance,
        }
    }

    /// Get a reference to the inner elements
    pub fn elements(&self) -> &[Arc<RwLock<dyn WdfElement<T>>>] {
        &self.elements
    }
}

impl<T: AudioNum> WdfElement<T> for SeriesAdapter<T> {
    fn port_resistance(&self) -> T {
        self.port_resistance
    }

    fn process_incident(&mut self, a: T) -> T {
        let total_r = self.port_resistance;
        let mut b_total = T::ZERO;

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

    fn voltage(&self) -> T {
        self.elements
            .iter()
            .map(|e| e.read().voltage())
            .fold(T::ZERO, |a, b| a + b)
    }

    fn current(&self) -> T {
        self.elements
            .first()
            .map(|e| e.read().current())
            .unwrap_or(T::ZERO)
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
pub struct ParallelAdapter<T: AudioNum> {
    elements: Vec<Arc<RwLock<dyn WdfElement<T>>>>,
    port_resistance: T,
}

impl<T: AudioNum> std::fmt::Debug for ParallelAdapter<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParallelAdapter")
            .field("num_elements", &self.elements.len())
            .field("port_resistance", &self.port_resistance)
            .finish()
    }
}

impl<T: AudioNum> ParallelAdapter<T> {
    /// Create a new parallel adapter from WDF elements
    pub fn new(elements: Vec<Arc<RwLock<dyn WdfElement<T>>>>) -> Self {
        let inv_port_resistance: T = elements
            .iter()
            .map(|e| T::ONE / e.read().port_resistance())
            .fold(T::ZERO, |a, b| a + b);

        let port_resistance = T::ONE / inv_port_resistance;

        Self {
            elements,
            port_resistance,
        }
    }

    /// Get a reference to the inner elements
    pub fn elements(&self) -> &[Arc<RwLock<dyn WdfElement<T>>>] {
        &self.elements
    }
}

impl<T: AudioNum> WdfElement<T> for ParallelAdapter<T> {
    fn port_resistance(&self) -> T {
        self.port_resistance
    }

    fn process_incident(&mut self, a: T) -> T {
        let total_g: T = self
            .elements
            .iter()
            .map(|e| T::ONE / e.read().port_resistance())
            .fold(T::ZERO, |a, b| a + b);

        let two = T::from_f32(2.0);
        let alpha: Vec<T> = self
            .elements
            .iter()
            .map(|e| {
                let g_i = T::ONE / e.read().port_resistance();
                two * g_i / total_g
            })
            .collect();

        let mut sum_alpha_b = T::ZERO;
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

    fn voltage(&self) -> T {
        self.elements
            .first()
            .map(|e| e.read().voltage())
            .unwrap_or(T::ZERO)
    }

    fn current(&self) -> T {
        self.elements
            .iter()
            .map(|e| e.read().current())
            .fold(T::ZERO, |a, b| a + b)
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
    use crate::elements::{Capacitor, Resistor};

    #[test]
    fn test_series_adapter() {
        let sample_rate = 44100.0;

        let resistor: Arc<RwLock<dyn WdfElement<f64>>> =
            Arc::new(RwLock::new(Resistor::new(1000.0)));
        let capacitor: Arc<RwLock<dyn WdfElement<f64>>> =
            Arc::new(RwLock::new(Capacitor::new(1e-6, sample_rate)));

        let elements = vec![resistor.clone(), capacitor.clone()];
        let adapter: SeriesAdapter<f64> = SeriesAdapter::new(elements);

        let total_r = adapter.port_resistance();
        let r1 = resistor.read().port_resistance();
        let r2 = capacitor.read().port_resistance();

        assert!((total_r - (r1 + r2)).abs() < 1e-10);
    }
}
