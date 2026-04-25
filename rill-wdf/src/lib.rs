//! Wave Digital Filter (WDF) implementation for analog circuit emulation
//! 
//! References:
//! - A. Fettweis, "Wave Digital Filters: Theory and Practice" (1986)
//! - K. J. Werner et al., "An Improved and Generalized Diode Clipper Model for Wave Digital Filters" (2015)
//! - V. Välimäki et al., "Virtual Analog Effects" (2018)

use std::sync::Arc;
use serde::{Serialize, Deserialize};
use parking_lot::RwLock;
use nalgebra::{DMatrix, DVector};
use num_complex::Complex64;
use rill_core::{
    AudioNode, 
    AudioError,
    param::{ParamValue, ParamType},
    node::{NodeMetadata, NodeCategory},
};

// Re-export типов (убрали дублирующийся ParamType)
pub use rill_core::param::ParamValue as CoreParamValue;

// --- Основные типы WDF ---

/// Порты WDF (адаптеры)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PortType {
    Series,     // Последовательное соединение
    Parallel,   // Параллельное соединение
    Reflection, // Отражающий порт
}

/// Волновые переменные: a (падающая), b (отраженная)
#[derive(Debug, Clone, Copy)]
pub struct WaveVariables {
    pub a: f64, // Incident wave (падающая волна)
    pub b: f64, // Reflected wave (отраженная волна)
}

impl WaveVariables {
    pub fn new() -> Self {
        Self { a: 0.0, b: 0.0 }
    }
    
    /// Вычисление напряжения и тока из волновых переменных
    pub fn to_voltage_current(&self, port_resistance: f64) -> (f64, f64) {
        let v = (self.a + self.b) / 2.0;
        let i = (self.a - self.b) / (2.0 * port_resistance);
        (v, i)
    }
    
    /// Вычисление волновых переменных из напряжения и тока
    pub fn from_voltage_current(v: f64, i: f64, port_resistance: f64) -> Self {
        let a = v + port_resistance * i;
        let b = v - port_resistance * i;
        Self { a, b }
    }
}

/// Базовый элемент WDF
pub trait WdfElement: Send + Sync {
    /// Сопротивление порта
    fn port_resistance(&self) -> f64;
    
    /// Обработка падающей волны, возврат отраженной
    fn process_incident(&mut self, a: f64) -> f64;
    
    /// Обновление состояния элемента
    fn update_state(&mut self);
    
    /// Получение текущего напряжения
    fn voltage(&self) -> f64;
    
    /// Получение текущего тока
    fn current(&self) -> f64;
    
    /// Сброс состояния
    fn reset(&mut self);
}

// --- Базовые элементы схемы ---

/// Резистор
#[derive(Debug, Clone)]
pub struct Resistor {
    resistance: f64,
    port_resistance: f64,
    voltage: f64,
    current: f64,
}

impl Resistor {
    pub fn new(resistance: f64) -> Self {
        Self {
            resistance,
            port_resistance: resistance,
            voltage: 0.0,
            current: 0.0,
        }
    }
    
    pub fn resistance(&self) -> f64 {
        self.resistance
    }
}

impl WdfElement for Resistor {
    fn port_resistance(&self) -> f64 {
        self.port_resistance
    }
    
    fn process_incident(&mut self, _a: f64) -> f64 {
        // Для резистора: b = 0 (полное поглощение)
        0.0
    }
    
    fn update_state(&mut self) {
        // Для резистора v = i * R
        self.voltage = self.current * self.resistance;
    }
    
    fn voltage(&self) -> f64 {
        self.voltage
    }
    
    fn current(&self) -> f64 {
        self.current
    }
    
    fn reset(&mut self) {
        self.voltage = 0.0;
        self.current = 0.0;
    }
}

/// Конденсатор (ёмкость)
#[derive(Debug, Clone)]
pub struct Capacitor {
    capacitance: f64,
    sample_rate: f64,
    port_resistance: f64,
    voltage: f64,
    current: f64,
    state: f64, // Состояние интегратора
}

impl Capacitor {
    pub fn new(capacitance: f64, sample_rate: f64) -> Self {
        // Сопротивление порта для конденсатора: R = T/(2C)
        let t = 1.0 / sample_rate;
        let port_resistance = t / (2.0 * capacitance);
        
        Self {
            capacitance,
            sample_rate,
            port_resistance,
            voltage: 0.0,
            current: 0.0,
            state: 0.0,
        }
    }
    
    pub fn capacitance(&self) -> f64 {
        self.capacitance
    }
    
    pub fn set_capacitance(&mut self, capacitance: f64) {
        self.capacitance = capacitance;
        let t = 1.0 / self.sample_rate;
        self.port_resistance = t / (2.0 * capacitance);
    }
}

impl WdfElement for Capacitor {
    fn port_resistance(&self) -> f64 {
        self.port_resistance
    }
    
    fn process_incident(&mut self, a: f64) -> f64 {
        // Для конденсатора: b = state - a
        self.state - a
    }
    
