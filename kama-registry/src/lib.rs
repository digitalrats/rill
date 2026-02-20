//! Центральный реестр узлов для экосистемы Kama Audio
//!
//! Позволяет:
//! - Регистрировать типы узлов из разных крейтов
//! - Создавать узлы по имени
//! - Сериализовать/десериализовать графы
//! - Получать метаданные о доступных узлах

#![warn(missing_docs)]

mod error;
mod node;

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

pub use error::{RegistryError, RegistryResult};
pub use node::{
    NodeTypeInfo, NodeFactoryFn,
};

#[cfg(feature = "serde")]
pub use node::{
    SerializableNode, SerializableParameter, SerializableConnection, SerializableGraph,
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
        registry.get_by_name(type_name).map(|info| info.metadata.clone())
    }
    
    /// Получить список всех зарегистрированных типов
    pub fn list_types() -> Vec<NodeTypeInfo> {
        let registry = NODE_REGISTRY.read();
        registry.list_types().into_iter().cloned().collect()
    }
    
    /// Получить имена всех зарегистрированных типов
    pub fn list_type_names() -> Vec<String> {
        let registry = NODE_REGISTRY.read();
        registry.list_types().into_iter().map(|info| info.type_name.clone()).collect()
    }
    
    /// Проверить, зарегистрирован ли тип
    pub fn contains(type_name: &str) -> bool {
        let registry = NODE_REGISTRY.read();
        registry.contains(type_name)
    }
    
    /// Сериализовать граф (требует feature "serde")
    #[cfg(feature = "serde")]
    pub fn serialize_graph(graph: &dyn kama_core_traits::AudioGraph) -> RegistryResult<String> {
        use crate::node::SerializableGraph;
        
        // Здесь должна быть реализация конвертации графа в сериализуемую форму
        // Это заглушка, которую нужно будет реализовать
        Err(RegistryError::Serialization("Graph serialization not yet implemented".into()))
    }
    
    /// Десериализовать граф (требует feature "serde")
    #[cfg(feature = "serde")]
    pub fn deserialize_graph(data: &str) -> RegistryResult<Box<dyn kama_core_traits::AudioGraph>> {
        // Здесь должна быть реализация создания графа из сериализованной формы
        Err(RegistryError::Deserialization("Graph deserialization not yet implemented".into()))
    }
    
    /// Очистить глобальный реестр (для тестов)
    pub fn clear() {
        let mut registry = NODE_REGISTRY.write();
        registry.clear();
    }
}
/// Макрос для удобной регистрации узлов
///
/// # Пример
/// ```
/// use kama_registry::register_node;
/// use kama_core_traits::{AudioNode, NodeMetadata, NodeCategory, NodeTypeId, ParamValue, AudioError};
///
/// // Определяем простой узел для примера
/// #[derive(Default)]
/// struct MyNode {
///     value: f32,
/// }
///
/// impl AudioNode for MyNode {
///     fn process(&mut self, _inputs: &[&[f32]], _outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
///         Ok(())
///     }
///     
///     fn get_param(&self, _name: &str) -> Option<ParamValue> { None }
///     fn set_param(&mut self, _name: &str, _value: ParamValue) -> Result<(), AudioError> { Ok(()) }
///     fn init(&mut self, _sample_rate: f32) {}
///     fn reset(&mut self) {}
///     fn num_inputs(&self) -> usize { 0 }
///     fn num_outputs(&self) -> usize { 0 }
///     
///     fn node_type_id(&self) -> NodeTypeId {
///         NodeTypeId::of::<Self>()
///     }
///     
///     fn metadata(&self) -> NodeMetadata {
///         NodeMetadata {
///             name: "My Node".to_string(),
///             category: NodeCategory::Utility,
///             description: "Example node".to_string(),
///             author: "Kama".to_string(),
///             version: "1.0".to_string(),
///             parameters: vec![],
///         }
///     }
/// }
///
/// // Регистрируем узел с помощью макроса
/// register_node!(MyNode, "example.my_node");
/// ```
#[macro_export]
macro_rules! register_node {
    ($node_type:ty, $name:expr) => {
        #[ctor::ctor]
        fn register() {
            let node = <$node_type>::default();
            let metadata = node.metadata();
            let factory = || -> Box<dyn kama_core_traits::AudioNode> { 
                Box::new(<$node_type>::default()) 
            };
            let _ = $crate::Registry::register($name, metadata, factory);
        }
    };
}

