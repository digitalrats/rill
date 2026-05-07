use rill_core::math::Transcendental;
use rill_core::traits::{Node, NodeId, NodeMetadata, NodeParams, NodeVariant};
use std::collections::HashMap;

// ============================================================================
// Registry Error
// ============================================================================

/// Errors that can occur during node construction via the registry.
#[derive(Debug, Clone)]
pub enum RegistryError {
    /// No constructor registered for the given type name.
    UnknownType(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownType(name) => write!(f, "unknown node type: {name}"),
        }
    }
}

impl std::error::Error for RegistryError {}

// ============================================================================
// NodeConstructor Trait
// ============================================================================

/// Factory trait for creating graph nodes by type name.
///
/// Each node type that wants to be constructable via the registry
/// implements this trait. The [`construct`](Self::construct) method
/// receives a [`NodeId`] and [`NodeParams`] and must return the
/// appropriate [`NodeVariant`].
pub trait NodeConstructor<T: Transcendental, const BUF_SIZE: usize>: Send + Sync {
    /// Canonical name for this node type (e.g. `"rill/sine_osc"`).
    fn type_name(&self) -> &'static str;

    /// Build a fully initialised node variant.
    ///
    /// Implementations should:
    /// 1. Extract parameters from `params`.
    /// 2. Create the concrete node.
    /// 3. Call [`Node::set_id`] with the given `id`.
    /// 4. Call [`Node::init`] with `params.sample_rate`.
    /// 5. Wrap in the correct [`NodeVariant`] variant.
    fn construct(&self, id: NodeId, params: &NodeParams) -> NodeVariant<T, BUF_SIZE>;
}

// ============================================================================
// NodeRegistry
// ============================================================================

/// A registry of named node constructors.
///
/// Register constructors with [`register`](Self::register), then create
/// nodes by type name with [`construct`](Self::construct).
///
/// # Type parameters
///
/// - `T` — sample type (typically `f32`)
/// - `BUF_SIZE` — block size (must match the target graph)
pub struct NodeRegistry<T: Transcendental, const BUF_SIZE: usize> {
    entries: HashMap<&'static str, Box<dyn NodeConstructor<T, BUF_SIZE>>>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Default for NodeRegistry<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> NodeRegistry<T, BUF_SIZE> {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Register a node constructor.
    ///
    /// The constructor's [`type_name`](NodeConstructor::type_name) is used
    /// as the lookup key. If a constructor with the same name already exists,
    /// it is replaced.
    pub fn register(&mut self, ctor: impl NodeConstructor<T, BUF_SIZE> + 'static) {
        let name = ctor.type_name();
        self.entries.insert(name, Box::new(ctor));
    }

    /// Register a node type via a closure.
    ///
    /// This is a convenience wrapper around [`register`](Self::register) for
    /// cases where a full struct + trait impl is not needed.
    pub fn register_fn(
        &mut self,
        type_name: &'static str,
        f: impl Fn(NodeId, &NodeParams) -> NodeVariant<T, BUF_SIZE> + Send + Sync + 'static,
    ) {
        self.entries.insert(
            type_name,
            Box::new(ClosureCtor {
                type_name,
                f: Box::new(f),
            }),
        );
    }

    /// Construct a node by type name.
    ///
    /// Returns [`RegistryError::UnknownType`] if the name has not been
    /// registered.
    pub fn construct(
        &self,
        type_name: &str,
        id: NodeId,
        params: &NodeParams,
    ) -> Result<NodeVariant<T, BUF_SIZE>, RegistryError> {
        self.entries
            .get(type_name)
            .ok_or_else(|| RegistryError::UnknownType(type_name.to_string()))
            .map(|ctor| ctor.construct(id, params))
    }

    /// Check whether a type name is registered.
    pub fn contains(&self, type_name: &str) -> bool {
        self.entries.contains_key(type_name)
    }

    /// List all registered type names.
    pub fn list_types(&self) -> Vec<&'static str> {
        self.entries.keys().copied().collect()
    }

    /// Number of registered constructors.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no constructors are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get metadata for a registered type without constructing a node.
    ///
    /// This requires constructing a temporary node and immediately
    /// discarding it. If performance is a concern, cache the metadata
    /// alongside the constructor in the registry.
    pub fn metadata(&self, type_name: &str) -> Option<NodeMetadata> {
        self.entries.get(type_name).map(|ctor| {
            let dummy = NodeParams::new(44100.0);
            let variant = ctor.construct(NodeId(u32::MAX), &dummy);
            variant.metadata()
        })
    }
}

// ============================================================================
// Internal: closure-based constructor wrapper
// ============================================================================

#[allow(clippy::type_complexity)]
struct ClosureCtor<T: Transcendental, const BUF_SIZE: usize> {
    type_name: &'static str,
    f: Box<dyn Fn(NodeId, &NodeParams) -> NodeVariant<T, BUF_SIZE> + Send + Sync>,
}

