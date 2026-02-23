//! # Карта параметров
//!
//! Простое хранилище для значений параметров, доступное по имени.
//! Используется внутри [`AutomationContext`](crate::AutomationContext)
//! для того, чтобы автоматы могли читать значения других параметров.
//!
//! ## Почему не использовать `ParamValue` напрямую?
//!
//! `ParamValue` из `kama-core-traits` слишком тяжёлый для внутреннего использования
//! в автоматизации — он содержит много метаданных. Здесь мы храним только `f64`,
//! так как все автоматизируемые параметры в конечном счёте приводятся к числам.

//! Карта параметров с поддержкой kama-core ParamValue

use parking_lot::RwLock;
use std::collections::HashMap;

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
    /// Создать новую пустую карту.
    pub fn new() -> Self {
        Self {
            params: RwLock::new(HashMap::new()),
        }
    }

    /// Установить значение параметра (создаёт запись, если не существует).
    pub fn set_parameter(&self, name: &str, value: f64) {
        let mut params = self.params.write();
        if let Some(data) = params.get_mut(name) {
            data.value = value;
        } else {
            params.insert(
                name.to_string(),
                ParameterData {
                    value,
                    min: None,
                    max: None,
                    step: None,
                    unit: None,
                },
            );
        }
    }

    /// Получить значение параметра по имени. Возвращает `None`, если параметр не найден.
    pub fn get_parameter(&self, name: &str) -> Option<f64> {
        let params = self.params.read();
        params.get(name).map(|data| data.value)
    }

    /// Получить полные данные параметра (включая метаданные).
    pub fn get_parameter_data(&self, name: &str) -> Option<ParameterData> {
        let params = self.params.read();
        params.get(name).cloned()
    }

    /// Удалить параметр. Возвращает `true`, если параметр существовал.
    pub fn remove_parameter(&self, name: &str) -> bool {
        let mut params = self.params.write();
        params.remove(name).is_some()
    }

    /// Очистить карту.
    pub fn clear(&self) {
        let mut params = self.params.write();
        params.clear();
    }

    /// Получить итератор по всем параметрам в виде `(имя, значение)`.
    pub fn iter(&self) -> Vec<(String, f64)> {
        let params = self.params.read();
        params.iter().map(|(k, v)| (k.clone(), v.value)).collect()
    }
}