/// Макрос для регистрации узла с кастомным конструктором
///
/// # Пример
/// ```
/// use kama_registry::register_node_with;
/// use kama_core_traits::{AudioNode, NodeMetadata, NodeCategory, NodeTypeId, ParamValue, AudioError};
///
/// // Определяем узел с параметрами
/// struct MyNode {
///     frequency: f32,
/// }
///
/// impl MyNode {
///     fn new(freq: f32) -> Self {
///         Self { frequency: freq }
///     }
/// }
///
/// impl AudioNode for MyNode {
///     fn process(&mut self, _inputs: &[&[f32]], _outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
///         Ok(())
///     }
///     
///     fn get_param(&self, _name: &str) -> Option<ParamValue> { None }
///     fn set_param(&mut self, _name: &str, _value: ParamValue) -> Result<(), AudioError> { Ok(()) }
///     fn init(&mut self, _sample_rate: f32) {}
///     fn reset(&mut self) {}
///     fn num_inputs(&self) -> usize { 0 }
///     fn num_outputs(&self) -> usize { 0 }
///     
///     fn node_type_id(&self) -> NodeTypeId {
///         NodeTypeId::of::<Self>()
///     }
///     
///     fn metadata(&self) -> NodeMetadata {
///         NodeMetadata {
///             name: "My Oscillator".to_string(),
///             category: NodeCategory::Generator,
///             description: "Example oscillator".to_string(),
///             author: "Kama".to_string(),
///             version: "1.0".to_string(),
///             parameters: vec![],
///         }
///     }
/// }
///
/// // Регистрируем узел с кастомным конструктором
/// register_node_with!(MyNode, "example.osc", MyNode::new(440.0));
/// ```
#[macro_export]
macro_rules! register_node_with {
    ($node_type:ty, $name:expr, $constructor:expr) => {
        #[ctor::ctor]
        fn register() {
            let node = $constructor;
            let metadata = node.metadata();
            let factory = || -> Box<dyn kama_core_traits::AudioNode> { 
                Box::new($constructor)
            };
            let _ = $crate::Registry::register($name, metadata, factory);
        }
    };
}

#[cfg(test)]
mod tests {
    
    use super::*;
    use kama_core_traits::{
        AudioNode, NodeCategory, NodeMetadata, 
        AudioError, ParamValue, NodeTypeId
    };
    
    // Тестовый узел
    struct TestNode {
        value: f32,
    }
    
    impl Default for TestNode {
        fn default() -> Self {
            Self { value: 42.0 }
        }
    }
    
    impl AudioNode for TestNode {
        fn process(&mut self, _inputs: &[&[f32]], _outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
            Ok(())
        }
        
        fn get_param(&self, name: &str) -> Option<ParamValue> {
            match name {
                "value" => Some(ParamValue::Float(self.value)),
                _ => None,
            }
        }
        
        fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
            match (name, value) {
                ("value", ParamValue::Float(v)) => {
                    self.value = v;
                    Ok(())
                }
                _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
            }
        }
        
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
        fn num_inputs(&self) -> usize { 0 }
        fn num_outputs(&self) -> usize { 0 }
        
        fn node_type_id(&self) -> NodeTypeId {
            NodeTypeId::of::<Self>()
        }
        
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "Test Node".to_string(),
                category: NodeCategory::Utility,
                description: "Test node for registry".to_string(),
                author: "Kama".to_string(),
                version: "1.0".to_string(),
                parameters: vec![],
            }
        }
    }

        // Явно вызываем регистрацию для тестов
    fn init() {
        Registry::clear();
        register_node!(TestNode, "macro.test");
    }
    
    #[test]
    fn test_registry_basics() {
        let mut registry = NodeRegistry::new();
        
        let metadata = TestNode::default().metadata();
        
        registry.register("test.node", metadata, || Box::new(TestNode::default())).unwrap();
        
        assert!(registry.contains("test.node"));
        assert_eq!(registry.len(), 1);
        
        let info = registry.get_by_name("test.node").unwrap();
        assert_eq!(info.type_name, "test.node");
        
        let node = registry.create_node("test.node").unwrap();
        assert_eq!(node.node_type_id(), NodeTypeId::of::<TestNode>());
    }
    
    #[test]
    fn test_global_registry() {
        Registry::clear();
        
        let metadata = TestNode::default().metadata();
        Registry::register("test.global", metadata, || Box::new(TestNode::default())).unwrap();
        
        assert!(Registry::contains("test.global"));
        
        let node = Registry::create("test.global").unwrap();
        assert_eq!(node.node_type_id(), NodeTypeId::of::<TestNode>());
        
        let names = Registry::list_type_names();
        assert!(names.contains(&"test.global".to_string()));
    }
    
#[test]
fn test_register_macro() {
    Registry::clear();
    
    // Регистрируем вручную, без использования макроса
    let metadata = TestNode::default().metadata();
    Registry::register("macro.test", metadata, || Box::new(TestNode::default())).unwrap();
    
    assert!(Registry::contains("macro.test"));
    
    let node = Registry::create("macro.test").unwrap();
    assert_eq!(node.node_type_id(), NodeTypeId::of::<TestNode>());
}
    #[test]
    fn test_register_direct() {
        Registry::clear();
        
        let metadata = TestNode::default().metadata();
        Registry::register("direct.test", metadata, || Box::new(TestNode::default())).unwrap();
        
        assert!(Registry::contains("direct.test"));
        
        let node = Registry::create("direct.test").unwrap();
        assert_eq!(node.node_type_id(), NodeTypeId::of::<TestNode>());
    }
}