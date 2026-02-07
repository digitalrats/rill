use kama_core::{AudioGraph, AudioNode};
use kama_buffers::MultiHeadBuffer;

fn main() {
    let sample_rate = 44100.0;
    
    // Создаём граф
    let mut graph = AudioGraph::new(sample_rate);
    
    // Создаём многоголовый буфер
    let mut buffer = MultiHeadBuffer::new(4096, sample_rate);
    
    // Добавляем головки
    let head1_id = buffer.add_head();
    let head2_id = buffer.add_head();
    
    if let Some(head1) = buffer.get_head_mut(head1_id) {
        head1.state.speed = 0.5;
        head1.state.pan = -0.5;
    }
    
    if let Some(head2) = buffer.get_head_mut(head2_id) {
        head2.state.speed = 2.0;
        head2.state.pan = 0.5;
    }
    
    // Добавляем в граф
    let buffer_id = graph.add_node(Box::new(buffer));
    
    println!("MultiHeadBuffer добавлен в граф с ID: {:?}", buffer_id);
    println!("Готово!");
}