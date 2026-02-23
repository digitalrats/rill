//! Центральный реестр узлов для экосистемы Kama Audio
//!
//! Позволяет:
//! - Регистрировать типы узлов из разных крейтов
//! - Создавать узлы по имени
//! - Сериализовать/десериализовать графы
//! - Получать метаданные о доступных узлах

#![warn(missing_docs)]

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{RegistryError, RegistryResult}; // <-- ИСПРАВЛЕНО: импорт из crate::error
use crate::node::{
    NodeFactoryFn,
    // <-- ИСПРАВЛЕНО: импорт из crate::node
    NodeTypeInfo,
};

#[cfg(feature = "serde")]
pub use crate::node::{
    SerializableConnection,
    SerializableGraph,
    // <-- ИСПРАВЛЕНО: импорт из crate::node
    SerializableNode,
    SerializableParameter,
};

use kama_core_traits::{AudioNode, NodeMetadata, NodeTypeId};

lazy_static::lazy_static! {
    /// Глобальный реестр узлов
    static ref NODE_REGISTRY: Arc<RwLock<NodeRegistry>> = Arc::new(RwLock::new(NodeRegistry::new()));
}

/// Реестр типов узлов
#[derive(Default)]
pub struct NodeRegistry {
    by_name: HashMap<String, NodeTypeInfo>,
    by_id: HashMap<NodeTypeId, String>,
}

impl NodeRegistry {
    /// Создать новый реестр
    pub fn new() -> Self {
        Self {
            by_name: HashMap::new(),
            by_id: HashMap::new(),
        }
    }

    /// Зарегистрировать тип узла
    pub fn register(
        &mut self,
        type_name: &str,
        metadata: NodeMetadata,
        factory: NodeFactoryFn,
    ) -> RegistryResult<()> {
        let type_name = type_name.to_string();

        if self.by_name.contains_key(&type_name) {
            return Err(RegistryError::NodeTypeAlreadyRegistered(type_name));
        }

        // Создаём временный узел для получения type_id
        let node = factory();
        let type_id = node.node_type_id();

        let info = NodeTypeInfo {
            type_name: type_name.clone(),
            metadata,
            type_id,
            factory,
        };

        self.by_name.insert(type_name.clone(), info);
        self.by_id.insert(type_id, type_name);

        Ok(())
    }

    /// Получить информацию о типе узла по имени
    pub fn get_by_name(&self, type_name: &str) -> Option<&NodeTypeInfo> {
        self.by_name.get(type_name)
    }

    /// Получить имя типа по ID
    pub fn get_name_by_id(&self, type_id: NodeTypeId) -> Option<&String> {
        self.by_id.get(&type_id)
    }

    /// Создать узел по имени
    pub fn create_node(&self, type_name: &str) -> RegistryResult<Box<dyn AudioNode>> {
        self.by_name
            .get(type_name)
            .map(|info| (info.factory)())
            .ok_or_else(|| RegistryError::NodeTypeNotFound(type_name.to_string()))
    }

    /// Получить список всех зарегистрированных типов
    pub fn list_types(&self) -> Vec<&NodeTypeInfo> {
        self.by_name.values().collect()
    }

    /// Проверить, зарегистрирован ли тип
    pub fn contains(&self, type_name: &str) -> bool {
        self.by_name.contains_key(type_name)
    }

    /// Получить количество зарегистрированных типов
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// Очистить реестр (для тестов)
    pub fn clear(&mut self) {
        self.by_name.clear();
        self.by_id.clear();
    }
}

/// Доступ к глобальному реестру
pub struct Registry;

impl Registry {
    /// Получить доступ к глобальному реестру
    pub fn global() -> Arc<RwLock<NodeRegistry>> {
        NODE_REGISTRY.clone()
    }

    /// Зарегистрировать тип узла в глобальном реестре
    pub fn register(
        type_name: &str,
        metadata: NodeMetadata,
        factory: NodeFactoryFn,
    ) -> RegistryResult<()> {
        let mut registry = NODE_REGISTRY.write();
        registry.register(type_name, metadata, factory)
    }

    /// Создать узел по имени
    pub fn create(type_name: &str) -> RegistryResult<Box<dyn AudioNode>> {
        let registry = NODE_REGISTRY.read();
        registry.create_node(type_name)
    }

    /// Получить метаданные узла по имени
    pub fn metadata(type_name: &str) -> Option<NodeMetadata> {
        let registry = NODE_REGISTRY.read();
        registry
            .get_by_name(type_name)
            .map(|info| info.metadata.clone())
    }

    /// Получить список всех зарегистрированных типов
    pub fn list_types() -> Vec<NodeTypeInfo> {
        let registry = NODE_REGISTRY.read();
        registry.list_types().into_iter().cloned().collect()
    }

    /// Получить имена всех зарегистрированных типов
    pub fn list_type_names() -> Vec<String> {
        let registry = NODE_REGISTRY.read();
        registry
            .list_types()
            .into_iter()
            .map(|info| info.type_name.clone())
            .collect()
    }

    /// Проверить, зарегистрирован ли тип
    pub fn contains(type_name: &str) -> bool {
        let registry = NODE_REGISTRY.read();
        registry.contains(type_name)
    }

    /// Сериализовать граф (требует feature "serde")
    #[cfg(feature = "serde")]
    pub fn serialize_graph(graph: &dyn kama_core_traits::AudioGraph) -> RegistryResult<String> {
        // Здесь должна быть реализация
        Err(RegistryError::Serialization(
            "Graph serialization not yet implemented".into(),
        ))
    }

    /// Десериализовать граф (требует feature "serde")
    #[cfg(feature = "serde")]
    pub fn deserialize_graph(data: &str) -> RegistryResult<Box<dyn kama_core_traits::AudioGraph>> {
        // Здесь должна быть реализация
        Err(RegistryError::Deserialization(
            "Graph deserialization not yet implemented".into(),
        ))
    }

    /// Очистить глобальный реестр (для тестов)
    pub fn clear() {
        let mut registry = NODE_REGISTRY.write();
        registry.clear();
    }
}
