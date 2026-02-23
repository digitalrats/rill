//! Тесты для RingBuffer с новым функционалом lookahead

use kama_buffers::RingBuffer;

#[test]
fn test_ring_buffer_basic() {
    let mut buffer = RingBuffer::new(8);
    let test_data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    buffer.write(&test_data);

    let mut output = vec![0.0; 3];
    buffer.read(1, &mut output);
    assert_eq!(output, [5.0, 4.0, 3.0]);

    buffer.read(2, &mut output);
    assert_eq!(output, [4.0, 3.0, 2.0]);
}

#[test]
fn test_ring_buffer_wraparound() {
    let mut buffer = RingBuffer::new(4);
    buffer.write(&[1.0, 2.0, 3.0, 4.0]);

    let mut output = vec![0.0; 2];
    buffer.read(1, &mut output);
    assert_eq!(output, [4.0, 3.0]);

    buffer.read(2, &mut output);
    assert_eq!(output, [3.0, 2.0]);

    buffer.read(3, &mut output);
    assert_eq!(output, [2.0, 1.0]);
}

#[test]
fn test_ring_buffer_overwrite() {
    let mut buffer = RingBuffer::new(4);
    buffer.write(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);

    let mut output = vec![0.0; 4];
    buffer.read(1, &mut output);
    assert_eq!(output, [6.0, 5.0, 4.0, 3.0]);
}

#[test]
fn test_ring_buffer_interpolated() {
    let mut buffer = RingBuffer::new(4);
    buffer.write(&[1.0, 2.0, 3.0, 4.0]);

    let mut output = vec![0.0; 2];
    buffer.read_interpolated(1.5, &mut output);

    assert!((output[0] - 3.5).abs() < 1e-6);
    assert!((output[1] - 2.5).abs() < 1e-6);
}

#[test]
fn test_ring_buffer_lookahead_basic() {
    let mut buffer = RingBuffer::new(8);
    let test_data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    buffer.write(&test_data);

    let mut output = vec![0.0; 3];
    buffer.read_lookahead(1, &mut output);

    // write_pos = 8, lookahead=1 читает позицию (8+1)%8=1 -> значение 2.0
    assert_eq!(output, [2.0, 3.0, 4.0]);

    let mut output = vec![0.0; 2];
    buffer.read_lookahead(2, &mut output);
    // lookahead=2 читает позиции (8+2)%8=2, (8+3)%8=3 -> 3.0, 4.0
    assert_eq!(output, [3.0, 4.0]);
}

#[test]
fn test_ring_buffer_lookahead_with_offset() {
    let mut buffer = RingBuffer::new(8);
    let test_data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    buffer.write(&test_data);

    let view = buffer.view();

    // lookahead=1: позиция (8+1)%8=1 -> значение 2.0
    assert_eq!(view.read_lookahead(1, 0), 2.0);
    assert_eq!(view.read_lookahead(2, 0), 3.0);
    assert_eq!(view.read_lookahead(3, 0), 4.0);

    // С offset: добавляем offset к вычисленной позиции
    assert_eq!(view.read_lookahead(1, 1), 3.0); // позиция (1+1)%8=2 -> 3.0
    assert_eq!(view.read_lookahead(2, 2), 5.0); // позиция (2+2)%8=4 -> 5.0
}

#[test]
fn test_ring_buffer_lookahead_wraparound() {
    let mut buffer = RingBuffer::new(4);
    let test_data = vec![1.0, 2.0, 3.0, 4.0];
    buffer.write(&test_data);

    let view = buffer.view();

    // write_pos = 4%4=0, filled = true
    assert_eq!(view.read_lookahead(0, 0), 1.0); // позиция 0 -> 1.0
    assert_eq!(view.read_lookahead(1, 0), 2.0); // позиция 1 -> 2.0
    assert_eq!(view.read_lookahead(2, 0), 3.0); // позиция 2 -> 3.0
    assert_eq!(view.read_lookahead(3, 0), 4.0); // позиция 3 -> 4.0
    assert_eq!(view.read_lookahead(4, 0), 1.0); // позиция 4%4=0 -> 1.0
    assert_eq!(view.read_lookahead(5, 0), 2.0); // позиция 5%4=1 -> 2.0
}

#[test]
fn test_ring_buffer_lookahead_before_filled() {
    let mut buffer = RingBuffer::new(8);

    // Записываем только 5 семплов
    buffer.write(&[1.0, 2.0, 3.0, 4.0, 5.0]);

    let view = buffer.view();

    // write_pos = 5, filled = false
    // Семплы находятся на позициях 0..4
    assert_eq!(view.read_lookahead(0, 0), 1.0);
    assert_eq!(view.read_lookahead(1, 0), 2.0);
    assert_eq!(view.read_lookahead(2, 0), 3.0);
    assert_eq!(view.read_lookahead(3, 0), 4.0);
    assert_eq!(view.read_lookahead(4, 0), 5.0);
    assert_eq!(view.read_lookahead(5, 0), 0.0); // за пределами
}

