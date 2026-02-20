//! Карта параметров с поддержкой kama-core ParamValue

use std::collections::HashMap;
use parking_lot::RwLock;

/// Данные параметра
#[derive(Debug, Clone)]
pub struct ParameterData {
    pub value: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub unit: Option<String>,
}

/// Карта параметров
#[derive(Debug, Default)]
pub struct ParameterMap {
    params: RwLock<HashMap<String, ParameterData>>,
}

impl ParameterMap {
    pub fn new() -> Self {
        Self {
            params: RwLock::new(HashMap::new()),
        }
    }
    
    pub fn set_parameter(&self, name: &str, value: f64) {
        let mut params = self.params.write();
        if let Some(data) = params.get_mut(name) {
            data.value = value;
        } else {
            params.insert(name.to_string(), ParameterData {
                value,
                min: None,
                max: None,
                step: None,
                unit: None,
            });
        }
    }
    
    pub fn get_parameter(&self, name: &str) -> Option<f64> {
        let params = self.params.read();
        params.get(name).map(|data| data.value)
    }
    
    pub fn get_parameter_data(&self, name: &str) -> Option<ParameterData> {
        let params = self.params.read();
        params.get(name).cloned()
    }
    
    pub fn remove_parameter(&self, name: &str) -> bool {
        let mut params = self.params.write();
        params.remove(name).is_some()
    }
    
    pub fn clear(&self) {
        let mut params = self.params.write();
        params.clear();
    }
    
    pub fn iter(&self) -> Vec<(String, f64)> {
        let params = self.params.read();
        params.iter()
            .map(|(k, v)| (k.clone(), v.value))
            .collect()
    }
}