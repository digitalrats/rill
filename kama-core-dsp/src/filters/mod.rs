//! # Базовые фильтры для обработки аудио
//!
//! Этот модуль предоставляет различные реализации фильтров, от простых
//! до сложных, для использования в аудио обработке. Все фильтры
//! параметризованы типом `T: AudioNum` и могут работать с `f32` или `f64`.
//!
//! ## Доступные фильтры
//!
//! | Фильтр | Характеристики | Применение |
//! |--------|---------------|------------|
//! | **[`Biquad`]** | Универсальный, 8 типов, 12dB/окт | Эквалайзеры, тональный контроль, кроссоверы |
//! | **[`OnePole`]** | Простой, быстрый, 6dB/окт | Сглаживание параметров, envelope followers |
//! | **[`StateVariableFilter`]** | 3 выхода одновременно, стабильный при резонансе | Аналоговая эмуляция, синтезаторы |
//! | **[`Butterworth`]** | Максимально плоский, нет пульсаций | Hi-Fi аудио, мастеринг, анализ |
//! | **[`ChebyshevI`]** | Пульсации в полосе пропускания, крутой спад | Эквалайзеры, крутые кроссоверы |
//! | **[`ChebyshevII`]** | Пульсации в полосе задерживания, плоская полоса | Anti-aliasing, децимация |
//! | **[`CombFilter`]** | Гребенчатый, металлический призвук | Реверберация, физическое моделирование |
//!
//! ## Общий интерфейс
//!
//! Все фильтры реализуют общий трейт [`Filter`], который предоставляет
//! единый способ управления параметрами:
//!
//! ```rust
//! use kama_core_dsp::filters::*;
//! use kama_core::AudioNum;
//!
//! fn process_filter<T: AudioNum>(filter: &mut dyn Filter<T>, input: T) -> T {
//!     filter.set_cutoff(1000.0);
//!     filter.set_q(0.707);
//!     filter.process_sample(input)
//! }
//! ```
//!
//! ## Примеры использования
//!
//! ### Создание фильтра нижних частот
//! ```
//! use kama_core_dsp::filters::{Biquad, FilterParams, FilterType};
//! use kama_core_dsp::Algorithm;
//!
//! let mut lowpass = Biquad::<f32>::new(FilterParams {
//!     filter_type: FilterType::LowPass,
//!     cutoff: 1000.0,
//!     q: 0.707,
//!     gain_db: 0.0,
//! });
//! lowpass.init(44100.0);
//!
//! let output = lowpass.process_sample(0.5);
//! ```
//!
//! ### Создание параметрического эквалайзера
//! ```
//! use kama_core_dsp::filters::{Biquad, FilterParams, FilterType};
//! use kama_core_dsp::Algorithm;
//!
//! let mut peak = Biquad::<f32>::new(FilterParams {
//!     filter_type: FilterType::Peak,
//!     cutoff: 1000.0,
//!     q: 2.0,
//!     gain_db: 6.0,  // +6dB подъём
//! });
//! peak.init(44100.0);
//! ```
//!
//! ### Фильтр Баттерворта высокого порядка
//! ```
//! use kama_core_dsp::filters::{Butterworth, FilterParams, FilterType};
//! use kama_core_dsp::Algorithm;
//!
//! let mut butter = Butterworth::<f32, 4>::lowpass(1000.0, 4);
//! butter.init(44100.0);
//! ```

mod biquad;
mod one_pole;
mod svf;
mod butterworth;
mod chebyshev;
mod comb;

pub use biquad::Biquad;
pub use one_pole::OnePole;
pub use svf::StateVariableFilter;
pub use butterworth::Butterworth;
pub use chebyshev::{ChebyshevI, ChebyshevII, ChebyshevParams};
pub use comb::CombFilter;

use kama_core::AudioNum;
use crate::algorithm::{Algorithm, ParameterizedAlgorithm, AlgorithmMetadata};