#[test]
fn test_ring_buffer_lookahead_after_overwrite() {
    let mut buffer = RingBuffer::new(4);

    buffer.write(&[1.0, 2.0, 3.0, 4.0]);
    println!(
        "After first write: write_pos={}, filled={}",
        buffer.write_pos(),
        buffer.is_filled()
    );

    buffer.write(&[5.0, 6.0]);
    println!(
        "After second write: write_pos={}, filled={}",
        buffer.write_pos(),
        buffer.is_filled()
    );

    let view = buffer.view();

    for i in 0..6 {
        println!("read_lookahead({}, 0) = {}", i, view.read_lookahead(i, 0));
    }

    // Ожидаемые значения
    assert_eq!(view.read_lookahead(0, 0), 3.0);
    assert_eq!(view.read_lookahead(1, 0), 4.0);
    assert_eq!(view.read_lookahead(2, 0), 5.0);
    assert_eq!(view.read_lookahead(3, 0), 6.0);
    assert_eq!(view.read_lookahead(4, 0), 3.0);
}

#[test]
fn test_ring_buffer_lookahead_sequence() {
    let mut buffer = RingBuffer::new(8);
    buffer.write(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);

    let mut output = vec![0.0; 4];
    buffer.read_lookahead(2, &mut output);

    // Должны получить семплы с позиций write_pos+2, +3, +4, +5
    // write_pos=8, позиции 10,11,12,13 -> 3,4,5,6
    assert_eq!(output, [3.0, 4.0, 5.0, 6.0]);

    let mut output = vec![0.0; 3];
    buffer.read_lookahead(5, &mut output);

    // С wrap-around: позиции 13,14,15 -> 6,7,8
    assert_eq!(output, [6.0, 7.0, 8.0]);
}

#[test]
fn test_ring_buffer_lookahead_with_read_delayed_comparison() {
    let mut buffer = RingBuffer::new(8);
    buffer.write(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);

    let view = buffer.view();

    // read_delayed смотрит в прошлое, read_lookahead - в будущее
    assert_eq!(view.read_delayed(1, 0), 8.0); // последний записанный
    assert_eq!(view.read_lookahead(1, 0), 2.0); // следующий после самого старого

    assert_eq!(view.read_delayed(2, 0), 7.0);
    assert_eq!(view.read_lookahead(2, 0), 3.0);

    assert_eq!(view.read_delayed(3, 0), 6.0);
    assert_eq!(view.read_lookahead(3, 0), 4.0);
}

#[test]
fn test_ring_buffer_lookahead_edge_cases() {
    let mut buffer = RingBuffer::new(4);

    // Пустой буфер
    {
        let view = buffer.view();
        assert_eq!(view.read_lookahead(0, 0), 0.0);
        assert_eq!(view.read_lookahead(1, 0), 0.0);
    }

    // Частично заполненный (2 семпла)
    buffer.write(&[1.0, 2.0]);
    {
        let view = buffer.view();
        // write_pos = 2, filled = false
        // Семплы: индекс 0 = 1.0, индекс 1 = 2.0
        assert_eq!(view.read_lookahead(0, 0), 1.0);
        assert_eq!(view.read_lookahead(1, 0), 2.0);
        assert_eq!(view.read_lookahead(2, 0), 0.0); // за пределами
    }

    // Добавляем ещё 2 семпла (теперь полный)
    buffer.write(&[3.0, 4.0]);
    {
        let view = buffer.view();
        // write_pos = 4%4=0, filled = true
        // Для заполненного буфера семплы циклические
        assert_eq!(view.read_lookahead(0, 0), 1.0);
        assert_eq!(view.read_lookahead(1, 0), 2.0);
        assert_eq!(view.read_lookahead(2, 0), 3.0);
        assert_eq!(view.read_lookahead(3, 0), 4.0);
        assert_eq!(view.read_lookahead(4, 0), 1.0); // зацикливание
    }
}

#[test]
fn test_ring_buffer_lookahead_stress() {
    let mut buffer = RingBuffer::new(16);

    // Заполняем последовательностью
    let test_data: Vec<f32> = (0..16).map(|i| i as f32).collect();
    buffer.write(&test_data);

    let view = buffer.view();

    // write_pos = 16%16=0, filled = true
    // Для заполненного буфера все позиции доступны
    for lookahead in 0..32 {
        let expected = (lookahead % 16) as f32;
        assert_eq!(
            view.read_lookahead(lookahead, 0),
            expected,
            "Mismatch at lookahead {}",
            lookahead
        );
    }
}

#[test]
fn test_ring_buffer_lookahead_with_offset_stress() {
    let mut buffer = RingBuffer::new(8);
    let test_data: Vec<f32> = (0..8).map(|i| i as f32).collect();
    buffer.write(&test_data);

    let view = buffer.view();

    // write_pos = 8%8=0
    for lookahead in 0..4 {
        for offset in 0..4 {
            let expected = ((lookahead + offset) % 8) as f32;
            assert_eq!(
                view.read_lookahead(lookahead, offset),
                expected,
                "Mismatch at lookahead={}, offset={}",
                lookahead,
                offset
            );
        }
    }
}