    fn update_state(&mut self) {
        // Обновление состояния: state = -b
        self.state = -self.current * self.port_resistance;
        
        // Интегрирование тока для получения напряжения
        let t = 1.0 / self.sample_rate;
        self.voltage += self.current * t / self.capacitance;
    }
    
    fn voltage(&self) -> f64 {
        self.voltage
    }
    
    fn current(&self) -> f64 {
        self.current
    }
    
    fn reset(&mut self) {
        self.voltage = 0.0;
        self.current = 0.0;
        self.state = 0.0;
    }
}

/// Катушка индуктивности
#[derive(Debug, Clone)]
pub struct Inductor {
    inductance: f64,
    sample_rate: f64,
    port_resistance: f64,
    voltage: f64,
    current: f64,
    state: f64,
}

impl Inductor {
    pub fn new(inductance: f64, sample_rate: f64) -> Self {
        // Сопротивление порта для индуктивности: R = 2L/T
        let t = 1.0 / sample_rate;
        let port_resistance = 2.0 * inductance / t;
        
        Self {
            inductance,
            sample_rate,
            port_resistance,
            voltage: 0.0,
            current: 0.0,
            state: 0.0,
        }
    }
}

impl WdfElement for Inductor {
    fn port_resistance(&self) -> f64 {
        self.port_resistance
    }
    
    fn process_incident(&mut self, _a: f64) -> f64 {
        // Для индуктивности: b = -state
        -self.state
    }
    
    fn update_state(&mut self) {
        // Обновление состояния: state = b
        self.state = self.current * self.port_resistance;
        
        // Интегрирование напряжения для получения тока
        let t = 1.0 / self.sample_rate;
        self.current += self.voltage * t / self.inductance;
    }
    
    fn voltage(&self) -> f64 {
        self.voltage
    }
    
    fn current(&self) -> f64 {
        self.current
    }
    
    fn reset(&mut self) {
        self.voltage = 0.0;
        self.current = 0.0;
        self.state = 0.0;
    }
}

/// Диод (нелинейный элемент)
#[derive(Debug, Clone)]
pub struct Diode {
    saturation_current: f64,    // Is (ток насыщения)
    thermal_voltage: f64,       // Vt = kT/q (~25.85mV при 300K)
    ideality_factor: f64,       // n (обычно 1-2)
    port_resistance: f64,
    voltage: f64,
    current: f64,
    last_b: f64,
}

impl Diode {
    pub fn new(saturation_current: f64, ideality_factor: f64, temperature_k: f64) -> Self {
        // Тепловое напряжение: Vt = kT/q
        let k = 1.380649e-23;   // Постоянная Больцмана
        let q = 1.60217662e-19; // Заряд электрона
        let thermal_voltage = (k * temperature_k) / q;
        
        // Начальное сопротивление порта (приблизительное)
        let port_resistance = thermal_voltage / saturation_current;
        
        Self {
            saturation_current,
            thermal_voltage,
            ideality_factor,
            port_resistance,
            voltage: 0.0,
            current: 0.0,
            last_b: 0.0,
        }
    }
    
    pub fn saturation_current(&self) -> f64 {
        self.saturation_current
    }
    
    pub fn thermal_voltage(&self) -> f64 {
        self.thermal_voltage
    }
    
    /// Уравнение диода Шокли
    fn diode_equation(&self, v: f64) -> f64 {
        let vt = self.thermal_voltage * self.ideality_factor;
        self.saturation_current * ((v / vt).exp() - 1.0)
    }
    
    /// Производная уравнения диода
    fn diode_derivative(&self, v: f64) -> f64 {
        let vt = self.thermal_voltage * self.ideality_factor;
        self.saturation_current * (v / vt).exp() / vt
    }
    
    /// Решение методом Ньютона-Рафсона
    fn solve_newton(&self, a: f64, r: f64) -> f64 {
        let mut v = 0.0; // Начальное предположение
        let tolerance = 1e-9;
        
        for _ in 0..10 { // Максимум 10 итераций
            let i = self.diode_equation(v);
            let g = self.diode_derivative(v);
            
            // Уравнение для решения: f(v) = v + R*i(v) - a = 0
            let f = v + r * i - a;
            
            if f.abs() < tolerance {
                break;
            }
            
            // Производная: f'(v) = 1 + R*g(v)
            let df = 1.0 + r * g;
            
            // Шаг Ньютона: v_new = v - f/f'
            v -= f / df;
        }
        
        v
    }
}

impl WdfElement for Diode {
    fn port_resistance(&self) -> f64 {
        self.port_resistance
    }
    
    fn process_incident(&mut self, a: f64) -> f64 {
        // Решаем нелинейное уравнение диода
        let v = self.solve_newton(a, self.port_resistance);
        let i = self.diode_equation(v);
        
        // Обновляем состояние
        self.voltage = v;
        self.current = i;
        
        // Отраженная волна: b = 2v - a
        2.0 * v - a
    }
    