/// Общий тип параметров для всех фильтров
///
/// Содержит основные параметры, общие для большинства фильтров:
/// - `filter_type`: тип фильтра (нижних частот, верхних и т.д.)
/// - `cutoff`: частота среза или центральная частота в Hz
/// - `q`: добротность (резонанс), обычно от 0.1 до 20.0
/// - `gain_db`: усиление в dB (для peak и shelving фильтров)
#[derive(Debug, Clone)]
pub struct FilterParams {
    /// Тип фильтра
    pub filter_type: FilterType,
    
    /// Частота среза/центральная частота (Hz)
    ///
    /// Для LowPass/HighPass: частота среза -3dB
    /// Для BandPass/Notch: центральная частота
    /// Для Peak/Shelf: центральная частота
    pub cutoff: f32,
    
    /// Добротность (0.1 - 20.0)
    ///
    /// Определяет ширину полосы фильтра. Большие значения = более узкая полоса.
    /// Для LowPass/HighPass влияет на резонанс на частоте среза.
    pub q: f32,
    
    /// Усиление в dB (для peak/shelving фильтров)
    ///
    /// Положительные значения = усиление, отрицательные = ослабление.
    /// Обычно от -24dB до +24dB.
    pub gain_db: f32,
}

/// Тип фильтра
///
/// Определяет частотную характеристику фильтра.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterType {
    /// Фильтр нижних частот (Low-Pass)
    ///
    /// Пропускает частоты ниже частоты среза, ослабляет выше.
    /// Используется для сглаживания, удаления высокочастотного шума,
    /// в субтрактивном синтезе (VCF).
    LowPass,
    
    /// Фильтр верхних частот (High-Pass)
    ///
    /// Пропускает частоты выше частоты среза, ослабляет ниже.
    /// Используется для удаления постоянной составляющей, рокот-фильтр,
    /// выделения верхних гармоник.
    HighPass,
    
    /// Полосовой фильтр (Band-Pass)
    ///
    /// Пропускает только полосу вокруг центральной частоты.
    /// Используется для выделения полосы частот, формантных фильтров,
    /// анализа сигналов.
    BandPass,
    
    /// Режекторный фильтр (Notch)
    ///
    /// Подавляет узкую полосу вокруг центральной частоты.
    /// Используется для удаления сетевой помехи 50/60Hz,
    /// подавления обратной связи, создания эффекта флэнджер.
    Notch,
    
    /// Пиковый фильтр (Peak)
    ///
    /// Усиливает или ослабляет полосу вокруг центральной частоты.
    /// Основной элемент параметрического эквалайзера.
    Peak,
    
    /// Полочный фильтр низких частот (Low-Shelf)
    ///
    /// Усиливает или ослабляет все частоты ниже частоты среза.
    /// Используется для тонального контроля (басы), коррекции АЧХ.
    LowShelf,
    
    /// Полочный фильтр высоких частот (High-Shelf)
    ///
    /// Усиливает или ослабляет все частоты выше частоты среза.
    /// Используется для тонального контроля (высокие), уменьшения шипения.
    HighShelf,
    
    /// Всепропускающий фильтр (All-Pass)
    ///
    /// Меняет фазу сигнала, не меняя амплитуду.
    /// Используется в фазовращателях, для выравнивания групповой задержки,
    /// в эффектах флэнджер.
    AllPass,
}