impl<T: Transcendental, const BUF_SIZE: usize> NodeConstructor<T, BUF_SIZE>
    for ClosureCtor<T, BUF_SIZE>
{
    fn type_name(&self) -> &'static str {
        self.type_name
    }

    fn construct(&self, id: NodeId, params: &NodeParams) -> NodeVariant<T, BUF_SIZE> {
        (self.f)(id, params)
    }
}

// ============================================================================
// Node Ctor Macro
// ============================================================================

/// Register a node constructor by type name.
///
/// Shorthand for [`NodeRegistry::register_fn`]. Emits a call to
/// `registry.register_fn(type_name, closure)`.
///
/// # Example
///
/// ```rust
/// use rill_graph::{node_ctor, NodeRegistry};
/// use rill_core::traits::{NodeId, NodeParams, NodeVariant, Source, Node};
///
/// // Inside a function that has access to a &mut NodeRegistry<f32, 64>:
/// fn register(registry: &mut NodeRegistry<f32, 64>) {
///     node_ctor!(registry, "test/my_source", |id, params| {
///         // construct and return NodeVariant
///         todo!()
///     });
/// }
/// ```
#[macro_export]
macro_rules! node_ctor {
    ($registry:expr, $type_name:expr, $ctor:expr) => {
        $registry.register_fn($type_name, $ctor);
    };
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::time::ClockTick;
    use rill_core::traits::node::NodeState;
    use rill_core::traits::port::Port;
    use rill_core::traits::NodeCategory;
    use rill_core::traits::Processor;
    use rill_core::traits::Source;
    use rill_core::traits::{ParamValue, ProcessResult};

    // ── Test helpers ────────────────────────────────────────────────

    struct TestSource<T: Transcendental, const B: usize> {
        id: NodeId,
        state: NodeState<T, B>,
        output: Port<T, B>,
        meta_name: &'static str,
        meta_cat: NodeCategory,
    }

    impl<T: Transcendental, const B: usize> TestSource<T, B> {
        fn new() -> Self {
            Self {
                id: NodeId(0),
                state: NodeState::new(44100.0),
                output: Port::output(NodeId(0), 0, "out"),
                meta_name: "TestSource",
                meta_cat: NodeCategory::Source,
            }
        }

        fn set_id_and_init(&mut self, id: NodeId, sample_rate: f32) {
            self.id = id;
            self.state.sample_rate = sample_rate;
        }
    }

    impl<T: Transcendental, const B: usize> Node<T, B> for TestSource<T, B> {
        fn metadata(&self) -> rill_core::traits::NodeMetadata {
            rill_core::traits::NodeMetadata::new(self.meta_name, self.meta_cat)
        }
        fn init(&mut self, sample_rate: f32) {
            self.state.sample_rate = sample_rate;
        }
        fn reset(&mut self) {}
        fn get_parameter(
            &self,
            _: &rill_core::traits::ParameterId,
        ) -> Option<rill_core::traits::ParamValue> {
            None
        }
        fn set_parameter(
            &mut self,
            _: &rill_core::traits::ParameterId,
            _: rill_core::traits::ParamValue,
        ) -> ProcessResult<()> {
            Ok(())
        }
        fn id(&self) -> NodeId {
            self.id
        }
        fn set_id(&mut self, id: NodeId) {
            self.id = id;
        }
        fn input_port(&self, _: usize) -> Option<&Port<T, B>> {
            None
        }
        fn input_port_mut(&mut self, _: usize) -> Option<&mut Port<T, B>> {
            None
        }
        fn output_port(&self, index: usize) -> Option<&Port<T, B>> {
            if index == 0 {
                Some(&self.output)
            } else {
                None
            }
        }
        fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<T, B>> {
            if index == 0 {
                Some(&mut self.output)
            } else {
                None
            }
        }
        fn control_port(&self, _: usize) -> Option<&Port<T, B>> {
            None
        }
        fn control_port_mut(&mut self, _: usize) -> Option<&mut Port<T, B>> {
            None
        }
        fn state(&self) -> &NodeState<T, B> {
            &self.state
        }
        fn state_mut(&mut self) -> &mut NodeState<T, B> {
            &mut self.state
        }
    }

    impl<T: Transcendental, const B: usize> Source<T, B> for TestSource<T, B> {
        fn generate(&mut self, _: &ClockTick, _: &[T], _: &[ClockTick]) -> ProcessResult<()> {
            Ok(())
        }
    }

    impl<T: Transcendental, const B: usize> Processor<T, B> for TestSource<T, B> {
        fn process(
            &mut self,
            _: &ClockTick,
            _: &[&[T; B]],
            _: &[T],
            _: &[ClockTick],
            _: &[&[T; B]],
        ) -> ProcessResult<()> {
            Ok(())
        }
        fn latency(&self) -> usize {
            0
        }
    }

    struct TestSourceCtor;
    impl<T: Transcendental, const B: usize> NodeConstructor<T, B> for TestSourceCtor {
        fn type_name(&self) -> &'static str {
            "test/source"
        }
        fn construct(&self, id: NodeId, params: &NodeParams) -> NodeVariant<T, B> {
            let mut node = TestSource::<T, B>::new();
            node.set_id_and_init(id, params.sample_rate);
            NodeVariant::Source(Box::new(node))
        }
    }

    struct TestProcessorCtor;
    impl<T: Transcendental, const B: usize> NodeConstructor<T, B> for TestProcessorCtor {
        fn type_name(&self) -> &'static str {
            "test/processor"
        }
        fn construct(&self, id: NodeId, params: &NodeParams) -> NodeVariant<T, B> {
            let mut node = TestSource::<T, B>::new();
            node.meta_name = "Noop";
            node.meta_cat = NodeCategory::Processor;
            node.set_id_and_init(id, params.sample_rate);
            NodeVariant::Processor(Box::new(node))
        }
    }

    // ── Tests ───────────────────────────────────────────────────────

    #[test]
    fn test_registry_empty() {
        let registry = NodeRegistry::<f32, 64>::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_registry_register_and_construct() {
        let mut registry = NodeRegistry::<f32, 64>::new();
        registry.register(TestSourceCtor);

        assert!(registry.contains("test/source"));
        assert_eq!(registry.len(), 1);

        let params = NodeParams::new(48000.0);
        let variant = registry
            .construct("test/source", NodeId(42), &params)
            .expect("should construct");

        match &variant {
            NodeVariant::Source(_) => {}
            _ => panic!("expected Source variant"),
        }

        // Verify init was called (sample_rate stored in state)
        assert_eq!(variant.metadata().name, "TestSource");
    }

    #[test]
    fn test_registry_unknown_type() {
        let registry = NodeRegistry::<f32, 64>::new();
        let params = NodeParams::new(44100.0);
        let result = registry.construct("nonexistent", NodeId(0), &params);
        assert!(result.is_err());
        match result {
            Err(RegistryError::UnknownType(name)) => assert_eq!(name, "nonexistent"),
            _ => panic!("expected UnknownType error"),
        }
    }

    #[test]
    fn test_registry_register_fn() {
        let mut registry = NodeRegistry::<f32, 64>::new();
        registry.register_fn("test/fn_ctor", |id, params| {
            let mut node = TestSource::<f32, 64>::new();
            node.set_id(id);
            node.init(params.sample_rate);
            NodeVariant::Source(Box::new(node))
        });

        assert!(registry.contains("test/fn_ctor"));
        let params = NodeParams::new(44100.0);
        let variant = registry
            .construct("test/fn_ctor", NodeId(1), &params)
            .expect("should construct from fn");
        match variant {
            NodeVariant::Source(_) => {}
            _ => panic!("expected Source variant"),
        }
    }

    #[test]
    fn test_registry_list_types() {
        let mut registry = NodeRegistry::<f32, 64>::new();
        registry.register(TestSourceCtor);
        registry.register(TestProcessorCtor);

        let mut types = registry.list_types();
        types.sort();
        assert_eq!(types, vec!["test/processor", "test/source"]);
    }

    #[test]
    fn test_registry_replace() {
        let mut registry = NodeRegistry::<f32, 64>::new();
        registry.register(TestSourceCtor);
        assert_eq!(registry.len(), 1);

        // Registering again under the same name replaces.
        registry.register(TestSourceCtor);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_registry_metadata() {
        let mut registry = NodeRegistry::<f32, 64>::new();
        registry.register(TestSourceCtor);

        let meta = registry.metadata("test/source");
        assert!(meta.is_some());
        assert_eq!(meta.unwrap().name, "TestSource");
    }

    #[test]
    fn test_construct_with_params() {
        let mut registry = NodeRegistry::<f32, 64>::new();
        registry.register_fn("test/with_params", |id, params| {
            let freq = params.get_f32("frequency", 440.0);
            assert_eq!(freq, 220.0);
            let amp = params.get_f32("amplitude", 0.5);
            assert_eq!(amp, 0.8);

            let mut node = TestSource::<f32, 64>::new();
            node.set_id(id);
            node.init(params.sample_rate);
            NodeVariant::Source(Box::new(node))
        });

        let params = NodeParams::new(44100.0)
            .with("frequency", ParamValue::Float(220.0))
            .with("amplitude", ParamValue::Float(0.8));
        let result = registry.construct("test/with_params", NodeId(0), &params);
        assert!(result.is_ok());
    }
}