    fn update_state(&mut self) {
        // Для диода состояние обновляется в process_incident
        // Можно адаптировать port_resistance на основе производной
        let g = self.diode_derivative(self.voltage);
        if g > 0.0 {
            self.port_resistance = 1.0 / g;
        }
    }
    
    fn voltage(&self) -> f64 {
        self.voltage
    }
    
    fn current(&self) -> f64 {
        self.current
    }
    
    fn reset(&mut self) {
        self.voltage = 0.0;
        self.current = 0.0;
        self.last_b = 0.0;
    }
}

// --- Адаптеры соединений ---

/// Серийный адаптер (последовательное соединение)
#[derive(Clone)]
pub struct SeriesAdapter {
    elements: Vec<Arc<RwLock<dyn WdfElement>>>,
    port_resistance: f64,
}

impl std::fmt::Debug for SeriesAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeriesAdapter")
            .field("num_elements", &self.elements.len())
            .field("port_resistance", &self.port_resistance)
            .finish()
    }
}

impl SeriesAdapter {
    pub fn new(elements: Vec<Arc<RwLock<dyn WdfElement>>>) -> Self {
        // Общее сопротивление порта: сумма сопротивлений
        let port_resistance: f64 = elements.iter()
            .map(|e| e.read().port_resistance())
            .sum();
        
        Self {
            elements,
            port_resistance,
        }
    }
}

impl WdfElement for SeriesAdapter {
    fn port_resistance(&self) -> f64 {
        self.port_resistance
    }
    
    fn process_incident(&mut self, a: f64) -> f64 {
        // Распределяем падающую волну между элементами
        let total_r = self.port_resistance;
        let mut b_total = 0.0;
        
        for element in &self.elements {
            let r_i = element.read().port_resistance();
            let a_i = a * (r_i / total_r);
            
            let b_i = element.write().process_incident(a_i);
            b_total += b_i * (r_i / total_r);
        }
        
        b_total
    }
    
    fn update_state(&mut self) {
        for element in &self.elements {
            element.write().update_state();
        }
    }
    
    fn voltage(&self) -> f64 {
        // Напряжение на серийном адаптере: сумма напряжений
        self.elements.iter()
            .map(|e| e.read().voltage())
            .sum()
    }
    
    fn current(&self) -> f64 {
        // Ток одинаков для всех элементов в серии
        if let Some(first) = self.elements.first() {
            first.read().current()
        } else {
            0.0
        }
    }
    
    fn reset(&mut self) {
        for element in &self.elements {
            element.write().reset();
        }
    }
}

/// Параллельный адаптер
#[derive(Clone)]
pub struct ParallelAdapter {
    elements: Vec<Arc<RwLock<dyn WdfElement>>>,
    port_resistance: f64,
}

impl std::fmt::Debug for ParallelAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParallelAdapter")
            .field("num_elements", &self.elements.len())
            .field("port_resistance", &self.port_resistance)
            .finish()
    }
}

impl ParallelAdapter {
    pub fn new(elements: Vec<Arc<RwLock<dyn WdfElement>>>) -> Self {
        // Общее сопротивление порта: 1 / sum(1/Ri)
        let inv_port_resistance: f64 = elements.iter()
            .map(|e| 1.0 / e.read().port_resistance())
            .sum();
        
        let port_resistance = 1.0 / inv_port_resistance;
        
        Self {
            elements,
            port_resistance,
        }
    }
}

impl WdfElement for ParallelAdapter {
    fn port_resistance(&self) -> f64 {
        self.port_resistance
    }
    
    fn process_incident(&mut self, a: f64) -> f64 {
        // Для параллельного адаптера: b_i одинаковы для всех элементов
        // Вычисляем общее b через адаптерные уравнения
        
        let total_g: f64 = self.elements.iter()
            .map(|e| 1.0 / e.read().port_resistance())
            .sum();
        
        let alpha: Vec<f64> = self.elements.iter()
            .map(|e| {
                let g_i = 1.0 / e.read().port_resistance();
                2.0 * g_i / total_g
            })
            .collect();
        
        // Вычисляем сумму alpha_i * b_i
        let mut sum_alpha_b = 0.0;
        for (i, element) in self.elements.iter().enumerate() {
            let b_i = element.write().process_incident(a);
            sum_alpha_b += alpha[i] * b_i;
        }
        
        // Общее b
        let b_total = sum_alpha_b - a;
        b_total
    }
    
    fn update_state(&mut self) {
        for element in &self.elements {
            element.write().update_state();
        }
    }
    
    fn voltage(&self) -> f64 {
        // Напряжение одинаково для всех элементов в параллели
        if let Some(first) = self.elements.first() {
            first.read().voltage()
        } else {
            0.0
        }
    }
    
    fn current(&self) -> f64 {
        // Общий ток: сумма токов
        self.elements.iter()
            .map(|e| e.read().current())
            .sum()
    }
    
    fn reset(&mut self) {
        for element in &self.elements {
            element.write().reset();
        }
    }
}

// --- Модели классических аналоговых устройств ---