impl FilterType {
    /// Получить строковое представление типа фильтра
    ///
    /// # Пример
    /// ```
    /// use kama_core_dsp::filters::FilterType;
    ///
    /// assert_eq!(FilterType::LowPass.as_str(), "lowpass");
    /// ```
    pub const fn as_str(&self) -> &'static str {
        match self {
            FilterType::LowPass => "lowpass",
            FilterType::HighPass => "highpass",
            FilterType::BandPass => "bandpass",
            FilterType::Notch => "notch",
            FilterType::Peak => "peak",
            FilterType::LowShelf => "lowshelf",
            FilterType::HighShelf => "highshelf",
            FilterType::AllPass => "allpass",
        }
    }
    
    /// Получить человеко-читаемое описание типа фильтра
    pub const fn description(&self) -> &'static str {
        match self {
            FilterType::LowPass => "Фильтр нижних частот (ФНЧ)",
            FilterType::HighPass => "Фильтр верхних частот (ФВЧ)",
            FilterType::BandPass => "Полосовой фильтр",
            FilterType::Notch => "Режекторный фильтр",
            FilterType::Peak => "Пиковый фильтр",
            FilterType::LowShelf => "Низкополочный фильтр",
            FilterType::HighShelf => "Высокополочный фильтр",
            FilterType::AllPass => "Всепропускающий фильтр",
        }
    }
    
    /// Получить рекомендации по применению
    pub const fn usage(&self) -> &'static str {
        match self {
            FilterType::LowPass => 
                "• Субтрактивный синтез (VCF)\n\
                 • Сглаживание сигналов\n\
                 • Anti-aliasing перед децимацией\n\
                 • Удаление высокочастотного шума",
            
            FilterType::HighPass => 
                "• Удаление постоянной составляющей (DC)\n\
                 • Фильтрация рокот (rumble filter)\n\
                 • Выделение верхних гармоник\n\
                 • Side-chain компрессия",
            
            FilterType::BandPass => 
                "• Выделение полосы частот\n\
                 • Формантные фильтры (вокал)\n\
                 • Анализ сигналов (спектр)\n\
                 • Эффект \"телефонного\" звука",
            
            FilterType::Notch => 
                "• Удаление сетевой помехи 50/60Hz\n\
                 • Подавление обратной связи (feedback)\n\
                 • Удаление резонансных частот\n\
                 • Создание эффекта \"флэнджер\"",
            
            FilterType::Peak => 
                "• Параметрический эквалайзер\n\
                 • Коррекция частотных характеристик\n\
                 • Выделение/подавление инструментов\n\
                 • Мастеринг",
            
            FilterType::LowShelf => 
                "• Тональный контроль (bass)\n\
                 • Коррекция АЧХ наушников\n\
                 • Усиление низких частот\n\
                 • RIAA коррекция",
            
            FilterType::HighShelf => 
                "• Тональный контроль (treble)\n\
                 • Коррекция высоких частот\n\
                 • Уменьшение шипения\n\
                 • Компенсация потерь в кабелях",
            
            FilterType::AllPass => 
                "• Фазовращатели (phaser)\n\
                 • Выравнивание групповой задержки\n\
                 • Создание эффектов (flanger)\n\
                 • Коррекция фазы в кроссоверах",
        }
    }
}

/// Общий трейт для всех фильтров
///
/// Предоставляет единый интерфейс для управления параметрами фильтра.
/// Все конкретные фильтры реализуют этот трейт через [`ParameterizedAlgorithm`]
/// с `Params = FilterParams`.
///
/// # Пример
/// ```
/// use kama_core_dsp::filters::*;
/// use kama_core::AudioNum;
///
/// fn process_filter<T: AudioNum>(filter: &mut dyn Filter<T>, input: T) -> T {
///     filter.set_cutoff(1000.0);
///     filter.set_q(0.707);
///     filter.process_sample(input)
/// }
/// ```
pub trait Filter<T: AudioNum>: ParameterizedAlgorithm<T, Params = FilterParams> {
    /// Установить частоту среза
    ///
    /// # Arguments
    /// * `cutoff` - частота в Hz (обычно 20..20000)
    fn set_cutoff(&mut self, cutoff: f32) {
        let mut params = self.params().clone();
        params.cutoff = cutoff;
        self.set_params(params);
    }
    
    /// Получить текущую частоту среза
    fn cutoff(&self) -> f32 {
        self.params().cutoff
    }
    
    /// Установить добротность (Q-фактор)
    ///
    /// # Arguments
    /// * `q` - добротность (обычно 0.1..20.0)
    fn set_q(&mut self, q: f32) {
        let mut params = self.params().clone();
        params.q = q;
        self.set_params(params);
    }
    
    /// Получить текущую добротность
    fn q(&self) -> f32 {
        self.params().q
    }
    
    /// Установить усиление (для peak/shelving фильтров)
    ///
    /// # Arguments
    /// * `gain` - усиление в dB (обычно -24..24)
    fn set_gain_db(&mut self, gain: f32) {
        let mut params = self.params().clone();
        params.gain_db = gain;
        self.set_params(params);
    }
    
