//! Пример использования векторных операций для DSP
//!
//! Демонстрирует базовые операции с векторами: сложение, умножение,
//! математические функции и работу с блоками данных.

use rill_core_dsp::vector::prelude::*;

fn main() {
    println!("=== Пример векторных операций ===");

    // 1. Базовые операции с векторами f32
    println!("\n1. Базовые операции (f32):");

    let data_a = [1.0f32, 2.0, 3.0, 4.0];
    let data_b = [5.0f32, 6.0, 7.0, 8.0];

    let vec_a = ScalarVector4::load(&data_a);
    let vec_b = ScalarVector4::load(&data_b);

    // Сложение
    let vec_sum = vec_a + vec_b;
    let mut result = [0.0f32; 4];
    vec_sum.store(&mut result);
    println!("   Сложение: {:?} + {:?} = {:?}", data_a, data_b, result);

    // Умножение
    let vec_mul = vec_a * vec_b;
    vec_mul.store(&mut result);
    println!("   Умножение: {:?} * {:?} = {:?}", data_a, data_b, result);

    // Комбинированное выражение
    let scalar = ScalarVector4::splat(2.0);
    let vec_expr = (vec_a + scalar) * vec_b;
    vec_expr.store(&mut result);
    println!("   (a + 2) * b = {:?}", result);

    // 2. Математические функции
    println!("\n2. Математические функции (f32):");

    let angles = [0.0f32, 0.5, 1.0, 1.5];
    let vec_angles = ScalarVector4::load(&angles);

    let vec_sin = vec_angles.sin();
    vec_sin.store(&mut result);
    println!("   sin({:?}) = {:?}", angles, result);

    let vec_cos = vec_angles.cos();
    vec_cos.store(&mut result);
    println!("   cos({:?}) = {:?}", angles, result);

    // 3. Операции со скалярами через splat
    println!("\n3. Операции со скалярами:");

    let gain = 0.5f32;
    let vec_gain = ScalarVector4::splat(gain);
    let vec_scaled = vec_a * vec_gain;
    vec_scaled.store(&mut result);
    println!("   {:?} * {} = {:?}", data_a, gain, result);

    // 4. Обработка блока данных (имитация DSP алгоритма)
    println!("\n4. Обработка блока данных:");

    // Исходный сигнал (синусоида)
    let mut signal = [0.0f32; 16];
    for i in 0..16 {
        signal[i] = (i as f32 * 0.1).sin();
    }
    println!("   Исходный сигнал: {:?}", signal);

    // Применяем gain к блоку по 4 семпла за раз
    let gain = 0.8f32;
    let gain_vec = ScalarVector4::splat(gain);
    let mut processed = [0.0f32; 16];

    for (idx, chunk) in signal.chunks_exact(4).enumerate() {
        let vec_chunk = ScalarVector4::load(chunk);
        let vec_processed = vec_chunk * gain_vec;
        // Сохраняем обратно
        let start = idx * 4;
        vec_processed.store(&mut processed[start..start + 4]);
    }

    println!("   После gain {}: {:?}", gain, processed);

    // 5. Использование разных размеров векторов (f64)
    println!("\n5. Векторы f64 (размер 2):");

    let data_a_f64 = [1.0f64, 2.0];
    let data_b_f64 = [3.0f64, 4.0];

    let vec_a_f64 = ScalarVector2::load(&data_a_f64);
    let vec_b_f64 = ScalarVector2::load(&data_b_f64);

    let vec_sum_f64 = vec_a_f64 + vec_b_f64;
    let mut result_f64 = [0.0f64; 2];
    vec_sum_f64.store(&mut result_f64);
    println!("   {:?} + {:?} = {:?}", data_a_f64, data_b_f64, result_f64);

    // 6. Демонстрация методов трейта Vector
    println!("\n6. Методы трейта Vector:");

    let vec = ScalarVector4::splat(3.14);
    let mut arr = [0.0f32; 4];
    vec.store(&mut arr);
    println!("   Вектор из одного значения 3.14: {:?}", arr);

    // Извлечение элемента
    println!("   extract(2) = {}", vec.extract(2));

    // Вставка элемента
    let new_vec = vec.insert(1, 99.0);
    new_vec.store(&mut arr);
    println!("   insert(1, 99.0) => {:?}", arr);

    // 7. Минимум, максимум, ограничение
    println!("\n7. Минимум, максимум, ограничение:");

    let vec1 = ScalarVector4::load(&[1.0, 5.0, 3.0, 7.0]);
    let vec2 = ScalarVector4::load(&[4.0, 2.0, 6.0, 0.0]);

    let vec_min = vec1.min(&vec2);
    vec_min.store(&mut arr);
    println!(
        "   min({:?}, {:?}) = {:?}",
        [1.0, 5.0, 3.0, 7.0],
        [4.0, 2.0, 6.0, 0.0],
        arr
    );

    let vec_max = vec1.max(&vec2);
    vec_max.store(&mut arr);
    println!("   max(...) = {:?}", arr);

    let min_vec = ScalarVector4::splat(2.0);
    let max_vec = ScalarVector4::splat(5.0);
    let vec_clamp = vec1.clamp(&min_vec, &max_vec);
    vec_clamp.store(&mut arr);
    println!("   clamp(2.0..5.0) = {:?}", arr);

    println!("\n=== Пример завершен ===");
}
