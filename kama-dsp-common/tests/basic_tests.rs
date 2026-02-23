//! Базовые тесты для kama-dsp-common

use kama_core_traits::{AudioNode, Clock, NodeCategory, TickInfo, TimeProvider};
use kama_dsp_common::*;
use std::sync::Arc;

// Заглушка для TimeProvider
#[derive(Debug)]
struct TestTimeProvider;

impl Clock for TestTimeProvider {
    fn sample_rate(&self) -> f64 {
        44100.0
    }
    fn position_samples(&self) -> u64 {
        0
    }
    fn advance(&self, _samples: u64) -> u64 {
        0
    }
    fn reset(&self) {}
}

impl TimeProvider for TestTimeProvider {
    fn bpm(&self) -> f64 {
        120.0
    }
    fn set_bpm(&self, _bpm: f64) {}
    fn tick_info(&self) -> TickInfo {
        TickInfo {
            bar: 0,
            beat: 0,
            sixteenth: 0,
            sample_pos: 0,
        }
    }
}

// Заглушка для BufferManager
fn create_test_buffers() -> kama_buffers::BufferManager {
    kama_buffers::BufferManager::new()
}

#[test]
fn test_stateless_fn_node() {
    // Создаем stateless узел (умножение на 2)
    let mut node = stateless_fn_node("Double", NodeCategory::Effect, |sample, _ctx| sample * 2.0);

    node.init(44100.0);

    let input = vec![1.0, 2.0, 3.0, 4.0];
    let mut output = vec![0.0; 4];

    let inputs = [input.as_slice()];
    let mut outputs = [output.as_mut_slice()];

    node.process(&inputs, &mut outputs).unwrap();

    assert_eq!(output, [2.0, 4.0, 6.0, 8.0]);
    assert_eq!(node.num_inputs(), 1);
    assert_eq!(node.num_outputs(), 1);
    assert_eq!(node.metadata().name, "Double");
}

#[test]
fn test_stateful_fn_node() {
    // Создаем stateful узел (интегратор/бегущее среднее)
    let mut node = stateful_fn_node(
        "RunningAverage",
        NodeCategory::Effect,
        0.0, // начальное состояние
        |sample, state, _ctx| {
            *state = *state * 0.9 + sample * 0.1;
            *state
        },
    );

    node.init(44100.0);

    let input = vec![1.0, 1.0, 1.0, 1.0];
    let mut output = vec![0.0; 4];

    let inputs = [input.as_slice()];
    let mut outputs = [output.as_mut_slice()];

    node.process(&inputs, &mut outputs).unwrap();

    // Проверяем, что состояние меняется
    assert!((output[0] - 0.1).abs() < 1e-6);
    assert!((output[1] - 0.19).abs() < 1e-6);
    assert!((output[2] - 0.271).abs() < 1e-6);
    assert!((output[3] - 0.3439).abs() < 1e-6);
}

#[test]
fn test_block_fn_node() {
    // Создаем блочный узел (усиление всего блока)
    let mut node = block_fn_node(
        "BlockGain",
        NodeCategory::Effect,
        1, // 1 вход
        1, // 1 выход
        |inputs: &[&[f32]], outputs: &mut [&mut [f32]], _ctx: &DspContext| {
            if inputs.is_empty() || outputs.is_empty() {
                return Ok(());
            }
            let input = inputs[0];
            let output = &mut outputs[0];
            for i in 0..input.len().min(output.len()) {
                output[i] = input[i] * 3.0;
            }
            Ok(())
        },
    );

    node.init(44100.0);

    let input = vec![1.0, 2.0, 3.0, 4.0];
    let mut output = vec![0.0; 4];

    let inputs = [input.as_slice()];
    let mut outputs = [output.as_mut_slice()];

    node.process(&inputs, &mut outputs).unwrap();

    assert_eq!(output, [3.0, 6.0, 9.0, 12.0]);
}

#[test]
fn test_multiple_inputs_outputs() {
    // Создаем узел с 2 входами и 2 выходами (стерео процессор)
    let mut node = block_fn_node(
        "StereoProcessor",
        NodeCategory::Effect,
        2, // 2 входа
        2, // 2 выхода
        |inputs: &[&[f32]], outputs: &mut [&mut [f32]], _ctx: &DspContext| {
            if inputs.len() < 2 || outputs.len() < 2 {
                return Ok(());
            }

            let left_in = inputs[0];
            let right_in = inputs[1];

            // Используем split_at_mut для безопасного получения двух мутабельных срезов
            let (left_out, right_out) = outputs.split_at_mut(1);
            let left_out = &mut left_out[0];
            let right_out = &mut right_out[0];

            let n = left_in.len().min(left_out.len()).min(right_out.len());

            for i in 0..n {
                left_out[i] = left_in[i] * 0.8 + right_in[i] * 0.2;
                right_out[i] = right_in[i] * 0.8 + left_in[i] * 0.2;
            }
            Ok(())
        },
    );

    node.init(44100.0);

    let left_in = vec![1.0, 2.0, 3.0, 4.0];
    let right_in = vec![0.5, 1.0, 1.5, 2.0];
    let mut left_out = vec![0.0; 4];
    let mut right_out = vec![0.0; 4];

    let inputs = [left_in.as_slice(), right_in.as_slice()];
    let mut outputs = [left_out.as_mut_slice(), right_out.as_mut_slice()];

    node.process(&inputs, &mut outputs).unwrap();

    // Используем допуск для сравнения чисел с плавающей точкой
    for i in 0..4 {
        assert!((left_out[i] - [0.9, 1.8, 2.7, 3.6][i]).abs() < 1e-6);
        assert!((right_out[i] - [0.6, 1.2, 1.8, 2.4][i]).abs() < 1e-6);
    }
}

