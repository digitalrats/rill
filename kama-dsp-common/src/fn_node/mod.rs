//! Конструкторы для создания узлов из функций

use std::marker::PhantomData;

use kama_core_traits::{
    AudioNode, AudioError, NodeTypeId, NodeMetadata, NodeCategory,
    param::{ParamValue},
};
use kama_core_traits::time::TimeProvider;  // <-- Добавляем импорт

use crate::context::DspContext;
use crate::dummy::DummyTimeProvider;

// -----------------------------------------------------------------------------
// Stateless Node (без состояния)
// -----------------------------------------------------------------------------

/// Внутренняя структура для stateless узла
struct StatelessNodeCore<F> {
    func: F,
    metadata: NodeMetadata,
    sample_rate: f32,
    num_inputs: usize,
    num_outputs: usize,
}

impl<F> AudioNode for StatelessNodeCore<F>
where
    F: Fn(f32, &DspContext) -> f32 + Send + Sync + 'static,
{
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }
        
        let input = inputs[0];
        let output = &mut outputs[0];
        let buffer_size = output.len().min(input.len());
        
        // Создаем временный контекст (в реальности будет передаваться из графа)
        let dummy_time = crate::dummy::DummyTimeProvider;
        let dummy_buffers = kama_buffers::BufferRegistry::new();
        let ctx = DspContext {
            time: &dummy_time,
            sample_rate: self.sample_rate,
            block_size: buffer_size,
            block_position: 0,
            buffers: &dummy_buffers,
            user_data: None,
        };
        
        for i in 0..buffer_size {
            output[i] = (self.func)(input[i], &ctx);
        }
        
        Ok(())
    }
    
    fn get_param(&self, _name: &str) -> Option<ParamValue> { None }
    
    fn set_param(&mut self, _name: &str, _value: ParamValue) -> Result<(), AudioError> {
        Ok(())
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }
    
    fn reset(&mut self) {}
    
    fn num_inputs(&self) -> usize { self.num_inputs }
    fn num_outputs(&self) -> usize { self.num_outputs }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
    }
}

/// Создать stateless узел из функции
///
/// # Пример
/// ```
/// use kama_dsp_common::{stateless_fn_node, NodeCategory};
///
/// let gain_node = stateless_fn_node(
///     "Gain",
///     NodeCategory::Effect,
///     |sample, ctx| sample * 0.5
/// );
/// ```
pub fn stateless_fn_node<F>(
    name: &str,
    category: NodeCategory,
    func: F,
) -> impl AudioNode
where
    F: Fn(f32, &DspContext) -> f32 + Send + Sync + 'static,
{
    let metadata = NodeMetadata {
        name: name.to_string(),
        category,
        description: format!("Stateless function node: {}", name),
        author: "Kama DSP Common".to_string(),
        version: "0.1.0".to_string(),
        parameters: vec![],
    };
    
    StatelessNodeCore {
        func,
        metadata,
        sample_rate: 44100.0,
        num_inputs: 1,
        num_outputs: 1,
    }
}

// -----------------------------------------------------------------------------
// Stateful Node (с состоянием)
// -----------------------------------------------------------------------------

/// Внутренняя структура для stateful узла
struct StatefulNodeCore<F, S> {
    func: F,
    state: S,
    metadata: NodeMetadata,
    sample_rate: f32,
    num_inputs: usize,
    num_outputs: usize,
    _phantom: PhantomData<fn() -> S>,
}

impl<S, F> AudioNode for StatefulNodeCore<F, S>
where
    S: Send + Sync + 'static,
    F: Fn(f32, &mut S, &DspContext) -> f32 + Send + Sync + 'static,
{
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }
        
        let input = inputs[0];
        let output = &mut outputs[0];
        let buffer_size = output.len().min(input.len());
        
        let dummy_time = crate::dummy::DummyTimeProvider;
        let dummy_buffers = kama_buffers::BufferRegistry::new();
        let ctx = DspContext {
            time: &dummy_time,
            sample_rate: self.sample_rate,
            block_size: buffer_size,
            block_position: 0,
            buffers: &dummy_buffers,
            user_data: None,
        };
        
        for i in 0..buffer_size {
            output[i] = (self.func)(input[i], &mut self.state, &ctx);
        }
        
        Ok(())
    }
    
    fn get_param(&self, _name: &str) -> Option<ParamValue> { None }
    
    fn set_param(&mut self, _name: &str, _value: ParamValue) -> Result<(), AudioError> {
        Ok(())
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }
    
    fn reset(&mut self) {}
    
    fn num_inputs(&self) -> usize { self.num_inputs }
    fn num_outputs(&self) -> usize { self.num_outputs }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
    }
}

