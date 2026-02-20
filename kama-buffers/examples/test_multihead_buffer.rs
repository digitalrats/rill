use kama_graph::AudioGraph;
use kama_core_traits::{AudioNode, ParamValue, NodeTypeId};
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
    
    // Получаем информацию через параметры
    if let Some(node) = graph.get_node(buffer_id) {
        if node.node_type_id() == NodeTypeId::of::<MultiHeadBuffer>() {
            if let Some(ParamValue::Int(num_heads)) = node.get_param("num_heads") {
                println!("Количество головок: {}", num_heads);
            }
            if let Some(ParamValue::Int(buffer_size)) = node.get_param("buffer_size") {
                println!("Размер буфера: {} сэмплов", buffer_size);
            }
        }
    }
    
    println!("Готово!");
}