/// Moog ladder filter (на основе модели Huovilainen)
#[derive(Clone)]
pub struct MoogLadderFilter {
    pub sample_rate: f64,
    pub cutoff: f64,
    pub resonance: f64,
    pub drive: f64,
    
    // 4 ступени ladder
    stages: [Capacitor; 4],
    feedback_path: f64,
    
    // Состояния
    input_voltage: f64,
    output_voltage: f64,
    stage_outputs: [f64; 4],
    
    resistors: Vec<Resistor>,
    adapters: Vec<SeriesAdapter>,
}

impl MoogLadderFilter {
    pub fn new(sample_rate: f64, cutoff: f64, resonance: f64) -> Self {
        // Вычисляем компоненты для каждой ступени
        let r = 1000.0; // Базовое сопротивление
        let c = 1.0 / (2.0 * std::f64::consts::PI * cutoff * r);
        
        let stages = [
            Capacitor::new(c, sample_rate),
            Capacitor::new(c, sample_rate),
            Capacitor::new(c, sample_rate),
            Capacitor::new(c, sample_rate),
        ];
        
        let mut filter = Self {
            sample_rate,
            cutoff,
            resonance: resonance.clamp(0.0, 0.99),
            drive: 1.0,
            stages,
            feedback_path: 0.0,
            input_voltage: 0.0,
            output_voltage: 0.0,
            stage_outputs: [0.0; 4],
            resistors: Vec::new(),
            adapters: Vec::new(),
        };
        
        filter.update_coefficients();
        filter
    }
    
    pub fn set_cutoff(&mut self, cutoff: f64) {
        self.cutoff = cutoff.max(20.0).min(self.sample_rate / 2.0);
        self.update_coefficients();
    }
    
    pub fn set_resonance(&mut self, resonance: f64) {
        self.resonance = resonance.clamp(0.0, 0.99);
        self.feedback_path = 4.0 * self.resonance;
    }
    
    pub fn set_drive(&mut self, drive: f64) {
        self.drive = drive.max(0.1).min(10.0);
    }
    
    fn update_coefficients(&mut self) {
        let r = 1000.0;
        let c = 1.0 / (2.0 * std::f64::consts::PI * self.cutoff * r);
        
        for stage in &mut self.stages {
            stage.set_capacitance(c);
        }
        
        self.feedback_path = 4.0 * self.resonance;
    }
    
    pub fn process(&mut self, input: f64) -> f64 {
        // Применяем overdrive к входу
        let input_driven = input * self.drive;
        let input_tanh = input_driven.tanh(); // Soft clipping
        
        // Feedback с выхода
        let feedback = self.output_voltage * self.feedback_path;
        
        // Вход с feedback
        self.input_voltage = input_tanh - feedback;
        
        // Обрабатываем через 4 ступени
        let mut signal = self.input_voltage;
        
        for i in 0..4 {
            // Упрощенная модель ladder (в реальности нужна полная WDF схема)
            let stage = &mut self.stages[i];
            
            // Простая RC фильтрация (заменяется на WDF)
            let alpha = 1.0 / (1.0 + stage.port_resistance() * 1000.0);
            self.stage_outputs[i] = alpha * signal + (1.0 - alpha) * self.stage_outputs[i];
            
            signal = self.stage_outputs[i];
        }
        
        self.output_voltage = signal;
        self.output_voltage
    }
}

/// Модель операционного усилителя (Op-Amp)
#[derive(Debug, Clone)]
pub struct OperationalAmplifier {
    gain: f64,
    slew_rate: f64,     // V/μs
    bandwidth: f64,     // GBW
    voltage_rails: (f64, f64), // +/- напряжение питания
    output_voltage: f64,
    internal_state: f64,
}

impl OperationalAmplifier {
    pub fn new(gain: f64, slew_rate: f64, bandwidth: f64) -> Self {
        Self {
            gain,
            slew_rate: slew_rate * 1e6, // Convert to V/s
            bandwidth,
            voltage_rails: (-15.0, 15.0), // Стандартные ±15V
            output_voltage: 0.0,
            internal_state: 0.0,
        }
    }
    
    pub fn process(&mut self, input: f64, dt: f64) -> f64 {
        // Идеальное усиление
        let ideal_output = input * self.gain;
        
        // Ограничение slew rate
        let max_change = self.slew_rate * dt;
        let output_change = ideal_output - self.internal_state;
        let limited_change = output_change.clamp(-max_change, max_change);
        
        // Обновляем внутреннее состояние
        self.internal_state += limited_change;
        
        // Ограничение по напряжению питания
        self.output_voltage = self.internal_state.clamp(
            self.voltage_rails.0,
            self.voltage_rails.1,
        );
        
        // Полюсная характеристика (однополюсная модель)
        let pole_frequency = self.bandwidth / self.gain;
        let alpha = 1.0 / (1.0 + 2.0 * std::f64::consts::PI * pole_frequency * dt);
        
        self.output_voltage = alpha * self.output_voltage + (1.0 - alpha) * ideal_output;
        
        self.output_voltage
    }
}

