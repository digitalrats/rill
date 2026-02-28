//! Движок управления - агрегирует события и применяет маппинги

use crate::error::{ControlError, ControlResult};
use crate::mapping::{ControlEvent, Mapping};
use crossbeam_channel::{Receiver, Sender};
use kama_core::signal::ParameterChanged;
use kama_core::traits::{ParameterId, PortId, SignalSource};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Статистика работы движка
#[derive(Debug, Default, Clone, Copy)]
pub struct ControlStats {
    /// Количество обработанных событий
    pub events_processed: u64,
    /// Количество примененных маппингов
    pub mappings_applied: u64,
    /// Количество ошибок
    pub errors: u64,
}

/// Движок управления - основной компонент системы
///
/// Получает события от различных источников (MIDI, OSC, HID),
/// применяет маппинги и отправляет изменения параметров в систему автоматизации.
pub struct ControlEngine {
    /// Маппинги событий на параметры
    mappings: Vec<Mapping>,
    /// Индекс маппингов по паттернам для быстрого поиска
    mapping_index: HashMap<String, Vec<usize>>,
    /// Канал для отправки изменений параметров
    output_tx: Option<Sender<ParameterChanged>>,
    /// Статистика
    stats: Arc<RwLock<ControlStats>>,
}

impl ControlEngine {
    /// Создать новый движок управления
    pub fn new() -> Self {
        Self {
            mappings: Vec::new(),
            mapping_index: HashMap::new(),
            output_tx: None,
            stats: Arc::new(RwLock::new(ControlStats::default())),
        }
    }
    
    /// Установить канал для отправки изменений параметров
    pub fn set_output_channel(&mut self, tx: Sender<ParameterChanged>) {
        self.output_tx = Some(tx);
    }
    
    /// Добавить маппинг
    pub fn add_mapping(&mut self, mapping: Mapping) {
        let idx = self.mappings.len();
        
        // Индексируем по строковому представлению паттерна
        let pattern_key = format!("{:?}", mapping.pattern);
        self.mapping_index
            .entry(pattern_key)
            .or_insert_with(Vec::new)
            .push(idx);
        
        self.mappings.push(mapping);
    }
    
    /// Добавить несколько маппингов
    pub fn add_mappings(&mut self, mappings: Vec<Mapping>) {
        for mapping in mappings {
            self.add_mapping(mapping);
        }
    }
    
    /// Удалить маппинг по индексу
    pub fn remove_mapping(&mut self, index: usize) -> Option<Mapping> {
        if index < self.mappings.len() {
            let mapping = self.mappings.remove(index);
            
            // Перестроить индекс (проще пересоздать)
            self.rebuild_index();
            
            Some(mapping)
        } else {
            None
        }
    }
    
    /// Очистить все маппинги
    pub fn clear_mappings(&mut self) {
        self.mappings.clear();
        self.mapping_index.clear();
    }
    
    /// Перестроить индекс маппингов
    fn rebuild_index(&mut self) {
        self.mapping_index.clear();
        for (idx, mapping) in self.mappings.iter().enumerate() {
            let pattern_key = format!("{:?}", mapping.pattern);
            self.mapping_index
                .entry(pattern_key)
                .or_insert_with(Vec::new)
                .push(idx);
        }
    }
    
    /// Включить/выключить маппинг
    pub fn set_mapping_enabled(&mut self, index: usize, enabled: bool) -> ControlResult<()> {
        if let Some(mapping) = self.mappings.get_mut(index) {
            mapping.enabled = enabled;
            Ok(())
        } else {
            Err(ControlError::Mapping(format!("Mapping {} not found", index)))
        }
    }
    
    /// Обработать одно событие
    pub fn process_event(&mut self, event: ControlEvent) {
        let mut stats = self.stats.write();
        stats.events_processed += 1;
        drop(stats);
        
        // Найти все подходящие маппинги
        for mapping in &self.mappings {
            if !mapping.enabled {
                continue;
            }
            
            if let Some(value) = mapping.apply(&event) {
                // Отправить изменение параметра
                if let Some(tx) = &self.output_tx {
                    let signal = ParameterChanged::new(
                        mapping.target.port,
                        mapping.target.parameter.clone(),
                        value,
                        mapping.transform.apply(event.normalized_value().unwrap_or(0.0),
                                               mapping.target.min,
                                               mapping.target.max),
                        SignalSource::External, // Можно уточнить источник
                    );
                    
                    if tx.send(signal).is_ok() {
                        let mut stats = self.stats.write();
                        stats.mappings_applied += 1;
                    } else {
                        let mut stats = self.stats.write();
                        stats.errors += 1;
                    }
                }
            }
        }
    }
    