    /// Получить текущее усиление в dB
    fn gain_db(&self) -> f32 {
        self.params().gain_db
    }
    
    /// Получить тип фильтра
    fn filter_type(&self) -> FilterType {
        self.params().filter_type
    }
}

// Blanket implementation для всех типов с Params = FilterParams
impl<T: AudioNum, F> Filter<T> for F where F: ParameterizedAlgorithm<T, Params = FilterParams> {}

// =============================================================================
// Сравнение фильтров
// =============================================================================

/// Сводка характеристик всех типов фильтров
#[derive(Debug)]
pub struct FilterComparison;

impl FilterComparison {
    /// Сравнение крутизны спада для разных реализаций
    ///
    /// # Пример
    /// ```
    /// use kama_core_dsp::filters::FilterComparison;
    ///
    /// println!("{}", FilterComparison::rolloff_comparison());
    /// ```
    pub fn rolloff_comparison() -> &'static str {
        "Крутизна спада (dB/октава):\n\
         ┌────────────────┬────────────┬──────────────┐\n\
         │ Фильтр         │ Порядок 2  │ Порядок 4    │\n\
         ├────────────────┼────────────┼──────────────┤\n\
         │ OnePole        │ 6 dB/окт   │ -            │\n\
         │ Biquad         │ 12 dB/окт  │ 24 dB/окт*   │\n\
         │ Butterworth    │ 12 dB/окт  │ 24 dB/окт    │\n\
         │ Chebyshev I    │ 12-18 dB/окт │ 24-36 dB/окт │\n\
         │ Chebyshev II   │ 12-18 dB/окт │ 24-36 dB/окт │\n\
         └────────────────┴────────────┴──────────────┘\n\
         * Biquad может быть каскадирован для более высоких порядков"
    }
    
    /// Рекомендации по выбору фильтра
    pub fn selection_guide() -> &'static str {
        "Как выбрать фильтр:\n\n\
         🎯 **Для Hi-Fi и прозрачной обработки**:\n\
         → Butterworth - максимально плоская характеристика\n\n\
         🎯 **Для синтезаторов и эффектов**:\n\
         → StateVariableFilter - аналоговое звучание, три выхода\n\
         → OnePole - простота и скорость\n\n\
         🎯 **Для эквалайзеров**:\n\
         → Biquad - универсальность, все типы\n\
         → ChebyshevI - более крутой спад\n\n\
         🎯 **Для anti-aliasing**:\n\
         → ChebyshevII - плоская полоса пропускания\n\
         → Butterworth - предсказуемое поведение\n\n\
         🎯 **Для реверберации**:\n\
         → CombFilter - гребенчатые структуры\n\
         → AllPass - диффузия"
    }
    
    /// Характеристики вычислительной сложности
    pub fn performance_guide() -> &'static str {
        "Производительность (относительная):\n\
         ⚡ **OnePole** - 1x (самый быстрый)\n\
         ⚡⚡ **Biquad** - 2x\n\
         ⚡⚡⚡ **StateVariableFilter** - 3x\n\
         ⚡⚡⚡ **CombFilter** - 3x\n\
         ⚡⚡⚡⚡ **Butterworth (4 order)** - 4x\n\
         ⚡⚡⚡⚡ **Chebyshev (4 order)** - 4x"
    }
}

// =============================================================================
// Примеры использования (doctests) с правильными импортами
// =============================================================================


