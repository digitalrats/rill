//! Параметры для аудиоузлов
//!
//! ВНИМАНИЕ: Базовые типы параметров перенесены в `kama-core-traits`.
//! Этот модуль теперь только re-export из `kama-core-traits`.

pub use kama_core_traits::param::{
    ParamValue,
    ParamType,
    ParamRange,
    ParamMetadata,
};