/// Модель кассетного магнитофона (Sony TC-260 style)
#[derive(Debug, Clone)]
pub struct CassetteDeckModel {
    pub sample_rate: f64,
    
    // Аналоговые компоненты
    input_amp: OperationalAmplifier,
    bias_oscillator: f64,        // High frequency bias
    record_head: Inductor,       // Головка записи
    playback_head: Inductor,     // Головка воспроизведения
    eq_filters: [Capacitor; 2],  // EQ коррекция
    output_amp: OperationalAmplifier,
    
    // Параметры ленты
    pub tape_speed: f64,            // см/сек
    tape_width: f64,            // мм
    pub bias_level: f64,            // Уровень подмагничивания
    pub noise_floor: f64,           // Уровень шума ленты
    
    // Нелинейности
    hysteresis: f64,            // Гистерезис
    saturation: f64,            // Насыщение
    print_through: f64,         // Просачивание
    pub wow_flutter: f64,           // Wow & flutter
    
    // Состояния
    tape_position: f64,
    wow_phase: f64,
    flutter_phase: f64,
}

impl CassetteDeckModel {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            
            input_amp: OperationalAmplifier::new(10.0, 0.5, 1e6),
            bias_oscillator: 100_000.0, // 100 kHz bias
            record_head: Inductor::new(100e-6, sample_rate), // 100 μH
            playback_head: Inductor::new(50e-6, sample_rate), // 50 μH
            eq_filters: [
                Capacitor::new(100e-9, sample_rate), // High frequency boost
                Capacitor::new(1e-6, sample_rate),   // Low frequency roll-off
            ],
            output_amp: OperationalAmplifier::new(5.0, 0.5, 1e6),
            
            tape_speed: 4.76,    // см/сек (стандарт для кассет)
            tape_width: 3.81,    // мм
            bias_level: 0.8,
            noise_floor: 1e-4,
            
            hysteresis: 0.1,
            saturation: 0.9,
            print_through: 0.01,
            wow_flutter: 0.002,
            
            tape_position: 0.0,
            wow_phase: 0.0,
            flutter_phase: 0.0,
        }
    }
    
    pub fn set_tape_speed(&mut self, speed_cm_per_sec: f64) {
        self.tape_speed = speed_cm_per_sec.clamp(1.19, 19.05); // От 1.19 до 19.05 см/сек
    }
    
    pub fn set_bias_level(&mut self, bias: f64) {
        self.bias_level = bias.clamp(0.0, 1.0);
    }
    
    fn tape_nonlinearity(&self, signal: f64) -> f64 {
        // Модель насыщения магнитной ленты (tanh-like)
        let saturated = signal.tanh() * self.saturation;
        
        // Добавляем гистерезис (упрощённо)
        let hysteresis_effect = self.hysteresis * signal.signum() * 0.01;
        
        saturated + hysteresis_effect
    }
    
    fn wow_and_flutter(&mut self, dt: f64) -> f64 {
        // Wow: низкочастотные изменения скорости (0.5-6 Hz)
        let wow_freq = 2.0; // Hz
        self.wow_phase += 2.0 * std::f64::consts::PI * wow_freq * dt;
        let wow = 0.01 * self.wow_flutter * self.wow_phase.sin();
        
        // Flutter: высокочастотные изменения (10-100 Hz)
        let flutter_freq = 30.0; // Hz
        self.flutter_phase += 2.0 * std::f64::consts::PI * flutter_freq * dt;
        let flutter = 0.005 * self.wow_flutter * self.flutter_phase.sin();
        
        wow + flutter
    }
    
    fn tape_noise(&self) -> f64 {
        // Шум ленты (розовый шум в основном)
        let white_noise = (rand::random::<f64>() - 0.5) * 2.0;
        let pink_noise = white_noise * self.noise_floor;
        
        // Добавляем щелчки и потрескивания (rare)
        let click_probability = 0.0001;
        let click = if rand::random::<f64>() < click_probability {
            (rand::random::<f64>() - 0.5) * 0.1
        } else {
            0.0
        };
        
        pink_noise + click
    }
    
    pub fn process_record(&mut self, input: f64) -> f64 {
        let dt = 1.0 / self.sample_rate;
        
        // Входной усилитель
        let amplified = self.input_amp.process(input, dt);
        
        // Добавляем высокочастотное подмагничивание
        let bias_phase = 2.0 * std::f64::consts::PI * self.bias_oscillator * dt;
        let bias_signal = self.bias_level * bias_phase.sin();
        
        // Суммируем сигнал и подмагничивание
        let record_signal = amplified + bias_signal;
        
        // Нелинейность ленты
        let recorded = self.tape_nonlinearity(record_signal);
        
        // Модель головки записи (упрощённо)
        let _head_current = recorded / self.record_head.port_resistance();
        
        // Обновляем позицию ленты
        self.tape_position += self.tape_speed * dt;
        
        recorded
    }
    
    pub fn process_playback(&mut self, recorded_signal: f64) -> f64 {
        let dt = 1.0 / self.sample_rate;
        
        // Эффекты движения ленты
        let speed_variation = 1.0 + self.wow_and_flutter(dt);
        
        // Модель головки воспроизведения
        let playback_voltage = recorded_signal * speed_variation;
        
        // EQ коррекция (подъём высоких частот)
        let mut eq_signal = playback_voltage;
        for filter in &mut self.eq_filters {
            // Упрощенная RC фильтрация
            let alpha = 1.0 / (1.0 + filter.port_resistance() * 1000.0);
            eq_signal = alpha * eq_signal;
        }
        
        // Эффект просачивания (print-through)
        let print_through_signal = self.print_through * playback_voltage;
        
        // Шум ленты
        let noise = self.tape_noise();
        
        // Выходной усилитель
        let final_signal = eq_signal + print_through_signal + noise;
        let output = self.output_amp.process(final_signal, dt);
        
        output
    }
    
    pub fn process(&mut self, input: f64) -> f64 {
        let recorded = self.process_record(input);
        let playback = self.process_playback(recorded);
        playback
    }
}

