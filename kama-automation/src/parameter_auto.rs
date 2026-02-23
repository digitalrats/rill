//! # Автоматизированный параметр
//!
//! Обёртка над обычным параметром, которая позволяет автоматизировать его значение.
//! В отличие от [`Servo`](crate::Servo), который управляет параметром извне,
//! этот тип внедряет автоматизацию непосредственно в узел.
//!
//! ## Когда использовать
//!
//! Используйте `AutomatedParameter`, когда вы создаёте свой собственный узел
//! и хотите, чтобы один из его параметров мог автоматизироваться.
//!
//! ## Пример
//!
//! ```
//! use kama_automation::{AutomatedParameter, automaton::LfoAutomaton};
//!
//! struct MyOscillator {
//!     frequency: AutomatedParameter<LfoAutomaton>,
//!     amplitude: f32,
//! }
//! ```

// kama-automation/src/parameter_auto.rs
//! Автоматизированный параметр

use crate::automaton::Automaton;
use crate::context::AutomationContext;
use std::fmt;
use std::marker::PhantomData;
use std::time::Instant;

/// Параметр с автоматизацией (обобщённая версия)
pub struct AutomatedParameter<A: Automaton<Time = f64, Context = AutomationContext>> {
    value: f32,
    default: f32,
    min: Option<f32>,
    max: Option<f32>,
    automaton: Option<A>,
    automation_enabled: bool,
    last_update: Instant,
    state: A::State,
    _phantom: PhantomData<A>,
}

// Вспомогательная структура для тестового TimeProvider
struct DummyTimeProvider;

impl fmt::Debug for DummyTimeProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DummyTimeProvider").finish()
    }
}

impl kama_core::traits::time::Clock for DummyTimeProvider {
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

impl kama_core::traits::time::TimeProvider for DummyTimeProvider {
    fn bpm(&self) -> f64 {
        120.0
    }
    fn set_bpm(&self, _bpm: f64) {}
    fn tick_info(&self) -> kama_core::traits::time::TickInfo {
        kama_core::traits::time::TickInfo {
            bar: 0,
            beat: 0,
            sixteenth: 0,
            sample_pos: 0,
        }
    }
}

impl<A: Automaton<Time = f64, Context = AutomationContext>> AutomatedParameter<A> {
    /// Создать новый автоматизированный параметр.
    ///
    /// # Аргументы
    /// * `default` — значение по умолчанию
    /// * `automaton` — автомат, управляющий параметром
    ///
    /// Автоматизация включена по умолчанию.
    pub fn new(default: f32, automaton: A) -> Self {
        let state = automaton.initial_state();
        Self {
            value: default,
            default,
            min: None,
            max: None,
            automaton: Some(automaton),
            automation_enabled: true,
            last_update: Instant::now(),
            state,
            _phantom: PhantomData,
        }
    }

    /// Обновить значение параметра. Должен вызываться каждый семпл или блок.
    pub fn update(&mut self) -> f32 {
        if self.automation_enabled {
            if let Some(automaton) = &mut self.automaton {
                let elapsed = self.last_update.elapsed();
                let time = elapsed.as_secs_f64();

                // Создаём временный контекст
                use std::sync::Arc;

                let context = AutomationContext::new(Arc::new(DummyTimeProvider));
                let (new_state, _) =
                    automaton.step(time, &context, A::Action::default(), &self.state);
                self.state = new_state;
                self.value = automaton.extract_value(&self.state) as f32;

                if let Some(min) = self.min {
                    self.value = self.value.max(min);
                }
                if let Some(max) = self.max {
                    self.value = self.value.min(max);
                }
            }
        }

        self.last_update = Instant::now();
        self.value
    }

    /// Установить диапазон допустимых значений.
    pub fn set_range(&mut self, min: f32, max: f32) {
        self.min = Some(min);
        self.max = Some(max);
    }

    /// Включить автоматизацию.
    pub fn enable_automation(&mut self) {
        self.automation_enabled = true;
    }

    /// Выключить автоматизацию.
    pub fn disable_automation(&mut self) {
        self.automation_enabled = false;
    }

    /// Получить текущее значение.
    pub fn value(&self) -> f32 {
        self.value
    }
}
