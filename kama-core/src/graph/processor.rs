use std::collections::HashMap;
use crate::node::AudioNode;
use crate::AudioError;
use crate::graph::NodeId;  // Правильный импорт

/// Временные буферы для обработки узла
#[derive(Default)]
pub struct NodeBuffers {
    pub inputs: Vec<Vec<f32>>,
    pub outputs: Vec<Vec<f32>>,
}

/// Пул буферов для повторного использования
pub struct BufferPool {
    buffers: Vec<Vec<f32>>,
    buffer_size: usize,
}

impl BufferPool {
    pub fn new(pool_size: usize, buffer_size: usize) -> Self {
        let mut buffers = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            buffers.push(vec![0.0; buffer_size]);
        }
        
        Self { buffers, buffer_size }
    }
    
    pub fn acquire(&mut self, size: usize) -> Vec<f32> {
        if let Some(mut buffer) = self.buffers.pop() {
            if buffer.len() != size {
                buffer.resize(size, 0.0);
            } else {
                buffer.fill(0.0);
            }
            buffer
        } else {
            vec![0.0; size]
        }
    }
    
    pub fn release(&mut self, mut buffer: Vec<f32>) {
        if buffer.len() == self.buffer_size {
            buffer.fill(0.0);
            self.buffers.push(buffer);
        }
    }
}

/// Менеджер буферов для графа
pub struct BufferManager {
    node_buffers: HashMap<NodeId, NodeBuffers>,
    buffer_pool: BufferPool,
}

impl BufferManager {
    pub fn new() -> Self {
        Self {
            node_buffers: HashMap::new(),
            buffer_pool: BufferPool::new(16, 4096),
        }
    }
    
    /// Получить или создать буферы для узла
    pub fn get_buffers(&mut self, node_id: NodeId, num_inputs: usize, num_outputs: usize, buffer_size: usize) -> &mut NodeBuffers {
        let buffers = self.node_buffers.entry(node_id).or_insert_with(NodeBuffers::default);
        
        if buffers.inputs.len() != num_inputs {
            buffers.inputs.clear();
            for _ in 0..num_inputs {
                buffers.inputs.push(self.buffer_pool.acquire(buffer_size));
            }
        } else {
            for input in &mut buffers.inputs {
                if input.len() != buffer_size {
                    input.resize(buffer_size, 0.0);
                } else {
                    input.fill(0.0);
                }
            }
        }
        
        if buffers.outputs.len() != num_outputs {
            buffers.outputs.clear();
            for _ in 0..num_outputs {
                buffers.outputs.push(self.buffer_pool.acquire(buffer_size));
            }
        } else {
            for output in &mut buffers.outputs {
                if output.len() != buffer_size {
                    output.resize(buffer_size, 0.0);
                } else {
                    output.fill(0.0);
                }
            }
        }
        
        buffers
    }
    
    /// Освободить все буферы
    pub fn release_all(&mut self) {
        for buffers in self.node_buffers.values() {
            for buffer in &buffers.inputs {
                self.buffer_pool.release(buffer.clone());
            }
            for buffer in &buffers.outputs {
                self.buffer_pool.release(buffer.clone());
            }
        }
        self.node_buffers.clear();
    }
}

/// Процессор для выполнения узла
pub struct NodeProcessor;

impl NodeProcessor {
    pub fn new() -> Self {
        Self
    }
    
    /// Обработать один узел
    pub fn process(
        &self,
        node: &mut dyn AudioNode,
        buffers: &mut NodeBuffers,
    ) -> Result<(), AudioError> {
        let input_slices: Vec<&[f32]> = buffers.inputs.iter()
            .map(|buf| buf.as_slice())
            .collect();
        
        let mut output_slices: Vec<&mut [f32]> = buffers.outputs.iter_mut()
            .map(|buf| buf.as_mut_slice())
            .collect();
        
        node.process(&input_slices, &mut output_slices)
    }
}