#[cfg(doctest)]
mod examples {
    /// ```rust
    /// use kama_core_dsp::filters::*;
    /// use kama_core::AudioNum;
    /// use kama_core_dsp::Algorithm;
    /// use std::f32::consts::PI;
    ///
    /// // 1. Простой low-pass фильтр для сглаживания
    /// let mut smooth = OnePole::<f32>::new(FilterParams {
    ///     filter_type: FilterType::LowPass,
    ///     cutoff: 100.0,
    ///     q: 0.0,
    ///     gain_db: 0.0,
    /// });
    /// smooth.init(44100.0);
    ///
    /// // Сглаживаем резкие изменения
    /// let mut smoothed = 0.0;
    /// for _ in 0..1000 {
    ///     smoothed = smooth.process_sample(1.0);
    /// }
    /// // После 1000 итераций значение должно быть близко к 1.0
    /// # assert!((smoothed - 1.0).abs() < 0.1);
    ///
    /// // 2. Параметрический эквалайзер с Biquad
    /// let mut peq = Biquad::<f32>::new(FilterParams {
    ///     filter_type: FilterType::Peak,
    ///     cutoff: 1000.0,
    ///     q: 2.0,
    ///     gain_db: 6.0,
    /// });
    /// peq.init(44100.0);
    ///
    /// // Прогреваем фильтр
    /// for _ in 0..1000 {
    ///     let _ = peq.process_sample(0.0);
    /// }
    ///
    /// // Генерируем синусоиду на частоте фильтра
    /// let sample_rate = 44100.0;
    /// let frequency = 1000.0;
    /// let amplitude = 0.5;
    /// let phase_inc = 2.0 * PI * frequency / sample_rate;
    /// let mut phase = 0.0;
    ///
    /// let mut max_output = 0.0;
    /// for _ in 0..1000 {
    ///     let input = amplitude * phase.sin();
    ///     let output = peq.process_sample(input);
    ///     max_output = max_output.max(output.abs());
    ///     phase += phase_inc;
    ///     if phase > 2.0 * PI {
    ///         phase -= 2.0 * PI;
    ///     }
    /// }
    ///
    /// // Пиковый фильтр с +6dB должен усиливать сигнал на частоте фильтра
    /// // Используем допуск из-за численных ошибок
    /// let epsilon = 1e-4;
    /// # assert!(max_output + epsilon > amplitude, 
    /// #     "Max output ({:.6}) should be greater than or close to input amplitude ({:.6})", 
    /// #     max_output, amplitude);
    /// # assert!(max_output < 1.0, "Max output ({}) should be less than 1.0", max_output);
    ///
    /// // 3. Аналоговая эмуляция с SVF
    /// let mut svf = StateVariableFilter::<f32>::new(FilterParams {
    ///     filter_type: FilterType::LowPass,
    ///     cutoff: 1000.0,
    ///     q: 0.7,
    ///     gain_db: 0.0,
    /// });
    /// svf.init(44100.0);
    ///
    /// let input = 0.5;
    /// let lp = svf.process_sample(input);
    /// let hp = svf.highpass();
    /// let bp = svf.bandpass();
    /// ```
    ///
    /// ```rust
    /// // 4. Крутой фильтр для кроссовера (Chebyshev)
    /// use kama_core_dsp::filters::*;
    /// use kama_core_dsp::Algorithm;
    ///
    /// let mut xover = ChebyshevI::<f32, 4>::new(
    ///     FilterParams {
    ///         filter_type: FilterType::LowPass,
    ///         cutoff: 1000.0,
    ///         q: 0.0,
    ///         gain_db: 0.0,
    ///     },
    ///     4,
    ///     0.5
    /// );
    /// xover.init(44100.0);
    ///
    /// // 5. Hi-Fi фильтр (Butterworth)
    /// let mut hifi = Butterworth::<f32, 4>::lowpass(1000.0, 4);
    /// hifi.init(44100.0);
    /// ```
    fn _dummy() {}
}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_filter_type_descriptions() {
        assert_eq!(FilterType::LowPass.as_str(), "lowpass");
        assert!(!FilterType::LowPass.description().is_empty());
        assert!(!FilterType::LowPass.usage().is_empty());
    }
    
    #[test]
    fn test_comparison_guide() {
        assert!(!FilterComparison::rolloff_comparison().is_empty());
        assert!(!FilterComparison::selection_guide().is_empty());
        assert!(!FilterComparison::performance_guide().is_empty());
    }
    
    #[test]
    fn test_filter_params_clone() {
        let params = FilterParams {
            filter_type: FilterType::LowPass,
            cutoff: 1000.0,
            q: 0.707,
            gain_db: 0.0,
        };
        let params2 = params.clone();
        assert_eq!(params.cutoff, params2.cutoff);
    }
}