// --- AudioNode реализации ---

pub struct WdfMoogFilterNode {
    filter: MoogLadderFilter,
    sample_rate: f32,
    temp_buffer: Vec<f64>,
}

impl WdfMoogFilterNode {
    pub fn new(sample_rate: f32, cutoff: f32, resonance: f32) -> Self {
        Self {
            filter: MoogLadderFilter::new(
                sample_rate as f64,
                cutoff as f64,
                resonance as f64,
            ),
            sample_rate,
            temp_buffer: Vec::new(),
        }
    }
    
    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.filter.set_cutoff(cutoff as f64);
    }
    
    pub fn set_resonance(&mut self, resonance: f32) {
        self.filter.set_resonance(resonance as f64);
    }
    
    pub fn set_drive(&mut self, drive: f32) {
        self.filter.set_drive(drive as f64);
    }
}

impl AudioNode for WdfMoogFilterNode {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }
        
        let buffer_size = outputs[0].len();
        
        // Подготавливаем временный буфер
        if self.temp_buffer.len() < buffer_size {
            self.temp_buffer.resize(buffer_size, 0.0);
        }
        
        let input = inputs[0];
        let output = &mut outputs[0];
        
        // Обрабатываем каждый семпл
        for i in 0..buffer_size {
            self.temp_buffer[i] = self.filter.process(input[i] as f64);
            output[i] = self.temp_buffer[i] as f32;
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "cutoff" => Some(ParamValue::Float(self.filter.cutoff as f32)),
            "resonance" => Some(ParamValue::Float(self.filter.resonance as f32)),
            "drive" => Some(ParamValue::Float(self.filter.drive as f32)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("cutoff", ParamValue::Float(v)) => {
                self.set_cutoff(v);
                Ok(())
            }
            ("resonance", ParamValue::Float(v)) => {
                self.set_resonance(v);
                Ok(())
            }
            ("drive", ParamValue::Float(v)) => {
                self.set_drive(v);
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }
    
    fn reset(&mut self) {
        self.temp_buffer.clear();
    }
    
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Moog Ladder Filter".to_string(),
            category: NodeCategory::Filter,
            description: "Analog-style ladder filter using WDF modeling".to_string(),
            author: "Rill WDF".to_string(),
            version: "1.0".to_string(),
            parameters: vec![
                rill_core::node::ParamMetadata {
                    name: "cutoff".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1000.0),
                    min: Some(20.0),
                    max: Some(20000.0),
                    step: Some(1.0),
                    unit: Some("Hz".to_string()),
                    choices: None,
                },
                rill_core::node::ParamMetadata {
                    name: "resonance".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.5),
                    min: Some(0.0),
                    max: Some(0.99),
                    step: Some(0.01),
                    unit: Some("Q".to_string()),
                    choices: None,
                },
                rill_core::node::ParamMetadata {
                    name: "drive".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    min: Some(0.1),
                    max: Some(10.0),
                    step: Some(0.1),
                    unit: Some("gain".to_string()),
                    choices: None,
                },
            ],
        }
    }
}

pub struct CassetteDeckNode {
    deck: CassetteDeckModel,
    sample_rate: f32,
    temp_buffer: Vec<f64>,
}

impl CassetteDeckNode {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            deck: CassetteDeckModel::new(sample_rate as f64),
            sample_rate,
            temp_buffer: Vec::new(),
        }
    }
    
    pub fn set_tape_speed(&mut self, speed: f32) {
        self.deck.set_tape_speed(speed as f64);
    }
    
    pub fn set_bias_level(&mut self, bias: f32) {
        self.deck.set_bias_level(bias as f64);
    }
}

