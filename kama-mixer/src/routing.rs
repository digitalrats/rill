use serde::{Serialize, Deserialize};

/// Режим микшера
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum MixerMode {
    Normal,        // Нормальный режим
    Parallel,      // Параллельная обработка
    Serial,        // Последовательная обработка
    Sidechain,     // Sidechain-компрессия
}

/// Матрица маршрутизации
#[derive(Debug, Clone)]
pub struct RoutingMatrix {
    matrix: Vec<Vec<f64>>, // [from_channel][to_channel]
}

impl RoutingMatrix {
    pub fn new(num_channels: usize) -> Self {
        Self {
            matrix: vec![vec![0.0; num_channels]; num_channels],
        }
    }
    
    pub fn set_route(&mut self, from: usize, to: usize, amount: f64) {
        if from < self.matrix.len() && to < self.matrix[from].len() {
            self.matrix[from][to] = amount.clamp(0.0, 1.0);
        }
    }
    
    pub fn get_route(&self, from: usize, to: usize) -> f64 {
        if from < self.matrix.len() && to < self.matrix[from].len() {
            self.matrix[from][to]
        } else {
            0.0
        }
    }
}