#[test]
fn test_effect_macro() {
    // Используем макрос effect!
    effect!(TestGain, |sample, _ctx| sample * 0.5);

    let mut node = TestGain();
    node.init(44100.0);

    let input = vec![1.0, 2.0, 3.0, 4.0];
    let mut output = vec![0.0; 4];

    let inputs = [input.as_slice()];
    let mut outputs = [output.as_mut_slice()];

    node.process(&inputs, &mut outputs).unwrap();

    assert_eq!(output, [0.5, 1.0, 1.5, 2.0]);
    assert_eq!(node.metadata().category, NodeCategory::Effect);
}

#[test]
fn test_effect_with_state_macro() {
    // Используем макрос effect_with_state!
    effect_with_state!(TestFilter, 0.0, |sample, state, _ctx| {
        *state = *state * 0.8 + sample * 0.2;
        *state
    });

    let mut node = TestFilter();
    node.init(44100.0);

    let input = vec![1.0, 1.0, 1.0, 1.0];
    let mut output = vec![0.0; 4];

    let inputs = [input.as_slice()];
    let mut outputs = [output.as_mut_slice()];

    node.process(&inputs, &mut outputs).unwrap();

    assert!((output[0] - 0.2).abs() < 1e-6);
    assert!((output[1] - 0.36).abs() < 1e-6);
    assert!((output[2] - 0.488).abs() < 1e-6);
}

#[test]
fn test_filter_macro() {
    // Используем макрос filter!
    filter!(LowPass, |sample, ctx| {
        // Простой фильтр нижних частот
        let alpha = 0.1;
        sample * alpha + 0.0 // заглушка
    });

    let mut node = LowPass();
    node.init(44100.0);

    assert_eq!(node.metadata().category, NodeCategory::Filter);
}

#[test]
fn test_generator_macro() {
    // Используем макрос generator!
    generator!(SineOsc, |_sample, ctx| {
        // Заглушка для осциллятора
        (ctx.block_position as f32 * 0.1).sin()
    });

    let mut node = SineOsc();
    node.init(44100.0);

    assert_eq!(node.metadata().category, NodeCategory::Generator);
}

#[test]
fn test_dsp_context_creation() {
    let time = TestTimeProvider;
    let buffers = create_test_buffers();

    let ctx = DspContext::new(&time, 44100.0, 512, 0, &buffers);

    assert_eq!(ctx.sample_rate, 44100.0);
    assert_eq!(ctx.block_size, 512);
    assert_eq!(ctx.block_position, 0);
    assert!((ctx.seconds() - 0.0).abs() < 1e-6);
}

#[test]
fn test_dsp_context_with_user_data() {
    let time = TestTimeProvider;
    let buffers = create_test_buffers();

    struct UserData {
        value: i32,
    }

    let user_data = UserData { value: 42 };

    let ctx = DspContext::new(&time, 44100.0, 512, 0, &buffers).with_user_data(&user_data);

    assert!(ctx.user_data.is_some());
}

#[test]
fn test_node_reset() {
    // Создаем stateful узел и проверяем reset
    let mut node = stateful_fn_node(
        "Stateful",
        NodeCategory::Effect,
        0.0,
        |sample, state, _ctx| {
            *state += sample;
            *state
        },
    );

    node.init(44100.0);

    let input = vec![1.0, 2.0, 3.0];
    let mut output = vec![0.0; 3];

    let inputs = [input.as_slice()];
    let mut outputs = [output.as_mut_slice()];

    node.process(&inputs, &mut outputs).unwrap();
    assert_eq!(output, [1.0, 3.0, 6.0]);

    // Сброс должен обнулить состояние
    node.reset();

    // Теперь обрабатываем другие данные
    let input2 = vec![4.0, 5.0, 6.0];
    let mut output2 = vec![0.0; 3];
    let inputs2 = [input2.as_slice()];
    let mut outputs2 = [output2.as_mut_slice()];

    node.process(&inputs2, &mut outputs2).unwrap();

    // Если reset сработал правильно, то должно начаться с 0
    // 4 -> 4, 4+5=9, 9+6=15
    assert_eq!(output2, [4.0, 9.0, 15.0]);

    // Создаем новый узел для сравнения
    let mut node2 = stateful_fn_node(
        "Stateful2",
        NodeCategory::Effect,
        0.0,
        |sample, state, _ctx| {
            *state += sample;
            *state
        },
    );
    node2.init(44100.0);

    let mut output3 = vec![0.0; 3];
    let inputs3 = [input2.as_slice()];
    let mut outputs3 = [output3.as_mut_slice()];

    node2.process(&inputs3, &mut outputs3).unwrap();

    // Новый узел должен дать те же результаты
    assert_eq!(output3, [4.0, 9.0, 15.0]);
}