impl AudioNode for CassetteDeckNode {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }
        
        let buffer_size = outputs[0].len();
        
        if self.temp_buffer.len() < buffer_size {
            self.temp_buffer.resize(buffer_size, 0.0);
        }
        
        let input = inputs[0];
        let output = &mut outputs[0];
        
        for i in 0..buffer_size {
            self.temp_buffer[i] = self.deck.process(input[i] as f64);
            output[i] = self.temp_buffer[i] as f32;
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "tape_speed" => Some(ParamValue::Float(self.deck.tape_speed as f32)),
            "bias_level" => Some(ParamValue::Float(self.deck.bias_level as f32)),
            "noise_floor" => Some(ParamValue::Float(self.deck.noise_floor as f32)),
            "wow_flutter" => Some(ParamValue::Float(self.deck.wow_flutter as f32)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("tape_speed", ParamValue::Float(v)) => {
                self.set_tape_speed(v);
                Ok(())
            }
            ("bias_level", ParamValue::Float(v)) => {
                self.set_bias_level(v);
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }
    
    fn reset(&mut self) {
        self.temp_buffer.clear();
    }
    
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Cassette Deck".to_string(),
            category: NodeCategory::Effect,
            description: "Sony TC-260 style cassette deck emulation".to_string(),
            author: "Rill WDF".to_string(),
            version: "1.0".to_string(),
            parameters: vec![
                rill_core::node::ParamMetadata {
                    name: "tape_speed".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(4.76),
                    min: Some(1.19),
                    max: Some(19.05),
                    step: Some(0.1),
                    unit: Some("cm/s".to_string()),
                    choices: Some(vec![
                        ("1.19 cm/s".to_string(), 1.19),
                        ("2.38 cm/s".to_string(), 2.38),
                        ("4.76 cm/s".to_string(), 4.76),
                        ("9.52 cm/s".to_string(), 9.52),
                        ("19.05 cm/s".to_string(), 19.05),
                    ]),
                },
                rill_core::node::ParamMetadata {
                    name: "bias_level".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.8),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("level".to_string()),
                    choices: None,
                },
                rill_core::node::ParamMetadata {
                    name: "wow_flutter".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.002),
                    min: Some(0.0),
                    max: Some(0.01),
                    step: Some(0.0001),
                    unit: Some("amount".to_string()),
                    choices: None,
                },
            ],
        }
    }
}

// --- Утилиты для анализа схем ---

pub mod analysis {
    use super::*;
    
    /// Анализ частотной характеристики
    pub fn frequency_response(
        elements: &[Arc<RwLock<dyn WdfElement>>],
        frequencies: &[f64],
        sample_rate: f64,
    ) -> Vec<(f64, Complex64)> {
        let mut response = Vec::new();
        
        for &freq in frequencies {
            // Создаем тестовый сигнал на этой частоте
            let omega = 2.0 * std::f64::consts::PI * freq;
            let _test_signal = Complex64::new(0.0, omega / sample_rate).exp();
            
            // Пропускаем через систему (упрощённо)
            // В реальности нужна полная симуляция
            let mut output = Complex64::new(1.0, 0.0);
            
            for element in elements {
                // Упрощенная передаточная функция
                let r = element.read().port_resistance();
                let h = 1.0 / (1.0 + Complex64::new(0.0, omega * r));
                output = output * h;
            }
            
            response.push((freq, output));
        }
        
        response
    }
    
    /// Анализ искажений (THD)
    pub fn analyze_distortion(
        element: &mut dyn WdfElement,
        frequency: f64,
        amplitude: f64,
        sample_rate: f64,
        num_cycles: usize,
    ) -> f64 {
        let num_samples = (sample_rate / frequency * num_cycles as f64) as usize;
        let mut signal = Vec::with_capacity(num_samples);
        let mut output = Vec::with_capacity(num_samples);
        
        // Генерируем чистый синус
        for i in 0..num_samples {
            let t = i as f64 / sample_rate;
            let sample = amplitude * (2.0 * std::f64::consts::PI * frequency * t).sin();
            signal.push(sample);
        }
        
        // Пропускаем через элемент
        for &sample in &signal {
            let a = sample;
            let b = element.process_incident(a);
            output.push(b);
            element.update_state();
        }
        
        // Анализ FFT для вычисления THD
        // (упрощённая реализация)
        let fundamental_amplitude = amplitude;
        
        let harmonic_amplitude = output.iter()
            .map(|&x| x.abs())
            .fold(0.0_f64, |a, b| a.max(b)) - fundamental_amplitude;
        
        (harmonic_amplitude / fundamental_amplitude).abs()
    }
}

// --- Интеграция с lo-fi системой ---

#[cfg(feature = "lofi")]
pub mod lofi_integration {
    use super::*;
    use rill_lofi::{LofiProcessor, ClassicSystem};
    
    /// Комбинированная система: WDF аналоговая модель + lo-fi цифровая эмуляция
    pub struct VintageAnalogSystem {
        wdf_filter: WdfMoogFilterNode,
        lofi_processor: LofiProcessor,
        sample_rate: f32,
        analog_dry_wet: f32,
        digital_dry_wet: f32,
    }
    
