//! Пример использования kama-dsp-common

use kama_dsp_common::*;

// Создаем эффекты с помощью макросов
effect!(Gain, |sample, _ctx| sample * 0.5);
effect!(Pan, |sample, ctx| {
    let pan = 0.5; // можно получать из параметров
    sample * (1.0 - pan)
});

effect_with_state!(OnePole, 0.0, |sample, state, _ctx| {
    let alpha = 0.1;
    *state = *state + alpha * (sample - *state);
    *state
});

// Ручное создание более сложного эффекта
fn create_delay() -> impl AudioNode {
    stateful_fn_node(
        "SimpleDelay",
        NodeCategory::Effect,
        vec![0.0; 44100], // буфер задержки
        |sample, buffer, ctx| {
            // TODO: реализация задержки
            sample
        },
    )
}

fn main() {
    println!("=== kama-dsp-common Example ===\n");

    // Создаем несколько эффектов
    let gain = Gain();
    let filter = OnePole();
    let delay = create_delay();

    println!("Created effects:");
    println!("  - {}", gain.metadata().name);
    println!("  - {}", filter.metadata().name);
    println!("  - {}", delay.metadata().name);

    println!("\n✅ kama-dsp-common работает!");
}
