//! Макросы для упрощения создания DSP-узлов

/// Создать простой эффект из функции
///
/// # Пример
/// ```
/// use kama_dsp_common::effect;
///
/// effect!(Gain, |sample, ctx| sample * 0.5);
/// ```
#[macro_export]
macro_rules! effect {
    ($name:ident, $process:expr) => {
        pub fn $name() -> impl $crate::AudioNode {
            $crate::stateless_fn_node(
                stringify!($name),
                $crate::NodeCategory::Effect,
                $process
            )
        }
    };
}

/// Создать эффект с состоянием
///
/// # Пример
/// ```
/// use kama_dsp_common::effect_with_state;
///
/// effect_with_state!(OnePole, 0.0, |sample, state, ctx| {
///     let alpha = 0.1;
///     *state = *state + alpha * (sample - *state);
///     *state
/// });
/// ```
#[macro_export]
macro_rules! effect_with_state {
    ($name:ident, $initial:expr, $process:expr) => {
        pub fn $name() -> impl $crate::AudioNode {
            $crate::stateful_fn_node(
                stringify!($name),
                $crate::NodeCategory::Effect,
                $initial,
                $process
            )
        }
    };
}

/// Создать фильтр из функции
#[macro_export]
macro_rules! filter {
    ($name:ident, $process:expr) => {
        pub fn $name() -> impl $crate::AudioNode {
            $crate::stateless_fn_node(
                stringify!($name),
                $crate::NodeCategory::Filter,
                $process
            )
        }
    };
}

/// Создать генератор (осциллятор) из функции
#[macro_export]
macro_rules! generator {
    ($name:ident, $process:expr) => {
        pub fn $name() -> impl $crate::AudioNode {
            $crate::stateless_fn_node(
                stringify!($name),
                $crate::NodeCategory::Generator,
                $process
            )
        }
    };
}