    impl VintageAnalogSystem {
        pub fn new(sample_rate: f32) -> Self {
            let wdf_filter = WdfMoogFilterNode::new(sample_rate, 1000.0, 0.5);
            
            let lofi_config = rill_lofi::LofiConfig::for_system(ClassicSystem::AkaiS900);
            let lofi_processor = LofiProcessor::new(lofi_config);
            
            Self {
                wdf_filter,
                lofi_processor,
                sample_rate,
                analog_dry_wet: 0.7,
                digital_dry_wet: 0.3,
            }
        }
        
        pub fn process(&mut self, input: f32) -> f32 {
            // Аналоговая обработка (WDF)
            let analog_out = {
                let input_slice = [input];
                let mut output_slice = [0.0f32];
                let inputs = [&input_slice[..]];
                let mut outputs = [&mut output_slice[..]];
                
                self.wdf_filter.process(&inputs, &mut outputs).ok();
                output_slice[0]
            };
            
            // Цифровая lo-fi обработка
            let digital_out = {
                let input_slice = [input];
                let mut output_slice = [0.0f32];
                let inputs = [&input_slice[..]];
                let mut outputs = [&mut output_slice[..]];
                
                self.lofi_processor.process(&inputs, &mut outputs).ok();
                output_slice[0]
            };
            
            // Смешивание
            analog_out * self.analog_dry_wet + digital_out * self.digital_dry_wet
        }
    }
}

// --- Тесты ---

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_resistor_wdf() {
        let mut resistor = Resistor::new(1000.0);
        
        assert_eq!(resistor.port_resistance(), 1000.0);
        
        let a = 1.0;
        let b = resistor.process_incident(a);
        
        // Для резистора b должно быть 0 (полное поглощение)
        assert!((b - 0.0).abs() < 1e-10);
    }
    
    #[test]
    fn test_capacitor_wdf() {
        let sample_rate = 44100.0;
        let capacitance = 1e-6; // 1 μF
        let capacitor = Capacitor::new(capacitance, sample_rate);
        
        let expected_r = 1.0 / (sample_rate * 2.0 * capacitance);
        assert!((capacitor.port_resistance() - expected_r).abs() < 1e-10);
    }
    
    #[test]
    fn test_moog_filter() {
        let sample_rate = 44100.0;
        let mut filter = MoogLadderFilter::new(sample_rate, 1000.0, 0.5);
        
        // Тестовый сигнал
        let test_freq = 440.0;
        let num_samples = 4410; // 0.1 секунда
        
        let mut output_sum = 0.0;
        
        for i in 0..num_samples {
            let t = i as f64 / sample_rate;
            let input = (2.0 * std::f64::consts::PI * test_freq * t).sin() * 0.5;
            let output = filter.process(input);
            output_sum += output.abs();
        }
        
        // Фильтр должен что-то делать
        assert!(output_sum > 0.0);
        
        // Меняем частоту среза
        filter.set_cutoff(5000.0);
        filter.set_resonance(0.8);
        
        // Проверяем, что параметры изменились
        assert!((filter.cutoff - 5000.0).abs() < 1e-10);
        assert!((filter.resonance - 0.8).abs() < 1e-10);
    }
    
    #[test]
    fn test_cassette_deck() {
        let sample_rate = 44100.0;
        let mut deck = CassetteDeckModel::new(sample_rate);
        
        // Тестовый сигнал
        let test_freq = 1000.0;
        let num_samples = 4410;
        
        let mut max_output = 0.0;
        
        for i in 0..num_samples {
            let t = i as f64 / sample_rate;
            let input = (2.0 * std::f64::consts::PI * test_freq * t).sin() * 0.3;
            let output = deck.process(input);
            
            if output.abs() > max_output {
                max_output = output.abs();
            }
        }
        
        // Дека должна обрабатывать сигнал
        assert!(max_output > 0.0);
        assert!(max_output <= 1.0); // Не должно клиппировать сильно
        
        // Меняем скорость ленты
        deck.set_tape_speed(9.52); // Двойная скорость
        deck.set_bias_level(0.9);
        
        assert!((deck.tape_speed - 9.52).abs() < 1e-10);
        assert!((deck.bias_level - 0.9).abs() < 1e-10);
    }
    
    #[test]
    fn test_series_adapter() {
        let sample_rate = 44100.0;
        
        // ИСПРАВЛЕНО: явно приводим к типу Arc<RwLock<dyn WdfElement>>
        let resistor: Arc<RwLock<dyn WdfElement>> = Arc::new(RwLock::new(Resistor::new(1000.0)));
        let capacitor: Arc<RwLock<dyn WdfElement>> = Arc::new(RwLock::new(Capacitor::new(1e-6, sample_rate)));
        
        let elements = vec![resistor.clone(), capacitor.clone()];
        let adapter = SeriesAdapter::new(elements);
        
        let total_r = adapter.port_resistance();
        let r1 = resistor.read().port_resistance();
        let r2 = capacitor.read().port_resistance();
        
        // R_total = R1 + R2
        assert!((total_r - (r1 + r2)).abs() < 1e-10);
    }
}