/// Создать stateful узел из функции с состоянием
///
/// # Пример
/// ```
/// use kama_dsp_common::{stateful_fn_node, NodeCategory};
///
/// let filter_node = stateful_fn_node(
///     "OnePole",
///     NodeCategory::Filter,
///     0.0, // начальное состояние
///     |sample, state, ctx| {
///         let alpha = 0.1;
///         *state = *state + alpha * (sample - *state);
///         *state
///     }
/// );
/// ```
pub fn stateful_fn_node<S, F>(
    name: &str,
    category: NodeCategory,
    initial_state: S,
    func: F,
) -> impl AudioNode
where
    S: Send + Sync + 'static,
    F: Fn(f32, &mut S, &DspContext) -> f32 + Send + Sync + 'static,
{
    let metadata = NodeMetadata {
        name: name.to_string(),
        category,
        description: format!("Stateful function node: {}", name),
        author: "Kama DSP Common".to_string(),
        version: "0.1.0".to_string(),
        parameters: vec![],
    };
    
    StatefulNodeCore {
        func,
        state: initial_state,
        metadata,
        sample_rate: 44100.0,
        num_inputs: 1,
        num_outputs: 1,
        _phantom: PhantomData,
    }
}

// -----------------------------------------------------------------------------
// Multi-channel и Block Processing
// -----------------------------------------------------------------------------

/// Создать узел для обработки целого блока (полезно для SIMD оптимизаций)
pub fn block_fn_node<F>(
    name: &str,
    category: NodeCategory,
    num_inputs: usize,
    num_outputs: usize,
    func: F,
) -> impl AudioNode
where
    F: Fn(&[&[f32]], &mut [&mut [f32]], &DspContext) -> Result<(), AudioError> + Send + Sync + 'static,
{
    struct BlockNode<F> {
        func: F,
        metadata: NodeMetadata,
        sample_rate: f32,
        num_inputs: usize,
        num_outputs: usize,
    }
    
    impl<F> AudioNode for BlockNode<F>
    where
        F: Fn(&[&[f32]], &mut [&mut [f32]], &DspContext) -> Result<(), AudioError> + Send + Sync + 'static,
    {
        fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
            let dummy_time = crate::dummy::DummyTimeProvider;
            let dummy_buffers = kama_buffers::BufferRegistry::new();
            let ctx = DspContext {
                time: &dummy_time,
                sample_rate: self.sample_rate,
                block_size: outputs.get(0).map(|o| o.len()).unwrap_or(0),
                block_position: 0,
                buffers: &dummy_buffers,
                user_data: None,
            };
            
            (self.func)(inputs, outputs, &ctx)
        }
        
        fn get_param(&self, _name: &str) -> Option<ParamValue> { None }
        fn set_param(&mut self, _name: &str, _value: ParamValue) -> Result<(), AudioError> { Ok(()) }
        fn init(&mut self, sample_rate: f32) { self.sample_rate = sample_rate; }
        fn reset(&mut self) {}
        fn num_inputs(&self) -> usize { self.num_inputs }
        fn num_outputs(&self) -> usize { self.num_outputs }
        fn node_type_id(&self) -> NodeTypeId { NodeTypeId::of::<Self>() }
        fn metadata(&self) -> NodeMetadata { self.metadata.clone() }
    }
    
    let metadata = NodeMetadata {
        name: name.to_string(),
        category,
        description: format!("Block processing node: {}", name),
        author: "Kama DSP Common".to_string(),
        version: "0.1.0".to_string(),
        parameters: vec![],
    };
    
    BlockNode {
        func,
        metadata,
        sample_rate: 44100.0,
        num_inputs,
        num_outputs,
    }
}