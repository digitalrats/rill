use kama_core_traits::{AudioNode, NodeTypeId, ParamValue};
use kama_buffers::MultiHeadBuffer;

fn main() {
    let sample_rate = 44100.0;
    
    // Создаём многоголовый буфер
    let mut buffer = MultiHeadBuffer::new(4096, sample_rate);
    
    // Добавляем головки
    let head1_id = buffer.add_head();
    let head2_id = buffer.add_head();
    
    if let Some(head1) = buffer.get_head_mut(head1_id) {
        head1.set_speed(0.5);
        head1.set_pan(-0.5);
    }
    
    if let Some(head2) = buffer.get_head_mut(head2_id) {
        head2.set_speed(2.0);
        head2.set_pan(0.5);
    }
    
    println!("MultiHeadBuffer создан");
    println!("Количество головок: {}", buffer.head_count());
    println!("Размер буфера: {} сэмплов", buffer.buffer_size());
    
    // Записываем тестовые данные
    let test_data: Vec<f32> = (0..256).map(|i| i as f32 / 255.0).collect();
    buffer.write(&test_data);
    
    // Обрабатываем
    let mut output_left = vec![0.0f32; 64];
    let mut output_right = vec![0.0f32; 64];
    let mut outputs = [&mut output_left[..], &mut output_right[..]];
    
    buffer.process(&[], &mut outputs).unwrap();
    
    println!("Готово! Сигнал обработан.");
}