    /// Обработать несколько событий
    pub fn process_events(&mut self, events: Vec<ControlEvent>) {
        for event in events {
            self.process_event(event);
        }
    }
    
    /// Получить текущую статистику
    pub fn stats(&self) -> ControlStats {
        *self.stats.read()
    }
    
    /// Сбросить статистику
    pub fn reset_stats(&self) {
        let mut stats = self.stats.write();
        *stats = ControlStats::default();
    }
    
    /// Получить список всех маппингов
    pub fn mappings(&self) -> &[Mapping] {
        &self.mappings
    }
    
    /// Найти маппинги по паттерну
    pub fn find_mappings(&self, pattern: &EventPattern) -> Vec<&Mapping> {
        let pattern_key = format!("{:?}", pattern);
        
        if let Some(indices) = self.mapping_index.get(&pattern_key) {
            indices
                .iter()
                .filter_map(|&idx| self.mappings.get(idx))
                .collect()
        } else {
            Vec::new()
        }
    }
}

impl Default for ControlEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use kama_core::traits::{NodeId, ParameterId};
    
    fn test_param(name: &str) -> ParameterId {
        ParameterId::new(name).unwrap()
    }
    
    #[test]
    fn test_engine_basic() {
        let mut engine = ControlEngine::new();
        let (tx, rx) = unbounded();
        engine.set_output_channel(tx);
        
        let node = NodeId(1);
        let port = PortId::control_in(node, 0);
        let param = test_param("gain");
        let target = Target::new(port, param, 0.0, 1.0);
        
        let mapping = Mapping::new(
            EventPattern::Knob(1),
            target,
            Transform::Linear,
        );
        
        engine.add_mapping(mapping);
        
        let event = ControlEvent::Knob { id: 1, value: 0.5 };
        engine.process_event(event);
        
        let received = rx.try_recv().unwrap();
        assert_eq!(received.value, 0.5);
    }
    
    #[test]
    fn test_engine_multiple_mappings() {
        let mut engine = ControlEngine::new();
        let (tx, rx) = unbounded();
        engine.set_output_channel(tx);
        
        let node = NodeId(1);
        let port1 = PortId::control_in(node, 0);
        let port2 = PortId::control_in(node, 1);
        
        let mapping1 = Mapping::new(
            EventPattern::Knob(1),
            Target::new(port1, test_param("gain"), 0.0, 1.0),
            Transform::Linear,
        );
        
        let mapping2 = Mapping::new(
            EventPattern::Knob(1), // Тот же паттерн
            Target::new(port2, test_param("pan"), -1.0, 1.0),
            Transform::Linear,
        );
        
        engine.add_mappings(vec![mapping1, mapping2]);
        
        let event = ControlEvent::Knob { id: 1, value: 0.5 };
        engine.process_event(event);
        
        // Должны получить два сигнала
        let mut count = 0;
        while let Ok(_) = rx.try_recv() {
            count += 1;
        }
        
        assert_eq!(count, 2);
    }
    
    #[test]
    fn test_engine_disable_mapping() {
        let mut engine = ControlEngine::new();
        let (tx, rx) = unbounded();
        engine.set_output_channel(tx);
        
        let node = NodeId(1);
        let port = PortId::control_in(node, 0);
        let param = test_param("gain");
        let target = Target::new(port, param, 0.0, 1.0);
        
        let mapping = Mapping::new(
            EventPattern::Knob(1),
            target,
            Transform::Linear,
        );
        
        engine.add_mapping(mapping);
        
        // Отключаем маппинг
        engine.set_mapping_enabled(0, false).unwrap();
        
        let event = ControlEvent::Knob { id: 1, value: 0.5 };
        engine.process_event(event);
        
        // Не должно быть сигналов
        assert!(rx.try_recv().is_err());
    }
    
    #[test]
    fn test_engine_stats() {
        let mut engine = ControlEngine::new();
        let (tx, _) = unbounded();
        engine.set_output_channel(tx);
        
        let node = NodeId(1);
        let port = PortId::control_in(node, 0);
        let mapping = Mapping::new(
            EventPattern::Knob(1),
            Target::new(port, test_param("gain"), 0.0, 1.0),
            Transform::Linear,
        );
        
        engine.add_mapping(mapping);
        
        let event = ControlEvent::Knob { id: 1, value: 0.5 };
        engine.process_event(event);
        
        let stats = engine.stats();
        assert_eq!(stats.events_processed, 1);
        assert_eq!(stats.mappings_applied, 1);
        assert_eq!(stats.errors, 0);
    }
}