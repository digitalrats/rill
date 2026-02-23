//! # Общие DSP утилиты для Kama Audio
//!
//! Этот крейт предоставляет инфраструктурные компоненты для создания DSP-узлов:
//!
//! ## Основные компоненты
//!
//! - **Контекст выполнения** [`DspContext`] — содержит информацию о текущем состоянии обработки
//! - **Функциональные узлы** — конструкторы для быстрого создания узлов из замыканий
//! - **Макросы** — [`effect!`], [`filter!`], [`generator!`] для ещё более простого создания
//! - **Фильтры** — общие трейты и типы для фильтров
//!
//! ## Примеры
//!
//! ### Создание эффекта через макрос
//! ```
//! use kama_dsp_common::effect;
//!
//! effect!(Gain, |sample, _ctx| sample * 0.5);
//! ```
//!
//! ### Создание эффекта с состоянием
//! ```
//! use kama_dsp_common::effect_with_state;
//!
//! effect_with_state!(OnePole, 0.0, |sample, state, _ctx| {
//!     let alpha = 0.1;
//!     *state = *state + alpha * (sample - *state);
//!     *state
//! });
//! ```
//!
//! ### Создание узла вручную
//! ```
//! use kama_dsp_common::{stateless_fn_node, NodeCategory};
//!
//! let gain_node = stateless_fn_node(
//!     "Gain",
//!     NodeCategory::Effect,
//!     |sample, _ctx| sample * 0.5
//! );
//! ```
//!
//! [`DspContext`]: crate::context::DspContext
//! [`effect!`]: crate::effect
//! [`filter!`]: crate::filter
//! [`generator!`]: crate::generator

//! Общие утилиты для DSP-крейтов
//!
//! Предоставляет:
//! - `DspContext` — контекст выполнения DSP-узлов
//! - Конструкторы функциональных узлов (`stateless_fn_node`, `stateful_fn_node`)
//! - Макросы для упрощения создания эффектов

#![warn(missing_docs)]

mod context;
mod dummy;
pub mod filter;
mod fn_node;
mod macros;

pub use context::DspContext;
pub use filter::{Filter, FilterFactory, FilterType};
pub use fn_node::{block_fn_node, stateful_fn_node, stateless_fn_node};

// Реэкспорты для удобства
pub use kama_core_traits::{
    param::{ParamMetadata, ParamType, ParamValue},
    AudioError, AudioNode, NodeCategory, NodeMetadata, NodeTypeId,
};

// Реэкспортируем макросы
pub use crate::macros::*;
