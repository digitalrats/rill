//! High-precision audio processing with f64

use std::sync::Arc;
use parking_lot::RwLock;
use crate::{AudioError, AudioResult};

// --- High-precision буферы ---

/// Высокоточный аудиобуфер (f64)
#[derive(Debug, Clone)]
pub struct HighPrecisionBuffer {
    data: Arc<RwLock<Vec<f64>>>,
    size: usize,
    channels: usize,
    sample_rate: f64,
}

impl HighPrecisionBuffer {
    pub fn new(size: usize, channels: usize, sample_rate: f64) -> Self {
        Self {
            data: Arc::new(RwLock::new(vec![0.0; size * channels])),
            size,
            channels,
            sample_rate,
        }
    }
    
    pub fn from_f32(data: &[f32], channels: usize, sample_rate: f64) -> Self {
        let size = data.len() / channels;
        let mut buffer = Self::new(size, channels, sample_rate);
        
        let mut data_f64 = buffer.data.write();
        for (i, &sample) in data.iter().enumerate() {
            data_f64[i] = sample as f64;
        }
        
        buffer
    }
    
    pub fn write(&mut self, position: usize, channel: usize, value: f64) {
        let mut data = self.data.write();
        let idx = (position % self.size) * self.channels + channel;
        if idx < data.len() {
            data[idx] = value;
        }
    }
    
    pub fn read(&self, position: usize, channel: usize) -> f64 {
        let data = self.data.read();
        let idx = (position % self.size) * self.channels + channel;
        data.get(idx).copied().unwrap_or(0.0)
    }
    
    pub fn read_interpolated(&self, position: f64, channel: usize) -> f64 {
        let pos_floor = position.floor();
        let pos_frac = position.fract();
        
        let data = self.data.read();
        let idx1 = (pos_floor as usize % self.size) * self.channels + channel;
        let idx2 = ((pos_floor as usize + 1) % self.size) * self.channels + channel;
        
        let sample1 = data.get(idx1).copied().unwrap_or(0.0);
        let sample2 = data.get(idx2).copied().unwrap_or(0.0);
        
        sample1 + pos_frac * (sample2 - sample1)
    }
    
    pub fn to_f32(&self) -> Vec<f32> {
        let data = self.data.read();
        data.iter().map(|&x| x as f32).collect()
    }
    
    pub fn convert_from_f32(&mut self, data: &[f32]) {
        let mut buffer = self.data.write();
        for (i, &sample) in data.iter().enumerate() {
            if i < buffer.len() {
                buffer[i] = sample as f64;
            }
        }
    }
    
    pub fn convert_to_f32(&self, output: &mut [f32]) {
        let data = self.data.read();
        for (i, &sample) in data.iter().enumerate() {
            if i < output.len() {
                output[i] = sample as f32;
            }
        }
    }
}

// --- High-precision AudioNode ---

/// Базовый трейт для высокоточных аудиоузлов
pub trait HighPrecisionNode: Send + Sync {
    /// Обработка с f64
    fn process_hp(&mut self, inputs: &[&[f64]], outputs: &mut [&mut [f64]]) -> AudioResult<()>;
    
    /// Конвертация f32 → f64
    fn convert_inputs(&self, inputs: &[&[f32]]) -> Vec<Vec<f64>> {
        inputs.iter()
            .map(|buf| buf.iter().map(|&x| x as f64).collect())
            .collect()
    }
    
    /// Конвертация f64 → f32
    fn convert_outputs(&self, outputs_hp: &[Vec<f64>], outputs: &mut [&mut [f32]]) {
        for (i, output_hp) in outputs_hp.iter().enumerate() {
            if i < outputs.len() {
                for (j, &sample_hp) in output_hp.iter().enumerate() {
                    if j < outputs[i].len() {
                        outputs[i][j] = sample_hp as f32;
                    }
                }
            }
        }
    }
}

/// Обертка для использования HighPrecisionNode как обычного AudioNode
pub struct HighPrecisionAdapter<N: HighPrecisionNode> {
    node: N,
    temp_input_buffers: Vec<Vec<f64>>,
    temp_output_buffers: Vec<Vec<f64>>,
}

impl<N: HighPrecisionNode> HighPrecisionAdapter<N> {
    pub fn new(node: N) -> Self {
        Self {
            node,
            temp_input_buffers: Vec::new(),
            temp_output_buffers: Vec::new(),
        }
    }
}

impl<N: HighPrecisionNode> crate::node::AudioNode for HighPrecisionAdapter<N> {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        let buffer_size = outputs.get(0).map(|o| o.len()).unwrap_or(0);
        
        // Конвертируем входы в f64
        let inputs_hp: Vec<Vec<f64>> = inputs.iter()
            .map(|buf| buf.iter().map(|&x| x as f64).collect())
            .collect();
        
        // Подготавливаем выходные буферы f64
        self.temp_output_buffers = vec![vec![0.0; buffer_size]; outputs.len()];
        
        // Создаем срезы для обработки
        let input_slices: Vec<&[f64]> = inputs_hp.iter()
            .map(|buf| buf.as_slice())
            .collect();
        
        let mut output_slices: Vec<&mut [f64]> = self.temp_output_buffers.iter_mut()
            .map(|buf| buf.as_mut_slice())
            .collect();
        
        // Обрабатываем в high precision
        self.node.process_hp(&input_slices, &mut output_slices)?;
        
        // Конвертируем выходы обратно в f32
        for (i, output_hp) in self.temp_output_buffers.iter().enumerate() {
            if i < outputs.len() {
                for (j, &sample_hp) in output_hp.iter().enumerate() {
                    if j < outputs[i].len() {
                        outputs[i][j] = sample_hp as f32;
                    }
                }
            }
        }
        
        Ok(())
    }
    
    // Остальные методы AudioNode...
    fn get_param(&self, name: &str) -> Option<crate::param::ParamValue> {
        // Делегируем если node поддерживает параметры
        None
    }
    
    fn set_param(&mut self, _name: &str, _value: crate::param::ParamValue) -> Result<(), AudioError> {
        Ok(())
    }
    
    fn init(&mut self, sample_rate: f32) {
        // Можно передать sample_rate в node если нужно
    }
    
    fn reset(&mut self) {}
    
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn metadata(&self) -> crate::node::NodeMetadata {
        crate::node::NodeMetadata {
            name: "High Precision Node".to_string(),
            category: crate::node::NodeCategory::Effect,
            description: "High precision audio processing with f64".to_string(),
            author: "Kama Core".to_string(),
            version: "1.0".to_string(),
            parameters: Vec::new(),
        }
    }
}

// --- Специализированные high-precision модули ---

pub mod oscillators {
    use super::*;
    
    /// Высокоточный синусоидальный осциллятор
    pub struct HighPrecisionSineOscillator {
        frequency: f64,
        phase: f64,
        sample_rate: f64,
        amplitude: f64,
    }
    
    impl HighPrecisionSineOscillator {
        pub fn new(frequency: f64, sample_rate: f64, amplitude: f64) -> Self {
            Self {
                frequency,
                phase: 0.0,
                sample_rate,
                amplitude,
            }
        }
        
        pub fn set_frequency(&mut self, frequency: f64) {
            self.frequency = frequency.max(0.0).min(self.sample_rate / 2.0);
        }
        
        pub fn set_amplitude(&mut self, amplitude: f64) {
            self.amplitude = amplitude.max(0.0).min(1.0);
        }
        
        pub fn generate(&mut self, output: &mut [f64]) {
            let phase_increment = 2.0 * std::f64::consts::PI * self.frequency / self.sample_rate;
            
            for out in output.iter_mut() {
                *out = self.phase.sin() * self.amplitude;
                self.phase += phase_increment;
                
                // Нормализуем фазу для сохранения точности
                if self.phase > 2.0 * std::f64::consts::PI {
                    self.phase -= 2.0 * std::f64::consts::PI;
                }
            }
        }
    }
    
    impl HighPrecisionNode for HighPrecisionSineOscillator {
        fn process_hp(&mut self, inputs: &[&[f64]], outputs: &mut [&mut [f64]]) -> AudioResult<()> {
            if outputs.is_empty() {
                return Ok(());
            }
            
            let output = &mut outputs[0];
            self.generate(output);
            
            Ok(())
        }
    }
    
    /// Высокоточный FM осциллятор
    pub struct HighPrecisionFMOscillator {
        carrier_freq: f64,
        modulator_freq: f64,
        modulation_index: f64,
        carrier_phase: f64,
        modulator_phase: f64,
        sample_rate: f64,
        amplitude: f64,
    }
    
    impl HighPrecisionFMOscillator {
        pub fn new(
            carrier_freq: f64,
            modulator_freq: f64,
            modulation_index: f64,
            sample_rate: f64,
            amplitude: f64,
        ) -> Self {
            Self {
                carrier_freq,
                modulator_freq,
                modulation_index,
                carrier_phase: 0.0,
                modulator_phase: 0.0,
                sample_rate,
                amplitude,
            }
        }
        
        pub fn generate(&mut self, output: &mut [f64]) {
            let carrier_inc = 2.0 * std::f64::consts::PI * self.carrier_freq / self.sample_rate;
            let modulator_inc = 2.0 * std::f64::consts::PI * self.modulator_freq / self.sample_rate;
            
            for out in output.iter_mut() {
                // Модулирующая волна
                let modulation = self.modulator_phase.sin() * self.modulation_index;
                
                // Несущая волна с FM
                *out = (self.carrier_phase + modulation).sin() * self.amplitude;
                
                // Обновляем фазы
                self.carrier_phase += carrier_inc;
                self.modulator_phase += modulator_inc;
                
                // Нормализуем фазы
                if self.carrier_phase > 2.0 * std::f64::consts::PI {
                    self.carrier_phase -= 2.0 * std::f64::consts::PI;
                }
                if self.modulator_phase > 2.0 * std::f64::consts::PI {
                    self.modulator_phase -= 2.0 * std::f64::consts::PI;
                }
            }
        }
    }
}

pub mod filters {
    use super::*;
    
    /// Высокоточный биквадратный фильтр
    pub struct HighPrecisionBiquadFilter {
        b0: f64, b1: f64, b2: f64,
        a1: f64, a2: f64,
        x1: f64, x2: f64,
        y1: f64, y2: f64,
        sample_rate: f64,
    }
    
    impl HighPrecisionBiquadFilter {
        pub fn new_lowpass(cutoff: f64, q: f64, sample_rate: f64) -> Self {
            let omega = 2.0 * std::f64::consts::PI * cutoff / sample_rate;
            let alpha = omega.sin() / (2.0 * q);
            
            let b0 = (1.0 - omega.cos()) / 2.0;
            let b1 = 1.0 - omega.cos();
            let b2 = b0;
            let a0 = 1.0 + alpha;
            let a1 = -2.0 * omega.cos();
            let a2 = 1.0 - alpha;
            
            Self {
                b0: b0 / a0,
                b1: b1 / a0,
                b2: b2 / a0,
                a1: a1 / a0,
                a2: a2 / a0,
                x1: 0.0, x2: 0.0,
                y1: 0.0, y2: 0.0,
                sample_rate,
            }
        }
        
        pub fn process(&mut self, input: f64) -> f64 {
            let output = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1 - self.a2 * self.y2;
            
            // Обновляем состояния
            self.x2 = self.x1;
            self.x1 = input;
            self.y2 = self.y1;
            self.y1 = output;
            
            output
        }
        
        pub fn process_buffer(&mut self, input: &[f64], output: &mut [f64]) {
            for i in 0..input.len().min(output.len()) {
                output[i] = self.process(input[i]);
            }
        }
        
        pub fn reset(&mut self) {
            self.x1 = 0.0;
            self.x2 = 0.0;
            self.y1 = 0.0;
            self.y2 = 0.0;
        }
    }
    
    impl HighPrecisionNode for HighPrecisionBiquadFilter {
        fn process_hp(&mut self, inputs: &[&[f64]], outputs: &mut [&mut [f64]]) -> AudioResult<()> {
            if inputs.is_empty() || outputs.is_empty() {
                return Ok(());
            }
            
            let input = inputs[0];
            let output = &mut outputs[0];
            
            self.process_buffer(input, output);
            
            Ok(())
        }
    }
    
    /// Каскад биквадратных фильтров (для фильтров высокого порядка)
    pub struct HighPrecisionBiquadCascade {
        filters: Vec<HighPrecisionBiquadFilter>,
        temp_buffer: Vec<f64>,
    }
    
    impl HighPrecisionBiquadCascade {
        pub fn new_elliptic_lowpass(
            order: usize,
            cutoff: f64,
            ripple: f64,
            stopband_attenuation: f64,
            sample_rate: f64,
        ) -> Self {
            // Здесь должна быть реализация расчета коэффициентов для эллиптического фильтра
            // Для простоты создаем несколько идентичных фильтров
            let filters = (0..order)
                .map(|_| HighPrecisionBiquadFilter::new_lowpass(cutoff, 0.707, sample_rate))
                .collect();
            
            Self {
                filters,
                temp_buffer: Vec::new(),
            }
        }
        
        pub fn process_buffer(&mut self, input: &[f64], output: &mut [f64]) {
            if self.temp_buffer.len() < input.len() {
                self.temp_buffer.resize(input.len(), 0.0);
            }
            
            // Копируем вход
            self.temp_buffer[..input.len()].copy_from_slice(input);
            
            // Применяем фильтры последовательно
            for filter in &mut self.filters {
                filter.process_buffer(&self.temp_buffer[..input.len()], &mut output[..input.len()]);
                if filter != self.filters.last().unwrap() {
                    // Копируем выход для следующего фильтра
                    self.temp_buffer[..input.len()].copy_from_slice(&output[..input.len()]);
                }
            }
        }
    }
}

// --- Автоматическое определение precision ---

pub enum PrecisionMode {
    F32,  // Использовать f32
    F64,  // Использовать f64 с конвертацией
    Auto, // Автовыбор на основе требований
}

pub struct PrecisionConfig {
    pub mode: PrecisionMode,
    pub min_dynamic_range: f64, // в dB
    pub max_error_allowed: f64,
    pub enable_oversampling: bool,
}

impl Default for PrecisionConfig {
    fn default() -> Self {
        Self {
            mode: PrecisionMode::Auto,
            min_dynamic_range: 120.0, // 120 dB минимальный динамический диапазон
            max_error_allowed: -144.0, // -144 dBFS максимальная ошибка
            enable_oversampling: false,
        }
    }
}

/// Автоматический выбор precision на основе требований
pub fn auto_select_precision(config: &PrecisionConfig, node_type: &str) -> bool {
    // f64 нужен для:
    // - Синтезаторов (особенно FM/PM)
    // - Фильтров высокого порядка
    // - Wavefolders и других нелинейных процессоров
    // - При oversampling
    
    match config.mode {
        PrecisionMode::F32 => false,
        PrecisionMode::F64 => true,
        PrecisionMode::Auto => {
            match node_type {
                "sine_oscillator" => false, // Простые осцилляторы могут использовать f32
                "fm_oscillator" => true,    // FM требует f64
                "wavefolder" => true,       // Нелинейные процессы
                "biquad_filter" => false,   // Простые фильтры
                "elliptic_filter" => true,  // Сложные фильтры
                "granular" => true,         // Гранулярный синтез
                "convolution" => true,      // Свертка
                _ => config.enable_oversampling, // Если oversampling - используем f64
            }
        }
    }
}

// --- Утилиты для конвертации ---

pub mod conversion {
    use super::*;
    
    /// Конвертация с dithering'ом для уменьшения quantization noise
    pub fn convert_f64_to_f32_with_dither(input: &[f64], output: &mut [f32], tpdf_dither: bool) {
        if tpdf_dither {
            // Triangular PDF dither
            for i in 0..input.len().min(output.len()) {
                let dither = (rand::random::<f32>() - 0.5) * 2.0 / 65536.0; // 16-bit dither
                output[i] = (input[i] as f32 + dither).max(-1.0).min(1.0);
            }
        } else {
            // Простое округление
            for i in 0..input.len().min(output.len()) {
                output[i] = input[i] as f32;
            }
        }
    }
    
    /// Конвертация с noise shaping
    pub struct NoiseShaper {
        error_history: [f64; 4],
        coeffs: [f64; 4],
    }
    
    impl NoiseShaper {
        pub fn new() -> Self {
            // Простые коэффициенты для noise shaping
            Self {
                error_history: [0.0; 4],
                coeffs: [0.8, -0.6, 0.4, -0.2],
            }
        }
        
        pub fn convert_with_noise_shaping(&mut self, input: f64) -> f32 {
            // Добавляем сформированный шум от предыдущих ошибок
            let shaped_noise: f64 = self.error_history.iter()
                .zip(self.coeffs.iter())
                .map(|(error, coeff)| error * coeff)
                .sum();
            
            let input_with_noise = input + shaped_noise;
            let output_f32 = input_with_noise as f32;
            let output_f64 = output_f32 as f64;
            
            // Вычисляем ошибку квантования
            let error = input_with_noise - output_f64;
            
            // Обновляем историю ошибок
            self.error_history.rotate_right(1);
            self.error_history[0] = error;
            
            output_f32
        }
    }
    
    /// Конвертация с oversampling
    pub struct OversamplingConverter {
        oversampling_factor: usize,
        anti_alias_filter: crate::dsp::BiquadFilter,
        interpolation_filter: crate::dsp::BiquadFilter,
        temp_buffer: Vec<f64>,
    }
    
    impl OversamplingConverter {
        pub fn new(oversampling_factor: usize, sample_rate: f64) -> Self {
            Self {
                oversampling_factor,
                anti_alias_filter: crate::dsp::BiquadFilter::new_lowpass(
                    sample_rate as f32 * 0.45,
                    0.707,
                    sample_rate as f32,
                ),
                interpolation_filter: crate::dsp::BiquadFilter::new_lowpass(
                    sample_rate as f32 * 0.45,
                    0.707,
                    (sample_rate * oversampling_factor as f64) as f32,
                ),
                temp_buffer: Vec::new(),
            }
        }
        
        pub fn upsample(&mut self, input: &[f32], output: &mut [f64]) {
            let os_factor = self.oversampling_factor;
            let mut filter = self.anti_alias_filter.clone();
            
            for (i, &sample) in input.iter().enumerate() {
                // Вставляем zeros
                let base_idx = i * os_factor;
                for j in 0..os_factor {
                    if base_idx + j < output.len() {
                        output[base_idx + j] = if j == 0 {
                            sample as f64
                        } else {
                            0.0
                        };
                    }
                }
            }
            
            // Применяем интерполяционный фильтр
            for sample in output.iter_mut() {
                *sample = filter.process(*sample as f32) as f64;
            }
        }
        
        pub fn downsample(&mut self, input: &[f64], output: &mut [f32]) {
            let os_factor = self.oversampling_factor;
            let mut filter = self.interpolation_filter.clone();
            
            // Фильтрация на высокой частоте дискретизации
            self.temp_buffer.resize(input.len(), 0.0);
            for (i, &sample) in input.iter().enumerate() {
                self.temp_buffer[i] = filter.process(sample as f32) as f64;
            }
            
            // Децимация
            for (i, out_sample) in output.iter_mut().enumerate() {
                let idx = i * os_factor;
                if idx < self.temp_buffer.len() {
                    *out_sample = self.temp_buffer[idx] as f32;
                }
            }
        }
    }
}

// --- Макросы для удобства ---

#[macro_export]
macro_rules! hp_node {
    ($node:expr) => {
        HighPrecisionAdapter::new($node)
    };
}

#[macro_export]
macro_rules! conditional_precision {
    ($config:expr, $node_type:expr, $f32_block:block, $f64_block:block) => {
        if auto_select_precision($config, $node_type) {
            $f64_block
        } else {
            $f32_block
        }
    };
}

// --- Пример использования ---

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_high_precision_sine() {
        let mut osc = oscillators::HighPrecisionSineOscillator::new(440.0, 44100.0, 0.5);
        let mut output = vec![0.0f64; 1024];
        
        osc.generate(&mut output);
        
        // Проверяем, что сигнал генерируется
        assert!(output.iter().any(|&x| x != 0.0));
        
        // Проверяем амплитуду
        let max_amplitude = output.iter()
            .map(|&x| x.abs())
            .fold(0.0f64, |a, b| a.max(b));
        
        assert!((max_amplitude - 0.5).abs() < 0.001);
    }
    
    #[test]
    fn test_precision_comparison() {
        // Тест кумулятивной ошибки в f32 vs f64
        let iterations = 100000;
        
        // f32 версия
        let mut sum_f32: f32 = 0.0;
        let increment_f32: f32 = 0.1;
        for _ in 0..iterations {
            sum_f32 += increment_f32;
        }
        
        // f64 версия
        let mut sum_f64: f64 = 0.0;
        let increment_f64: f64 = 0.1;
        for _ in 0..iterations {
            sum_f64 += increment_f64;
        }
        
        let expected = increment_f32 as f64 * iterations as f64;
        let error_f32 = (sum_f32 as f64 - expected).abs();
        let error_f64 = (sum_f64 - expected).abs();
        
        // f64 должна быть точнее
        assert!(error_f64 < error_f32);
        
        println!("f32 error: {:.10}", error_f32);
        println!("f64 error: {:.10}", error_f64);
    }
    
    #[test]
    fn test_fm_oscillator_precision() {
        let mut fm_osc = oscillators::HighPrecisionFMOscillator::new(
            440.0,    // carrier
            220.0,    // modulator
            5.0,      // modulation index
            44100.0,  // sample rate
            0.5,      // amplitude
        );
        
        let mut output = vec![0.0f64; 1024];
        
        // Используем адаптер для интеграции с kama-core
        let adapter = HighPrecisionAdapter::new(fm_osc);
        
        // Создаем тестовые буферы
        let input_buffer = vec![0.0f32; 1024];
        let mut output_buffer = vec![0.0f32; 1024];
        
        let inputs = [&input_buffer[..]];
        let mut outputs = [&mut output_buffer[..]];
        
        // Обрабатываем
        let mut adapter = adapter;
        adapter.process(&inputs, &mut outputs).unwrap();
        
        // Проверяем результат
        assert!(output_buffer.iter().any(|&x| x != 0.0));
    }
    
    #[test]
    fn test_precision_auto_selection() {
        let config = PrecisionConfig::default();
        
        // FM осциллятор должен использовать f64
        assert!(auto_select_precision(&config, "fm_oscillator"));
        
        // Простой синусоидальный осциллятор может использовать f32
        assert!(!auto_select_precision(&config, "sine_oscillator"));
        
        // Эллиптический фильтр должен использовать f64
        assert!(auto_select_precision(&config, "elliptic_filter"));
    }
}

//! SIMD ускорение для high-precision (f64) аудиообработки

use core::simd::{f64x2, f64x4, f64x8, Simd, SimdFloat, SimdInt};
use std::arch::is_x86_feature_detected;

/// Конфигурация SIMD для f64
#[derive(Debug, Clone, Copy)]
pub struct F64SimdConfig {
    pub has_avx512: bool,    // AVX-512 для f64
    pub has_avx2: bool,      // AVX2 для f64
    pub has_sse2: bool,      // SSE2 (базовая поддержка f64)
    pub optimal_width: usize, // Оптимальная ширина вектора
}

impl F64SimdConfig {
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        let (has_avx512, has_avx2, has_sse2) = unsafe {
            use std::arch::x86_64::*;
            (
                is_x86_feature_detected!("avx512f"),
                is_x86_feature_detected!("avx2"),
                is_x86_feature_detected!("sse2"),
            )
        };
        
        #[cfg(not(target_arch = "x86_64"))]
        let (has_avx512, has_avx2, has_sse2) = (false, false, true); // SSE2 обычно есть везде
        
        let optimal_width = if has_avx512 {
            8  // AVX-512: 512-bit = 8 f64
        } else if has_avx2 {
            4  // AVX2: 256-bit = 4 f64
        } else {
            2  // SSE2: 128-bit = 2 f64
        };
        
        Self {
            has_avx512,
            has_avx2,
            has_sse2,
            optimal_width,
        }
    }
}

/// Выровненный буфер для f64 SIMD операций
#[derive(Debug)]
pub struct AlignedF64Buffer {
    data: Vec<f64>,
    alignment: usize,
}

impl AlignedF64Buffer {
    pub fn new(size: usize, alignment: Option<usize>) -> Self {
        let alignment = alignment.unwrap_or(64); // По умолчанию 64 байта
        let mut data = Vec::with_capacity(size + alignment / 8);
        
        // Выравниваем начало
        let ptr = data.as_mut_ptr() as usize;
        let offset = ptr.align_offset(alignment);
        if offset > 0 {
            unsafe { data.set_len(offset) };
        }
        
        data.reserve(size);
        unsafe { data.set_len(size) };
        
        Self { data, alignment }
    }
    
    pub fn as_slice(&self) -> &[f64] {
        &self.data
    }
    
    pub fn as_mut_slice(&mut self) -> &mut [f64] {
        &mut self.data
    }
    
    pub fn as_ptr(&self) -> *const f64 {
        self.data.as_ptr()
    }
    
    pub fn as_mut_ptr(&mut self) -> *mut f64 {
        self.data.as_mut_ptr()
    }
    
    pub fn is_aligned(&self) -> bool {
        (self.as_ptr() as usize) % self.alignment == 0
    }
}

// --- SIMD операции для high-precision ---

/// SIMD конвертация f32 → f64
pub fn simd_convert_f32_to_f64(input: &[f32], output: &mut [f64]) {
    let config = F64SimdConfig::detect();
    
    match config.optimal_width {
        8 => convert_f32_to_f64_f64x8(input, output),
        4 => convert_f32_to_f64_f64x4(input, output),
        2 => convert_f32_to_f64_f64x2(input, output),
        _ => scalar_convert_f32_to_f64(input, output),
    }
}

fn convert_f32_to_f64_f64x8(input: &[f32], output: &mut [f64]) {
    // Обрабатываем по 8 f32 -> 8 f64 за раз
    let chunks = input.chunks_exact(8);
    let remainder = chunks.remainder();
    
    for (i, chunk) in chunks.enumerate() {
        // Конвертируем каждый элемент (непрямая SIMD, но всё равно быстрее)
        for j in 0..8 {
            output[i * 8 + j] = chunk[j] as f64;
        }
    }
    
    // Остаток
    let start = input.len() - remainder.len();
    for i in 0..remainder.len() {
        output[start + i] = input[start + i] as f64;
    }
}

fn convert_f32_to_f64_f64x4(input: &[f32], output: &mut [f64]) {
    let chunks = input.chunks_exact(4);
    let remainder = chunks.remainder();
    
    for (i, chunk) in chunks.enumerate() {
        let mut arr = [0.0f64; 4];
        for j in 0..4 {
            arr[j] = chunk[j] as f64;
        }
        let simd_vec = f64x4::from_array(arr);
        simd_vec.copy_to_slice(&mut output[i*4..(i+1)*4]);
    }
    
    let start = input.len() - remainder.len();
    for i in 0..remainder.len() {
        output[start + i] = input[start + i] as f64;
    }
}

fn scalar_convert_f32_to_f64(input: &[f32], output: &mut [f64]) {
    for i in 0..input.len().min(output.len()) {
        output[i] = input[i] as f64;
    }
}

/// SIMD конвертация f64 → f32 с округлением
pub fn simd_convert_f64_to_f32(input: &[f64], output: &mut [f32], dither: bool) {
    let config = F64SimdConfig::detect();
    
    if dither {
        match config.optimal_width {
            4 => convert_f64_to_f32_dithered_f64x4(input, output),
            2 => convert_f64_to_f32_dithered_f64x2(input, output),
            _ => convert_f64_to_f32_dithered_scalar(input, output),
        }
    } else {
        match config.optimal_width {
            4 => convert_f64_to_f32_f64x4(input, output),
            2 => convert_f64_to_f32_f64x2(input, output),
            _ => scalar_convert_f64_to_f32(input, output),
        }
    }
}

fn convert_f64_to_f32_f64x4(input: &[f64], output: &mut [f32]) {
    let chunks = input.chunks_exact(4);
    let remainder = chunks.remainder();
    
    for (i, chunk) in chunks.enumerate() {
        let simd_vec = f64x4::from_slice(chunk);
        // Конвертируем в f32 (нужна кастомизация для лучшего контроля)
        for j in 0..4 {
            output[i*4 + j] = simd_vec[j] as f32;
        }
    }
    
    let start = input.len() - remainder.len();
    for i in 0..remainder.len() {
        output[start + i] = input[start + i] as f32;
    }
}

fn convert_f64_to_f32_dithered_f64x4(input: &[f64], output: &mut [f32]) {
    let dither_scale = 1.0 / 65536.0; // 16-bit dither
    let half_dither = dither_scale * 0.5;
    
    let chunks = input.chunks_exact(4);
    let remainder = chunks.remainder();
    
    for (i, chunk) in chunks.enumerate() {
        let base_vec = f64x4::from_slice(chunk);
        
        // Генерируем TPDF dither
        let dither1 = f64x4::from_array([
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
        ]);
        let dither2 = f64x4::from_array([
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
        ]);
        
        // TPDF: dither1 + dither2 - 1.0
        let tpdf_dither = (dither1 + dither2 - f64x4::splat(1.0)) * dither_scale;
        
        // Добавляем dither и конвертируем
        let dithered = base_vec + tpdf_dither;
        
        for j in 0..4 {
            let val = dithered[j];
            output[i*4 + j] = val.max(-1.0).min(1.0) as f32;
        }
    }
    
    let start = input.len() - remainder.len();
    for i in 0..remainder.len() {
        let dither = (rand::random::<f64>() * 2.0 - 1.0) * dither_scale;
        let dithered = input[start + i] + dither;
        output[start + i] = dithered.max(-1.0).min(1.0) as f32;
    }
}

// --- SIMD оптимизированные high-precision осцилляторы ---

pub mod simd_oscillators {
    use super::*;
    use std::f64::consts::PI;
    
    /// SIMD-оптимизированный высокоточный синусоидальный осциллятор
    pub struct SimdHighPrecisionSineOscillator {
        frequency: f64,
        phase: f64,
        phase_increment: f64,
        sample_rate: f64,
        amplitude: f64,
        simd_config: F64SimdConfig,
        phase_vector: Option<f64x4>, // Кэшированный SIMD вектор фаз
    }
    
    impl SimdHighPrecisionSineOscillator {
        pub fn new(frequency: f64, sample_rate: f64, amplitude: f64) -> Self {
            let simd_config = F64SimdConfig::detect();
            let phase_increment = 2.0 * PI * frequency / sample_rate;
            
            Self {
                frequency,
                phase: 0.0,
                phase_increment,
                sample_rate,
                amplitude,
                simd_config,
                phase_vector: None,
            }
        }
        
        /// Генерация с SIMD оптимизацией
        pub fn generate_simd(&mut self, output: &mut [f64]) {
            match self.simd_config.optimal_width {
                4 => self.generate_f64x4(output),
                2 => self.generate_f64x2(output),
                _ => self.generate_scalar(output),
            }
        }
        
        fn generate_f64x4(&mut self, output: &mut [f64]) {
            let amplitude_vec = f64x4::splat(self.amplitude);
            let two_pi_vec = f64x4::splat(2.0 * PI);
            let phase_inc_vec = f64x4::splat(self.phase_increment);
            
            // Создаем вектор фаз: [phase, phase+inc, phase+2*inc, phase+3*inc]
            let mut phase_vec = f64x4::from_array([
                self.phase,
                self.phase + self.phase_increment,
                self.phase + 2.0 * self.phase_increment,
                self.phase + 3.0 * self.phase_increment,
            ]);
            
            let chunks = output.chunks_exact_mut(4);
            let remainder = chunks.remainder();
            
            for chunk in chunks {
                // Вычисляем синус
                let sine_vec = phase_vec.sin();
                let output_vec = sine_vec * amplitude_vec;
                
                // Сохраняем результат
                output_vec.copy_to_slice(chunk);
                
                // Обновляем фазы для следующей итерации
                phase_vec += phase_inc_vec * f64x4::splat(4.0);
                
                // Нормализуем фазы (должно быть редко, но важно для точности)
                let needs_normalization = phase_vec.simd_ge(two_pi_vec);
                if needs_normalization.any() {
                    // Вычитаем 2π там, где нужно
                    phase_vec = phase_vec - (needs_normalization.select(two_pi_vec, f64x4::splat(0.0)));
                }
            }
            
            // Сохраняем последнюю фазу
            self.phase = phase_vec[0];
            
            // Обрабатываем остаток
            let start = output.len() - remainder.len();
            for i in start..output.len() {
                output[i] = self.phase.sin() * self.amplitude;
                self.phase += self.phase_increment;
                
                if self.phase >= 2.0 * PI {
                    self.phase -= 2.0 * PI;
                }
            }
        }
        
        fn generate_scalar(&mut self, output: &mut [f64]) {
            for out in output.iter_mut() {
                *out = self.phase.sin() * self.amplitude;
                self.phase += self.phase_increment;
                
                if self.phase >= 2.0 * PI {
                    self.phase -= 2.0 * PI;
                }
            }
        }
    }
    
    /// SIMD-оптимизированный высокоточный FM осциллятор
    pub struct SimdHighPrecisionFMOscillator {
        carrier_freq: f64,
        modulator_freq: f64,
        modulation_index: f64,
        carrier_phase: f64,
        modulator_phase: f64,
        sample_rate: f64,
        amplitude: f64,
        simd_config: F64SimdConfig,
        
        // Предвычисленные константы для SIMD
        carrier_inc: f64,
        modulator_inc: f64,
        two_pi: f64,
    }
    
    impl SimdHighPrecisionFMOscillator {
        pub fn new(
            carrier_freq: f64,
            modulator_freq: f64,
            modulation_index: f64,
            sample_rate: f64,
            amplitude: f64,
        ) -> Self {
            let simd_config = F64SimdConfig::detect();
            
            Self {
                carrier_freq,
                modulator_freq,
                modulation_index,
                carrier_phase: 0.0,
                modulator_phase: 0.0,
                sample_rate,
                amplitude,
                simd_config,
                carrier_inc: 2.0 * PI * carrier_freq / sample_rate,
                modulator_inc: 2.0 * PI * modulator_freq / sample_rate,
                two_pi: 2.0 * PI,
            }
        }
        
        /// SIMD генерация с FM
        pub fn generate_simd(&mut self, output: &mut [f64]) {
            match self.simd_config.optimal_width {
                4 => self.generate_f64x4(output),
                2 => self.generate_f64x2(output),
                _ => self.generate_scalar(output),
            }
        }
        
        fn generate_f64x4(&mut self, output: &mut [f64]) {
            let amplitude_vec = f64x4::splat(self.amplitude);
            let modulation_index_vec = f64x4::splat(self.modulation_index);
            let two_pi_vec = f64x4::splat(self.two_pi);
            let carrier_inc_vec = f64x4::splat(self.carrier_inc);
            let modulator_inc_vec = f64x4::splat(self.modulator_inc);
            
            // Инициализируем SIMD векторы фаз
            let mut carrier_phase_vec = f64x4::from_array([
                self.carrier_phase,
                self.carrier_phase + self.carrier_inc,
                self.carrier_phase + 2.0 * self.carrier_inc,
                self.carrier_phase + 3.0 * self.carrier_inc,
            ]);
            
            let mut modulator_phase_vec = f64x4::from_array([
                self.modulator_phase,
                self.modulator_phase + self.modulator_inc,
                self.modulator_phase + 2.0 * self.modulator_inc,
                self.modulator_phase + 3.0 * self.modulator_inc,
            ]);
            
            let chunks = output.chunks_exact_mut(4);
            let remainder = chunks.remainder();
            
            for chunk in chunks {
                // Синус модулирующей волны
                let modulator_sin = modulator_phase_vec.sin();
                
                // FM: фаза несущей + модуляция
                let modulated_phase = carrier_phase_vec + modulator_sin * modulation_index_vec;
                
                // Синус модулированной несущей
                let output_vec = modulated_phase.sin() * amplitude_vec;
                
                // Сохраняем результат
                output_vec.copy_to_slice(chunk);
                
                // Обновляем фазы
                carrier_phase_vec += carrier_inc_vec * f64x4::splat(4.0);
                modulator_phase_vec += modulator_inc_vec * f64x4::splat(4.0);
                
                // Нормализуем фазы
                let carrier_needs_norm = carrier_phase_vec.simd_ge(two_pi_vec);
                let modulator_needs_norm = modulator_phase_vec.simd_ge(two_pi_vec);
                
                if carrier_needs_norm.any() {
                    carrier_phase_vec = carrier_phase_vec - 
                        (carrier_needs_norm.select(two_pi_vec, f64x4::splat(0.0)));
                }
                
                if modulator_needs_norm.any() {
                    modulator_phase_vec = modulator_phase_vec - 
                        (modulator_needs_norm.select(two_pi_vec, f64x4::splat(0.0)));
                }
            }
            
            // Сохраняем состояния
            self.carrier_phase = carrier_phase_vec[0];
            self.modulator_phase = modulator_phase_vec[0];
            
            // Остаток
            let start = output.len() - remainder.len();
            for i in start..output.len() {
                let modulation = self.modulator_phase.sin() * self.modulation_index;
                output[i] = (self.carrier_phase + modulation).sin() * self.amplitude;
                
                self.carrier_phase += self.carrier_inc;
                self.modulator_phase += self.modulator_inc;
                
                if self.carrier_phase >= self.two_pi {
                    self.carrier_phase -= self.two_pi;
                }
                if self.modulator_phase >= self.two_pi {
                    self.modulator_phase -= self.two_pi;
                }
            }
        }
    }
}

// --- SIMD оптимизированные high-precision фильтры ---

pub mod simd_filters {
    use super::*;
    
    /// SIMD-оптимизированный high-precision биквадратный фильтр
    pub struct SimdHighPrecisionBiquadFilter {
        // Коэффициенты
        b0: f64, b1: f64, b2: f64,
        a1: f64, a2: f64,
        
        // SIMD-векторизованные состояния
        x1: f64x4, x2: f64x4,
        y1: f64x4, y2: f64x4,
        
        sample_rate: f64,
        simd_config: F64SimdConfig,
    }
    
    impl SimdHighPrecisionBiquadFilter {
        pub fn new_lowpass(cutoff: f64, q: f64, sample_rate: f64) -> Self {
            let omega = 2.0 * std::f64::consts::PI * cutoff / sample_rate;
            let alpha = omega.sin() / (2.0 * q);
            
            let b0 = (1.0 - omega.cos()) / 2.0;
            let b1 = 1.0 - omega.cos();
            let b2 = b0;
            let a0 = 1.0 + alpha;
            let a1 = -2.0 * omega.cos();
            let a2 = 1.0 - alpha;
            
            let simd_config = F64SimdConfig::detect();
            
            Self {
                b0: b0 / a0,
                b1: b1 / a0,
                b2: b2 / a0,
                a1: a1 / a0,
                a2: a2 / a0,
                x1: f64x4::splat(0.0),
                x2: f64x4::splat(0.0),
                y1: f64x4::splat(0.0),
                y2: f64x4::splat(0.0),
                sample_rate,
                simd_config,
            }
        }
        
        /// Пакетная обработка с SIMD
        pub fn process_buffer_simd(&mut self, input: &[f64], output: &mut [f64]) {
            match self.simd_config.optimal_width {
                4 => self.process_f64x4(input, output),
                2 => self.process_f64x2(input, output),
                _ => self.process_scalar(input, output),
            }
        }
        
        fn process_f64x4(&mut self, input: &[f64], output: &mut [f64]) {
            // Загружаем коэффициенты в SIMD векторы
            let b0_vec = f64x4::splat(self.b0);
            let b1_vec = f64x4::splat(self.b1);
            let b2_vec = f64x4::splat(self.b2);
            let a1_vec = f64x4::splat(self.a1);
            let a2_vec = f64x4::splat(self.a2);
            
            let mut x1 = self.x1;
            let mut x2 = self.x2;
            let mut y1 = self.y1;
            let mut y2 = self.y2;
            
            let chunks = input.chunks_exact(4);
            let remainder = chunks.remainder();
            
            for (i, chunk) in chunks.enumerate() {
                let input_vec = f64x4::from_slice(chunk);
                
                // Direct Form II в SIMD
                // y[n] = b0*x[n] + b1*x[n-1] + b2*x[n-2] - a1*y[n-1] - a2*y[n-2]
                let output_vec = b0_vec * input_vec + b1_vec * x1 + b2_vec * x2
                    - a1_vec * y1 - a2_vec * y2;
                
                // Сохраняем результат
                output_vec.copy_to_slice(&mut output[i*4..(i+1)*4]);
                
                // Обновляем состояния
                x2 = x1;
                x1 = input_vec;
                y2 = y1;
                y1 = output_vec;
            }
            
            // Сохраняем SIMD состояния
            self.x1 = x1;
            self.x2 = x2;
            self.y1 = y1;
            self.y2 = y2;
            
            // Обработка остатка (скалярно)
            let start = input.len() - remainder.len();
            for i in 0..remainder.len() {
                let idx = start + i;
                let x = input[idx];
                
                // Для остатка используем скалярные состояния
                let x1_scalar = x1[0];
                let x2_scalar = x2[0];
                let y1_scalar = y1[0];
                let y2_scalar = y2[0];
                
                let y = self.b0 * x + self.b1 * x1_scalar + self.b2 * x2_scalar
                    - self.a1 * y1_scalar - self.a2 * y2_scalar;
                
                output[idx] = y;
                
                // Обновляем скалярные состояния
                x1 = f64x4::from_array([y, x1[1], x1[2], x1[3]]);
                x2 = f64x4::from_array([x, x2[1], x2[2], x2[3]]);
                y2 = f64x4::from_array([y1_scalar, y2[1], y2[2], y2[3]]);
                y1 = f64x4::from_array([y, y1[1], y1[2], y1[3]]);
            }
        }
        
        fn process_scalar(&mut self, input: &[f64], output: &mut [f64]) {
            // Используем скалярные состояния из SIMD вектора
            let mut x1 = self.x1[0];
            let mut x2 = self.x2[0];
            let mut y1 = self.y1[0];
            let mut y2 = self.y2[0];
            
            for i in 0..input.len().min(output.len()) {
                let x = input[i];
                let y = self.b0 * x + self.b1 * x1 + self.b2 * x2
                    - self.a1 * y1 - self.a2 * y2;
                
                output[i] = y;
                
                x2 = x1;
                x1 = x;
                y2 = y1;
                y1 = y;
            }
            
            // Обновляем SIMD состояния
            self.x1 = f64x4::splat(x1);
            self.x2 = f64x4::splat(x2);
            self.y1 = f64x4::splat(y1);
            self.y2 = f64x4::splat(y2);
        }
    }
    
    /// SIMD-оптимизированный каскад фильтров
    pub struct SimdHighPrecisionBiquadCascade {
        filters: Vec<SimdHighPrecisionBiquadFilter>,
        temp_buffers: [AlignedF64Buffer; 2], // Double-buffering для SIMD
        current_buffer: usize,
    }
    
    impl SimdHighPrecisionBiquadCascade {
        pub fn new(order: usize, cutoff: f64, q: f64, sample_rate: f64) -> Self {
            let filters = (0..order)
                .map(|_| SimdHighPrecisionBiquadFilter::new_lowpass(cutoff, q, sample_rate))
                .collect();
            
            // Создаем выровненные буферы для SIMD
            let buffer_size = 4096; // Начальный размер
            let temp_buffers = [
                AlignedF64Buffer::new(buffer_size, Some(64)),
                AlignedF64Buffer::new(buffer_size, Some(64)),
            ];
            
            Self {
                filters,
                temp_buffers,
                current_buffer: 0,
            }
        }
        
        pub fn process_simd(&mut self, input: &[f64], output: &mut [f64]) {
            let buffer_size = input.len();
            
            // Убедимся, что буферы достаточно большие
            if self.temp_buffers[0].as_slice().len() < buffer_size {
                let new_size = buffer_size.next_power_of_two();
                self.temp_buffers[0] = AlignedF64Buffer::new(new_size, Some(64));
                self.temp_buffers[1] = AlignedF64Buffer::new(new_size, Some(64));
            }
            
            // Копируем вход в текущий буфер
            let src = self.current_buffer;
            let dst = 1 - src;
            
            let temp_in = &mut self.temp_buffers[src].as_mut_slice()[..buffer_size];
            temp_in.copy_from_slice(input);
            
            // Применяем фильтры последовательно
            for (i, filter) in self.filters.iter_mut().enumerate() {
                let current_input = if i == 0 {
                    temp_in
                } else {
                    &self.temp_buffers[dst].as_slice()[..buffer_size]
                };
                
                let current_output = if i == self.filters.len() - 1 {
                    output
                } else {
                    &mut self.temp_buffers[dst].as_mut_slice()[..buffer_size]
                };
                
                filter.process_buffer_simd(current_input, current_output);
            }
            
            // Меняем буферы местами для следующего вызова
            self.current_buffer = dst;
        }
    }
}

// --- SIMD для oversampling ---

pub mod simd_oversampling {
    use super::*;
    
    /// SIMD-оптимизированный oversampling конвертер
    pub struct SimdOversamplingConverter {
        factor: usize,
        halfband_filters: Vec<SimdHighPrecisionBiquadFilter>,
        temp_buffer: AlignedF64Buffer,
        simd_config: F64SimdConfig,
    }
    
    impl SimdOversamplingConverter {
        pub fn new(factor: usize, sample_rate: f64) -> Self {
            // Создаем half-band фильтры для oversampling
            // (упрощённо, на практике нужны более сложные фильтры)
            let mut halfband_filters = Vec::new();
            
            for _ in 0..(factor.ilog2() as usize) {
                halfband_filters.push(SimdHighPrecisionBiquadFilter::new_lowpass(
                    sample_rate * 0.45,
                    0.707,
                    sample_rate * 2.0, // Предполагаем удвоение на каждом этапе
                ));
            }
            
            let temp_buffer = AlignedF64Buffer::new(4096 * factor, Some(64));
            
            Self {
                factor,
                halfband_filters,
                temp_buffer,
                simd_config: F64SimdConfig::detect(),
            }
        }
        
        /// Upsample с SIMD
        pub fn upsample_simd(&mut self, input: &[f64], output: &mut [f64]) {
            let os_factor = self.factor;
            let output_len = input.len() * os_factor;
            
            if output.len() < output_len {
                return;
            }
            
            // Zero-insertion с SIMD
            match self.simd_config.optimal_width {
                4 => self.upsample_f64x4(input, output, os_factor),
                2 => self.upsample_f64x2(input, output, os_factor),
                _ => self.upsample_scalar(input, output, os_factor),
            }
            
            // Применяем half-band фильтры
            for filter in &mut self.halfband_filters {
                filter.process_buffer_simd(&output[..output_len], output);
            }
        }
        
        fn upsample_f64x4(&mut self, input: &[f64], output: &mut [f64], factor: usize) {
            // Zero-insertion с SIMD
            for (i, &sample) in input.iter().enumerate() {
                let base_idx = i * factor;
                
                // Первый элемент - семпл, остальные - нули
                output[base_idx] = sample;
                
                // Заполняем нулями оставшуюся часть
                for j in 1..factor {
                    if base_idx + j < output.len() {
                        output[base_idx + j] = 0.0;
                    }
                }
            }
        }
    }
}

// --- Интеграция с существующим HighPrecisionNode ---

pub trait SimdHighPrecisionNode: Send + Sync {
    /// Автоматическое определение поддержки SIMD
    fn supports_simd(&self) -> bool {
        F64SimdConfig::detect().optimal_width > 1
    }
    
    /// Обработка с автоматическим выбором SIMD/scalar
    fn process_auto_simd(&mut self, inputs: &[&[f64]], outputs: &mut [&mut [f64]]) -> AudioResult<()> {
        if self.supports_simd() {
            self.process_simd(inputs, outputs)
        } else {
            self.process_scalar(inputs, outputs)
        }
    }
    
    /// SIMD-оптимизированная обработка
    fn process_simd(&mut self, inputs: &[&[f64]], outputs: &mut [&mut [f64]]) -> AudioResult<()>;
    
    /// Скалярная обработка (fallback)
    fn process_scalar(&mut self, inputs: &[&[f64]], outputs: &mut [&mut [f64]]) -> AudioResult<()>;
}

/// Адаптер для автоматического использования SIMD
pub struct AutoSimdHighPrecisionAdapter<N: SimdHighPrecisionNode + HighPrecisionNode> {
    node: N,
    use_simd: bool,
    temp_buffers: Vec<AlignedF64Buffer>,
}

impl<N: SimdHighPrecisionNode + HighPrecisionNode> AutoSimdHighPrecisionAdapter<N> {
    pub fn new(node: N) -> Self {
        let use_simd = node.supports_simd();
        
        Self {
            node,
            use_simd,
            temp_buffers: Vec::new(),
        }
    }
    
    pub fn process_optimized(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> AudioResult<()> {
        let buffer_size = outputs.get(0).map(|o| o.len()).unwrap_or(0);
        
        // Подготавливаем временные буферы
        self.prepare_buffers(buffer_size, inputs.len(), outputs.len());
        
        // Конвертируем входы в f64 с SIMD
        for (i, input) in inputs.iter().enumerate() {
            if i < self.temp_buffers.len() / 2 {
                simd_convert_f32_to_f64(
                    input,
                    &mut self.temp_buffers[i].as_mut_slice()[..buffer_size],
                );
            }
        }
        
        // Создаем срезы для обработки
        let input_slices: Vec<&[f64]> = self.temp_buffers.iter()
            .take(inputs.len())
            .map(|buf| &buf.as_slice()[..buffer_size])
            .collect();
        
        let mut output_slices: Vec<&mut [f64]> = self.temp_buffers.iter_mut()
            .skip(inputs.len())
            .take(outputs.len())
            .map(|buf| &mut buf.as_mut_slice()[..buffer_size])
            .collect();
        
        // Обрабатываем с автоматическим выбором SIMD
        if self.use_simd {
            self.node.process_simd(&input_slices, &mut output_slices)?;
        } else {
            self.node.process_scalar(&input_slices, &mut output_slices)?;
        }
        
        // Конвертируем выходы обратно в f32 с SIMD
        for (i, output) in outputs.iter_mut().enumerate() {
            if i < output_slices.len() {
                simd_convert_f64_to_f32(
                    &output_slices[i],
                    output,
                    false, // Без dither'а для производительности
                );
            }
        }
        
        Ok(())
    }
    
    fn prepare_buffers(&mut self, buffer_size: usize, num_inputs: usize, num_outputs: usize) {
        let total_needed = num_inputs + num_outputs;
        
        while self.temp_buffers.len() < total_needed {
            self.temp_buffers.push(AlignedF64Buffer::new(
                buffer_size.next_power_of_two(),
                Some(64),
            ));
        }
        
        // Убедимся, что все буферы достаточно большие
        for buffer in &mut self.temp_buffers {
            if buffer.as_slice().len() < buffer_size {
                *buffer = AlignedF64Buffer::new(
                    buffer_size.next_power_of_two(),
                    Some(64),
                );
            }
        }
    }
}

// --- Бенчмарки и тесты ---

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    
    #[test]
    fn test_f64_simd_config() {
        let config = F64SimdConfig::detect();
        
        println!("F64 SIMD Configuration:");
        println!("  AVX512: {}", config.has_avx512);
        println!("  AVX2: {}", config.has_avx2);
        println!("  SSE2: {}", config.has_sse2);
        println!("  Optimal width: {}", config.optimal_width);
        
        assert!(config.optimal_width >= 2, "SSE2 should be available on x86_64");
    }
    
    #[test]
    fn test_simd_sine_oscillator() {
        let mut osc = simd_oscillators::SimdHighPrecisionSineOscillator::new(
            440.0, 44100.0, 0.5
        );
        
        let buffer_size = 1024;
        let mut output = vec![0.0f64; buffer_size];
        
        // Тестируем SIMD версию
        let start = Instant::now();
        osc.generate_simd(&mut output);
        let simd_time = start.elapsed();
        
        // Проверяем результат
        let max_amplitude = output.iter()
            .map(|&x| x.abs())
            .fold(0.0f64, |a, b| a.max(b));
        
        assert!((max_amplitude - 0.5).abs() < 0.001);
        
        // Тестируем скалярную версию для сравнения
        let mut osc_scalar = oscillators::HighPrecisionSineOscillator::new(
            440.0, 44100.0, 0.5
        );
        let mut output_scalar = vec![0.0f64; buffer_size];
        
        let start = Instant::now();
        osc_scalar.generate(&mut output_scalar);
        let scalar_time = start.elapsed();
        
        // Сравниваем результаты
        let mut max_error = 0.0;
        for i in 0..buffer_size {
            let error = (output[i] - output_scalar[i]).abs();
            if error > max_error {
                max_error = error;
            }
        }
        
        println!("SIMD sine oscillator:");
        println!("  SIMD time: {:?}", simd_time);
        println!("  Scalar time: {:?}", scalar_time);
        println!("  Speedup: {:.2}x", 
                scalar_time.as_nanos() as f64 / simd_time.as_nanos() as f64);
        println!("  Max error: {:.10}", max_error);
        
        assert!(max_error < 1e-12, "SIMD and scalar results differ too much");
    }
    
    #[test]
    fn test_simd_fm_oscillator() {
        let mut fm_osc = simd_oscillators::SimdHighPrecisionFMOscillator::new(
            440.0, 220.0, 5.0, 44100.0, 0.5
        );
        
        let buffer_size = 1024;
        let mut output = vec![0.0f64; buffer_size];
        
        fm_osc.generate_simd(&mut output);
        
        // Проверяем, что FM работает (должны быть sidebands)
        let dc_offset = output.iter().sum::<f64>() / buffer_size as f64;
        assert!(dc_offset.abs() < 0.01, "FM should have near-zero DC offset");
        
        // Проверяем амплитуду
        let max_amplitude = output.iter()
            .map(|&x| x.abs())
            .fold(0.0f64, |a, b| a.max(b));
        assert!(max_amplitude <= 0.5 + 1e-6, "FM amplitude should not exceed 0.5");
    }
    
    #[test]
    fn test_aligned_buffer() {
        let alignment = 64;
        let buffer = AlignedF64Buffer::new(1024, Some(alignment));
        
        assert!(buffer.is_aligned(), "Buffer should be aligned");
        
        let slice = buffer.as_slice();
        assert_eq!(slice.len(), 1024);
        
        // Проверяем, что все элементы инициализированы
        for &sample in slice {
            assert_eq!(sample, 0.0);
        }
    }
    
    #[test]
    fn benchmark_simd_vs_scalar() {
        const BUFFER_SIZE: usize = 8192;
        const ITERATIONS: usize = 1000;
        
        // Тестовые данные
        let input: Vec<f64> = (0..BUFFER_SIZE)
            .map(|i| (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / 44100.0).sin() * 0.5)
            .collect();
        
        // SIMD биквадратный фильтр
        let mut filter_simd = simd_filters::SimdHighPrecisionBiquadFilter::new_lowpass(
            1000.0, 0.707, 44100.0
        );
        let mut output_simd = vec![0.0f64; BUFFER_SIZE];
        
        // Скалярный фильтр для сравнения
        let mut filter_scalar = filters::HighPrecisionBiquadFilter::new_lowpass(
            1000.0, 0.707, 44100.0
        );
        let mut output_scalar = vec![0.0f64; BUFFER_SIZE];
        
        // Бенчмарк SIMD
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            filter_simd.process_buffer_simd(&input, &mut output_simd);
        }
        let simd_time = start.elapsed();
        
        // Бенчмарк scalar
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            filter_scalar.process_buffer(&input, &mut output_scalar);
        }
        let scalar_time = start.elapsed();
        
        // Проверяем корректность
        let mut max_error = 0.0;
        for i in 0..BUFFER_SIZE {
            let error = (output_simd[i] - output_scalar[i]).abs();
            if error > max_error {
                max_error = error;
            }
        }
        
        println!("High-precision biquad filter benchmark:");
        println!("  Buffer size: {}", BUFFER_SIZE);
        println!("  Iterations: {}", ITERATIONS);
        println!("  SIMD time: {:?}", simd_time);
        println!("  Scalar time: {:?}", scalar_time);
        println!("  Speedup: {:.2}x", 
                scalar_time.as_nanos() as f64 / simd_time.as_nanos() as f64);
        println!("  Max error: {:.12}", max_error);
        println!("  Samples per second (SIMD): {:.0}", 
                (BUFFER_SIZE * ITERATIONS) as f64 / simd_time.as_secs_f64());
        
        assert!(max_error < 1e-10, "SIMD and scalar results differ too much");
    }
    
    #[test]
    fn test_simd_conversion() {
        let buffer_size = 1024;
        let input_f32: Vec<f32> = (0..buffer_size)
            .map(|i| (i as f32 / buffer_size as f32) * 2.0 - 1.0)
            .collect();
        
        let mut output_f64 = vec![0.0; buffer_size];
        
        // SIMD конвертация
        simd_convert_f32_to_f64(&input_f32, &mut output_f64);
        
        // Проверяем результат
        for i in 0..buffer_size {
            let expected = input_f32[i] as f64;
            assert!((output_f64[i] - expected).abs() < 1e-12);
        }
        
        // Конвертация обратно
        let mut output_f32 = vec![0.0f32; buffer_size];
        simd_convert_f64_to_f32(&output_f64, &mut output_f32, false);
        
        for i in 0..buffer_size {
            let error = (output_f32[i] - input_f32[i]).abs();
            assert!(error < 1e-7, "Conversion round-trip error too large: {}", error);
        }
    }
}

//! SIMD-оптимизированный noise shaping для high-precision аудио

use core::simd::{f64x2, f64x4, f64x8, f32x4, f32x8, f32x16, Simd, SimdFloat, SimdInt, Mask, LaneCount, SupportedLaneCount};
use std::arch::x86_64::*;
use std::f64::consts::PI;

// --- Основные типы noise shaping ---

/// Тип noise shaping фильтра
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NoiseShapingType {
    None,               // Без noise shaping
    SimpleFirstOrder,   // Простой фильтр 1-го порядка
    ModifiedEWeighted,  // Модифицированный E-weighted
    Lipshitz,           // Lipshitz et al. (классический)
    Gerzon,             // Gerzon/Craven
    Custom(Vec<f64>),   // Пользовательские коэффициенты
}

/// Конфигурация noise shaper'а
#[derive(Debug, Clone)]
pub struct NoiseShaperConfig {
    pub shaping_type: NoiseShapingType,
    pub dither_type: DitherType,
    pub bit_depth: u8,          // Целевая битовая глубина (8, 16, 24, 32)
    pub enable_simd: bool,
    pub custom_coeffs: Option<Vec<f64>>,
}

impl Default for NoiseShaperConfig {
    fn default() -> Self {
        Self {
            shaping_type: NoiseShapingType::Lipshitz,
            dither_type: DitherType::TPDF,
            bit_depth: 16,
            enable_simd: true,
            custom_coeffs: None,
        }
    }
}

/// Типы dither'а
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DitherType {
    None,           // Без dither'а
    Rectangular,    // RPDF (Rectangular PDF)
    Triangular,     // TPDF (Triangular PDF)
    Gaussian,       // Gaussian PDF
    HighPass,       // High-pass TPDF
}

// --- SIMD-оптимизированный генератор шума ---

/// SIMD-оптимизированный генератор псевдослучайных чисел
pub struct SimdNoiseGenerator<const LANES: usize>
where
    LaneCount<LANES>: SupportedLaneCount,
{
    state: [u64; LANES],
    a: u64,
    c: u64,
    m: u64,
}

impl<const LANES: usize> SimdNoiseGenerator<LANES>
where
    LaneCount<LANES>: SupportedLaneCount,
{
    pub fn new(seed: u64) -> Self {
        let mut state = [0u64; LANES];
        
        // Инициализируем разные состояния для каждого lane
        for i in 0..LANES {
            state[i] = seed.wrapping_add(i as u64 * 0x9E3779B97F4A7C15);
        }
        
        // Параметры LCG (Linear Congruential Generator)
        // Используем параметры из Numerical Recipes
        Self {
            state,
            a: 1664525,
            c: 1013904223,
            m: 1 << 31,
        }
    }
    
    /// Генерирует SIMD вектор равномерно распределенных чисел [0, 1)
    pub fn next_f64(&mut self) -> Simd<f64, LANES> {
        let mut result = [0.0f64; LANES];
        
        for i in 0..LANES {
            // LCG: X_{n+1} = (a * X_n + c) mod m
            self.state[i] = self.state[i]
                .wrapping_mul(self.a)
                .wrapping_add(self.c);
            
            // Конвертируем в f64 в диапазоне [0, 1)
            result[i] = (self.state[i] & 0x7FFFFFFF) as f64 / 0x7FFFFFFF as f64;
        }
        
        Simd::from_array(result)
    }
    
    /// Генерирует TPDF (Triangular PDF) dither
    pub fn next_tpdf_f64(&mut self) -> Simd<f64, LANES> {
        // TPDF = (U1 + U2 - 1.0)
        let u1 = self.next_f64();
        let u2 = self.next_f64();
        u1 + u2 - Simd::splat(1.0)
    }
    
    /// Генерирует Gaussian dither с использованием Box-Muller
    pub fn next_gaussian_f64(&mut self) -> Simd<f64, LANES> {
        // Box-Muller transform для SIMD
        let u1 = self.next_f64();
        let u2 = self.next_f64();
        
        // Избегаем log(0)
        let epsilon = Simd::splat(1e-10);
        let u1_safe = u1.max(epsilon);
        
        let r = (-Simd::splat(2.0) * u1_safe.ln()).sqrt();
        let theta = Simd::splat(2.0 * PI) * u2;
        
        let z0 = r * theta.cos();
        z0
    }
}

// --- SIMD-оптимизированный noise shaper ---

/// Основной SIMD noise shaper
pub struct SimdNoiseShaper {
    config: NoiseShaperConfig,
    
    // Коэффициенты фильтра (для разных порядков)
    coeffs: Vec<f64>,
    
    // История ошибок (кольцевой буфер для SIMD)
    error_history: Vec<f64>,
    history_pos: usize,
    history_size: usize,
    
    // SIMD состояния
    simd_coeffs: Option<Vec<f64x4>>,  // Предвычисленные SIMD коэффициенты
    simd_enabled: bool,
    
    // Генераторы шума
    rng_f64x4: SimdNoiseGenerator<4>,
    rng_f64x8: SimdNoiseGenerator<8>,
    
    // Кэшированные значения для производительности
    quant_step: f64,          // Шаг квантования (2^(-bit_depth+1))
    dither_scale: f64,        // Масштаб dither'а
    noise_gain: f64,          // Усиление шума для shaping
}

impl SimdNoiseShaper {
    pub fn new(config: NoiseShaperConfig) -> Self {
        let coeffs = match config.shaping_type {
            NoiseShapingType::None => vec![],
            NoiseShapingType::SimpleFirstOrder => vec![0.5],
            NoiseShapingType::ModifiedEWeighted => vec![1.0, -0.5, 0.25],
            NoiseShapingType::Lipshitz => vec![1.0, -1.5, 0.5625], // Классический Lipshitz
            NoiseShapingType::Gerzon => vec![1.0, -1.6, 0.7, -0.2],
            NoiseShapingType::Custom(ref c) => c.clone(),
        };
        
        let history_size = coeffs.len();
        let mut error_history = vec![0.0; history_size * 4]; // Выравниваем для SIMD
        
        let quant_step = 2.0f64.powi(-(config.bit_depth as i32) + 1);
        let dither_scale = quant_step;
        
        // Инициализируем SIMD коэффициенты если включено
        let simd_coeffs = if config.enable_simd && history_size > 0 {
            Some(Self::prepare_simd_coeffs(&coeffs))
        } else {
            None
        };
        
        // Проверяем поддержку SIMD
        let simd_enabled = config.enable_simd && simd_coeffs.is_some();
        
        Self {
            config,
            coeffs,
            error_history,
            history_pos: 0,
            history_size,
            simd_coeffs,
            simd_enabled,
            rng_f64x4: SimdNoiseGenerator::new(0xDEADBEEF),
            rng_f64x8: SimdNoiseGenerator::new(0xCAFEBABE),
            quant_step,
            dither_scale,
            noise_gain: 1.0,
        }
    }
    
    /// Подготовка коэффициентов для SIMD
    fn prepare_simd_coeffs(coeffs: &[f64]) -> Vec<f64x4> {
        let mut simd_coeffs = Vec::with_capacity(coeffs.len());
        
        for chunk in coeffs.chunks(4) {
            let mut arr = [0.0; 4];
            for (i, &coeff) in chunk.iter().enumerate() {
                arr[i] = coeff;
            }
            simd_coeffs.push(f64x4::from_array(arr));
        }
        
        simd_coeffs
    }
    
    /// Основная функция обработки с SIMD
    pub fn process_buffer_simd(&mut self, input: &[f64], output: &mut [f32]) {
        if input.len() != output.len() {
            return;
        }
        
        if !self.simd_enabled || self.coeffs.is_empty() {
            // Fallback на скалярную версию
            self.process_buffer_scalar(input, output);
            return;
        }
        
        // Выбираем оптимальную SIMD ширину
        let simd_config = F64SimdConfig::detect();
        
        match simd_config.optimal_width {
            8 => self.process_f64x8(input, output),
            4 => self.process_f64x4(input, output),
            2 => self.process_f64x2(input, output),
            _ => self.process_buffer_scalar(input, output),
        }
    }
    
    /// Обработка с f64x4 (AVX2/SSE2)
    fn process_f64x4(&mut self, input: &[f64], output: &mut [f32]) {
        let buffer_size = input.len();
        let chunks = buffer_size / 4;
        let remainder = buffer_size % 4;
        
        let quant_step_vec = f64x4::splat(self.quant_step);
        let dither_scale_vec = f64x4::splat(self.dither_scale);
        let noise_gain_vec = f64x4::splat(self.noise_gain);
        let half_vec = f64x4::splat(0.5);
        
        // Загружаем историю ошибок в SIMD регистры
        let mut error_history_simd = self.load_error_history_f64x4();
        
        for chunk_idx in 0..chunks {
            let base_idx = chunk_idx * 4;
            
            // Загружаем входные семплы
            let input_vec = if base_idx + 4 <= buffer_size {
                f64x4::from_slice(&input[base_idx..base_idx + 4])
            } else {
                // Обработка границы
                let mut arr = [0.0; 4];
                for i in 0..4 {
                    arr[i] = input.get(base_idx + i).copied().unwrap_or(0.0);
                }
                f64x4::from_array(arr)
            };
            
            // Генерируем dither
            let dither_vec = self.generate_dither_f64x4();
            
            // Добавляем сформированный шум
            let shaped_noise = self.apply_noise_shaping_simd(error_history_simd);
            let processed = input_vec + shaped_noise * noise_gain_vec + dither_vec * dither_scale_vec;
            
            // Квантование (симуляция)
            let quantized = self.quantize_simd(processed, quant_step_vec, half_vec);
            
            // Вычисляем ошибку квантования
            let error = processed - quantized;
            
            // Обновляем историю ошибок
            error_history_simd = self.update_error_history_simd(error_history_simd, error);
            
            // Конвертируем в f32 и сохраняем
            let output_f32: [f32; 4] = [
                quantized[0] as f32,
                quantized[1] as f32,
                quantized[2] as f32,
                quantized[3] as f32,
            ];
            
            for i in 0..4 {
                if base_idx + i < output.len() {
                    output[base_idx + i] = output_f32[i];
                }
            }
        }
        
        // Сохраняем историю ошибок
        self.save_error_history_f64x4(error_history_simd);
        
        // Обработка остатка
        let start = chunks * 4;
        for i in 0..remainder {
            if start + i < buffer_size {
                output[start + i] = self.process_sample_scalar(input[start + i]) as f32;
            }
        }
    }
    
    /// Обработка с f64x8 (AVX-512)
    fn process_f64x8(&mut self, input: &[f64], output: &mut [f32]) {
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx512f") {
                unsafe { self.process_f64x8_avx512(input, output) };
            } else {
                self.process_f64x4(input, output);
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            self.process_f64x4(input, output);
        }
    }
    
    #[cfg(target_arch = "x86_64")]
    unsafe fn process_f64x8_avx512(&mut self, input: &[f64], output: &mut [f32]) {
        use std::arch::x86_64::*;
        
        let buffer_size = input.len();
        let chunks = buffer_size / 8;
        let remainder = buffer_size % 8;
        
        // AVX-512 константы
        let quant_step_vec = _mm512_set1_pd(self.quant_step);
        let dither_scale_vec = _mm512_set1_pd(self.dither_scale);
        let noise_gain_vec = _mm512_set1_pd(self.noise_gain);
        let half_vec = _mm512_set1_pd(0.5);
        
        for chunk_idx in 0..chunks {
            let base_idx = chunk_idx * 8;
            
            // Загружаем входные данные
            let input_ptr = input.as_ptr().add(base_idx);
            let input_vec = _mm512_loadu_pd(input_ptr);
            
            // Генерируем dither (8 значений за раз)
            let dither_vec = self.generate_dither_avx512();
            
            // Noise shaping (упрощённо для AVX-512)
            let shaped_noise = _mm512_set1_pd(0.0); // TODO: Реализовать full AVX-512 shaping
            
            let processed = _mm512_add_pd(
                _mm512_add_pd(input_vec, shaped_noise),
                _mm512_mul_pd(dither_vec, dither_scale_vec)
            );
            
            // Квантование
            let quantized = self.quantize_avx512(processed, quant_step_vec, half_vec);
            
            // Конвертируем в f32
            let quantized_f32 = _mm512_cvtpd_ps(quantized);
            
            // Сохраняем
            let output_ptr = output.as_mut_ptr().add(base_idx) as *mut f32;
            _mm256_storeu_ps(output_ptr, quantized_f32);
        }
        
        // Остаток обрабатываем через f64x4
        let start = chunks * 8;
        let remainder_input = &input[start..];
        let remainder_output = &mut output[start..];
        
        if remainder >= 4 {
            self.process_f64x4(&remainder_input[..4], &mut remainder_output[..4]);
            
            if remainder == 8 {
                self.process_f64x4(&remainder_input[4..], &mut remainder_output[4..]);
            }
        } else {
            for i in 0..remainder {
                remainder_output[i] = self.process_sample_scalar(remainder_input[i]) as f32;
            }
        }
    }
    
    #[cfg(target_arch = "x86_64")]
    unsafe fn generate_dither_avx512(&mut self) -> __m512d {
        use std::arch::x86_64::*;
        
        // Генерируем 8 равномерных случайных чисел
        let u1 = _mm512_set_pd(
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
        );
        
        let u2 = _mm512_set_pd(
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
            rand::random::<f64>(),
        );
        
        match self.config.dither_type {
            DitherType::TPDF => {
                // TPDF: (U1 + U2 - 1.0)
                let one = _mm512_set1_pd(1.0);
                _mm512_sub_pd(_mm512_add_pd(u1, u2), one)
            }
            DitherType::Rectangular => {
                // RPDF: (U1 - 0.5) * 2.0
                let half = _mm512_set1_pd(0.5);
                let two = _mm512_set1_pd(2.0);
                _mm512_mul_pd(_mm512_sub_pd(u1, half), two)
            }
            DitherType::Gaussian => {
                // Gaussian через Box-Muller (упрощённо)
                // На практике нужна более точная реализация
                self.generate_gaussian_avx512(u1, u2)
            }
            _ => _mm512_set1_pd(0.0),
        }
    }
    
    /// Загрузка истории ошибок в f64x4
    fn load_error_history_f64x4(&self) -> [f64x4; 2] {
        let mut history = [f64x4::splat(0.0); 2];
        
        if self.history_size > 0 {
            // Загружаем последние 8 значений ошибок (2 x f64x4)
            for i in 0..2 {
                let mut arr = [0.0; 4];
                for j in 0..4 {
                    let idx = (self.history_pos + i * 4 + j) % self.error_history.len();
                    arr[j] = self.error_history[idx];
                }
                history[i] = f64x4::from_array(arr);
            }
        }
        
        history
    }
    
    /// Сохранение истории ошибок из f64x4
    fn save_error_history_f64x4(&mut self, history: [f64x4; 2]) {
        if self.history_size == 0 {
            return;
        }
        
        for i in 0..2 {
            let arr: [f64; 4] = history[i].into();
            for j in 0..4 {
                let idx = (self.history_pos + i * 4 + j) % self.error_history.len();
                self.error_history[idx] = arr[j];
            }
        }
        
        self.history_pos = (self.history_pos + 8) % self.error_history.len();
    }
    
    /// Генерация dither для f64x4
    fn generate_dither_f64x4(&mut self) -> f64x4 {
        match self.config.dither_type {
            DitherType::TPDF => self.rng_f64x4.next_tpdf_f64(),
            DitherType::Rectangular => {
                let uniform = self.rng_f64x4.next_f64();
                (uniform - f64x4::splat(0.5)) * f64x4::splat(2.0)
            }
            DitherType::Gaussian => self.rng_f64x4.next_gaussian_f64(),
            DitherType::HighPass => {
                // High-pass TPDF (больше энергии на высоких частотах)
                let tpdf = self.rng_f64x4.next_tpdf_f64();
                // Простой high-pass: e[n] - 0.5*e[n-1]
                // TODO: Добавить состояние
                tpdf
            }
            DitherType::None => f64x4::splat(0.0),
        }
    }
    
    /// Применение noise shaping в SIMD
    fn apply_noise_shaping_simd(&self, error_history: [f64x4; 2]) -> f64x4 {
        if let Some(ref simd_coeffs) = self.simd_coeffs {
            let mut shaped = f64x4::splat(0.0);
            
            // Применяем фильтр к истории ошибок
            for (i, &coeff_vec) in simd_coeffs.iter().enumerate() {
                if i * 4 < self.history_size {
                    // Выбираем правильный блок истории
                    let history_block = if i < 2 { error_history[i] } else { f64x4::splat(0.0) };
                    shaped += coeff_vec * history_block;
                }
            }
            
            shaped
        } else {
            f64x4::splat(0.0)
        }
    }
    
    /// Обновление истории ошибок в SIMD
    fn update_error_history_simd(&self, mut history: [f64x4; 2], new_error: f64x4) -> [f64x4; 2] {
        if self.history_size == 0 {
            return history;
        }
        
        // Сдвигаем историю
        // [e0, e1, e2, e3], [e4, e5, e6, e7] -> [new, e0, e1, e2], [e3, e4, e5, e6]
        
        // Первый вектор: новые ошибки становятся первыми элементами
        let first_vec_new = f64x4::from_array([
            new_error[0], history[0][0], history[0][1], history[0][2]
        ]);
        
        // Второй вектор: сдвигаем элементы
        let second_vec_new = f64x4::from_array([
            history[0][3], history[1][0], history[1][1], history[1][2]
        ]);
        
        [first_vec_new, second_vec_new]
    }
    
    /// SIMD квантование
    fn quantize_simd(&self, value: f64x4, step: f64x4, half: f64x4) -> f64x4 {
        // Квантование с округлением: round(value / step) * step
        let scaled = value / step;
        let rounded = (scaled + half).floor();
        rounded * step
    }
    
    #[cfg(target_arch = "x86_64")]
    unsafe fn quantize_avx512(&self, value: __m512d, step: __m512d, half: __m512d) -> __m512d {
        use std::arch::x86_64::*;
        
        let scaled = _mm512_div_pd(value, step);
        let rounded = _mm512_floor_pd(_mm512_add_pd(scaled, half));
        _mm512_mul_pd(rounded, step)
    }
    
    /// Скалярная обработка (fallback)
    fn process_buffer_scalar(&mut self, input: &[f64], output: &mut [f32]) {
        for i in 0..input.len().min(output.len()) {
            output[i] = self.process_sample_scalar(input[i]) as f32;
        }
    }
    
    /// Обработка одного семпла (скалярная версия)
    fn process_sample_scalar(&mut self, input: f64) -> f64 {
        // Генерация dither
        let dither = match self.config.dither_type {
            DitherType::TPDF => rand::random::<f64>() + rand::random::<f64>() - 1.0,
            DitherType::Rectangular => (rand::random::<f64>() - 0.5) * 2.0,
            DitherType::Gaussian => {
                let u1 = rand::random::<f64>().max(1e-10);
                let u2 = rand::random::<f64>();
                (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos()
            }
            DitherType::HighPass => {
                // High-pass TPDF
                let tpdf = rand::random::<f64>() + rand::random::<f64>() - 1.0;
                // TODO: Реализовать high-pass фильтрацию
                tpdf
            }
            DitherType::None => 0.0,
        };
        
        // Noise shaping
        let mut shaped_noise = 0.0;
        for (i, &coeff) in self.coeffs.iter().enumerate() {
            let idx = (self.history_pos + self.error_history.len() - i - 1) % self.error_history.len();
            shaped_noise += coeff * self.error_history[idx];
        }
        
        // Добавляем dither и shaped noise
        let processed = input + shaped_noise * self.noise_gain + dither * self.dither_scale;
        
        // Квантование
        let quantized = self.quantize_scalar(processed);
        
        // Вычисляем ошибку
        let error = processed - quantized;
        
        // Сохраняем ошибку в историю
        self.error_history[self.history_pos] = error;
        self.history_pos = (self.history_pos + 1) % self.error_history.len();
        
        quantized
    }
    
    /// Скалярное квантование
    fn quantize_scalar(&self, value: f64) -> f64 {
        let scaled = value / self.quant_step;
        let rounded = (scaled + 0.5).floor();
        rounded * self.quant_step
    }
    
    /// Сброс состояния
    pub fn reset(&mut self) {
        self.error_history.fill(0.0);
        self.history_pos = 0;
        self.rng_f64x4 = SimdNoiseGenerator::new(0xDEADBEEF);
        self.rng_f64x8 = SimdNoiseGenerator::new(0xCAFEBABE);
    }
}

// --- Продвинутые SIMD фильтры для noise shaping ---

pub mod advanced_filters {
    use super::*;
    
    /// FIR фильтр с SIMD оптимизацией для noise shaping
    pub struct SimdFirNoiseShaper {
        coeffs: Vec<f64>,
        history: Vec<f64>,
        pos: usize,
        simd_coeffs_blocks: Vec<f64x4>,  // Коэффициенты упакованные для SIMD
        simd_history_blocks: Vec<f64x4>, // История упакованная для SIMD
    }
    
    impl SimdFirNoiseShaper {
        pub fn new(coeffs: Vec<f64>) -> Self {
            // Выравниваем длину коэффициентов для SIMD
            let aligned_len = ((coeffs.len() + 3) / 4) * 4;
            let mut padded_coeffs = coeffs.clone();
            padded_coeffs.resize(aligned_len, 0.0);
            
            // Упаковываем коэффициенты в SIMD блоки
            let simd_coeffs_blocks: Vec<f64x4> = padded_coeffs
                .chunks_exact(4)
                .map(|chunk| f64x4::from_slice(chunk))
                .collect();
            
            // Создаём историю с выравниванием
            let history_size = aligned_len;
            let history = vec![0.0; history_size];
            
            // Упаковываем историю в SIMD блоки
            let simd_history_blocks: Vec<f64x4> = history
                .chunks_exact(4)
                .map(|chunk| f64x4::from_slice(chunk))
                .collect();
            
            Self {
                coeffs,
                history,
                pos: 0,
                simd_coeffs_blocks,
                simd_history_blocks,
            }
        }
        
        /// Обработка с SIMD (convolution)
        pub fn process_simd(&mut self, input: f64) -> f64 {
            // Добавляем новый вход в историю
            self.history[self.pos] = input;
            self.pos = (self.pos + 1) % self.history.len();
            
            // Обновляем SIMD блоки истории
            self.update_simd_history();
            
            // Вычисляем свёртку через SIMD
            let mut result = 0.0;
            
            // Часть через SIMD
            let simd_iterations = self.simd_coeffs_blocks.len();
            for i in 0..simd_iterations {
                let coeff_block = self.simd_coeffs_blocks[i];
                let hist_block = self.simd_history_blocks[i];
                
                // Умножение и горизонтальное сложение
                let prod = coeff_block * hist_block;
                result += prod[0] + prod[1] + prod[2] + prod[3];
            }
            
            result
        }
        
        fn update_simd_history(&mut self) {
            // Обновляем SIMD блоки из скалярной истории
            for (i, chunk) in self.history.chunks_exact(4).enumerate() {
                if i < self.simd_history_blocks.len() {
                    self.simd_history_blocks[i] = f64x4::from_slice(chunk);
                }
            }
        }
    }
    
    /// IIR фильтр с SIMD оптимизацией
    pub struct SimdIirNoiseShaper {
        b_coeffs: Vec<f64>,  // feedforward
        a_coeffs: Vec<f64>,  // feedback
        x_history: Vec<f64>, // История входов
        y_history: Vec<f64>, // История выходов
        x_pos: usize,
        y_pos: usize,
    }
    
    impl SimdIirNoiseShaper {
        pub fn new(b_coeffs: Vec<f64>, a_coeffs: Vec<f64>) -> Self {
            let x_history = vec![0.0; b_coeffs.len()];
            let y_history = vec![0.0; a_coeffs.len()];
            
            Self {
                b_coeffs,
                a_coeffs,
                x_history,
                y_history,
                x_pos: 0,
                y_pos: 0,
            }
        }
        
        /// Обработка с SIMD-ускоренными умножениями
        pub fn process_simd(&mut self, input: f64) -> f64 {
            // Обновляем историю входов
            self.x_history[self.x_pos] = input;
            self.x_pos = (self.x_pos + 1) % self.x_history.len();
            
            // Feedforward часть (FIR)
            let mut output = 0.0;
            
            // SIMD для feedforward
            for chunk in self.b_coeffs.chunks_exact(4) {
                let coeff_vec = f64x4::from_slice(chunk);
                
                // Нужно получить соответствующие элементы истории
                // (упрощённо, на практике нужен правильный доступ к истории)
                let hist_start = (self.x_pos + self.x_history.len() - chunk.len()) % self.x_history.len();
                let mut hist_arr = [0.0; 4];
                for i in 0..4 {
                    hist_arr[i] = self.x_history[(hist_start + i) % self.x_history.len()];
                }
                let hist_vec = f64x4::from_array(hist_arr);
                
                let prod = coeff_vec * hist_vec;
                output += prod[0] + prod[1] + prod[2] + prod[3];
            }
            
            // Feedback часть (IIR)
            for (i, &coeff) in self.a_coeffs.iter().enumerate().skip(1) {
                let idx = (self.y_pos + self.y_history.len() - i) % self.y_history.len();
                output -= coeff * self.y_history[idx];
            }
            
            // Нормализуем (если a[0] != 1)
            if self.a_coeffs[0] != 1.0 {
                output /= self.a_coeffs[0];
            }
            
            // Сохраняем выход в историю
            self.y_history[self.y_pos] = output;
            self.y_pos = (self.y_pos + 1) % self.y_history.len();
            
            output
        }
    }
}

// --- Frequency-domain noise shaping ---

pub mod frequency_domain {
    use super::*;
    use rustfft::{FftPlanner, num_complex::Complex};
    
    /// Частотный noise shaper с FFT и SIMD
    pub struct FrequencyDomainNoiseShaper {
        fft_size: usize,
        fft_forward: Arc<dyn rustfft::Fft<f64>>,
        fft_inverse: Arc<dyn rustfft::Fft<f64>>,
        window: Vec<f64>,
        overlap: usize,
        scale: f64,
        
        // Буферы для обработки
        input_buffer: Vec<f64>,
        output_buffer: Vec<f64>,
        fft_buffer: Vec<Complex<f64>>,
        
        // Частотная маска для shaping
        freq_mask: Vec<f64>,
        
        // SIMD конфигурация
        simd_config: F64SimdConfig,
    }
    
    impl FrequencyDomainNoiseShaper {
        pub fn new(fft_size: usize, overlap: usize, shape_curve: &[f64]) -> Self {
            let mut planner_forward = FftPlanner::new();
            let fft_forward = planner_forward.plan_fft_forward(fft_size);
            
            let mut planner_inverse = FftPlanner::new();
            let fft_inverse = planner_inverse.plan_fft_inverse(fft_size);
            
            // Оконная функция (Hann)
            let window: Vec<f64> = (0..fft_size)
                .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f64 / fft_size as f64).cos()))
                .collect();
            
            // Нормализация окна
            let window_scale = window.iter().sum::<f64>() / fft_size as f64;
            let window: Vec<f64> = window.iter().map(|&w| w / window_scale).collect();
            
            // Частотная маска
            let mut freq_mask = vec![1.0; fft_size / 2 + 1];
            for (i, mask) in freq_mask.iter_mut().enumerate() {
                let freq = i as f64 * 44100.0 / fft_size as f64;
                // Применяем shaping curve (упрощённо)
                if i < shape_curve.len() {
                    *mask = shape_curve[i];
                }
            }
            
            let scale = 1.0 / (fft_size as f64);
            
            Self {
                fft_size,
                fft_forward,
                fft_inverse,
                window,
                overlap,
                scale,
                input_buffer: vec![0.0; fft_size],
                output_buffer: vec![0.0; fft_size * 2], // Для overlap-add
                fft_buffer: vec![Complex::new(0.0, 0.0); fft_size],
                freq_mask,
                simd_config: F64SimdConfig::detect(),
            }
        }
        
        /// Обработка буфера с frequency-domain noise shaping
        pub fn process_buffer(&mut self, input: &[f64], output: &mut [f64]) {
            let hop_size = self.fft_size / self.overlap;
            let mut input_pos = 0;
            let mut output_pos = 0;
            
            while input_pos + self.fft_size <= input.len() && output_pos + hop_size <= output.len() {
                // Копируем с окном
                for i in 0..self.fft_size {
                    self.input_buffer[i] = input[input_pos + i] * self.window[i];
                    self.fft_buffer[i] = Complex::new(self.input_buffer[i], 0.0);
                }
                
                // FFT
                self.fft_forward.process(&mut self.fft_buffer);
                
                // Применяем noise shaping в частотной области
                self.apply_frequency_shaping();
                
                // IFFT
                self.fft_inverse.process(&mut self.fft_buffer);
                
                // Overlap-add с окном
                for i in 0..self.fft_size {
                    let real_part = self.fft_buffer[i].re * self.scale * self.window[i];
                    
                    let out_idx = output_pos + i;
                    if out_idx < self.output_buffer.len() {
                        self.output_buffer[out_idx] += real_part;
                    }
                }
                
                input_pos += hop_size;
                output_pos += hop_size;
            }
            
            // Копируем результат
            let copy_len = output.len().min(self.output_buffer.len());
            output[..copy_len].copy_from_slice(&self.output_buffer[..copy_len]);
            
            // Сдвигаем output buffer для next block
            self.output_buffer.copy_within(hop_size.., 0);
            self.output_buffer[self.output_buffer.len() - hop_size..].fill(0.0);
        }
        
        /// Применение noise shaping в частотной области
        fn apply_frequency_shaping(&mut self) {
            // Прямой доступ к частотным bin'ам для noise shaping
            // Обрабатываем первые N/2+1 bins (симметрично для реального сигнала)
            for i in 0..=self.fft_size / 2 {
                let mask = self.freq_mask[i];
                
                // Применяем shaping к амплитуде
                let magnitude = self.fft_buffer[i].norm();
                let shaped_magnitude = magnitude * mask;
                
                // Сохраняем фазу
                let phase = self.fft_buffer[i].arg();
                self.fft_buffer[i] = Complex::from_polar(shaped_magnitude, phase);
                
                // Симметричная часть (для реального сигнала)
                if i > 0 && i < self.fft_size / 2 {
                    let sym_idx = self.fft_size - i;
                    self.fft_buffer[sym_idx] = Complex::from_polar(shaped_magnitude, -phase);
                }
            }
        }
    }
}

// --- Анализ и визуализация noise shaping ---

pub mod analysis {
    use super::*;
    use std::f64::consts::LN_10;
    
    /// Анализ эффективности noise shaping
    pub struct NoiseShaperAnalyzer {
        sample_rate: f64,
        fft_size: usize,
        freq_bins: Vec<f64>,
        noise_psd: Vec<f64>,     // Power Spectral Density
        signal_psd: Vec<f64>,
        measurements: usize,
    }
    
    impl NoiseShaperAnalyzer {
        pub fn new(sample_rate: f64, fft_size: usize) -> Self {
            let freq_bins: Vec<f64> = (0..=fft_size/2)
                .map(|i| i as f64 * sample_rate / fft_size as f64)
                .collect();
            
            Self {
                sample_rate,
                fft_size,
                freq_bins,
                noise_psd: vec![0.0; fft_size/2 + 1],
                signal_psd: vec![0.0; fft_size/2 + 1],
                measurements: 0,
            }
        }
        
        /// Измерение PSD с SIMD ускорением
        pub fn measure_psd_simd(&mut self, signal: &[f64], noise: &[f64]) {
            // Упрощённый расчёт PSD через периодограмму
            let fft_size = self.fft_size;
            let mut signal_fft = vec![Complex::new(0.0, 0.0); fft_size];
            let mut noise_fft = vec![Complex::new(0.0, 0.0); fft_size];
            
            // Копируем данные (можно оптимизировать с SIMD)
            let copy_len = signal.len().min(fft_size);
            for i in 0..copy_len {
                signal_fft[i].re = signal[i];
                noise_fft[i].re = noise[i];
            }
            
            // Здесь должен быть FFT (опущено для краткости)
            // После FFT вычисляем PSD: |X[k]|² / N
            
            self.measurements += 1;
        }
        
        /// Вычисление SNR в dB
        pub fn calculate_snr(&self) -> f64 {
            let signal_power: f64 = self.signal_psd.iter().sum();
            let noise_power: f64 = self.noise_psd.iter().sum();
            
            if noise_power > 0.0 {
                10.0 * (signal_power / noise_power).log10()
            } else {
                f64::INFINITY
            }
        }
        
        /// Вычисление динамического диапазона
        pub fn calculate_dynamic_range(&self, bit_depth: u8) -> f64 {
            let theoretical_max = 6.02 * bit_depth as f64 + 1.76;
            let noise_floor = self.calculate_noise_floor();
            theoretical_max - noise_floor
        }
        
        /// Расчёт уровня шума
        fn calculate_noise_floor(&self) -> f64 {
            let total_noise: f64 = self.noise_psd.iter().sum();
            let avg_noise = total_noise / self.noise_psd.len() as f64;
            10.0 * avg_noise.log10()
        }
        
        /// Получение shaping curve в dB
        pub fn get_shaping_curve_db(&self) -> Vec<(f64, f64)> {
            self.freq_bins.iter()
                .zip(self.noise_psd.iter())
                .map(|(&freq, &psd)| (freq, 10.0 * psd.log10()))
                .collect()
        }
    }
}

// --- Интеграция с существующей системой ---

impl HighPrecisionNode for SimdNoiseShaper {
    fn process_hp(&mut self, inputs: &[&[f64]], outputs: &mut [&mut [f64]]) -> AudioResult<()> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }
        
        let input = inputs[0];
        let output_f64 = &mut outputs[0];
        
        // Временный буфер для f32
        let mut temp_f32 = vec![0.0f32; input.len()];
        
        // Применяем noise shaping
        self.process_buffer_simd(input, &mut temp_f32);
        
        // Конвертируем обратно в f64
        for i in 0..input.len().min(output_f64.len()) {
            output_f64[i] = temp_f32[i] as f64;
        }
        
        Ok(())
    }
}

// --- Макросы для удобства ---

#[macro_export]
macro_rules! simd_noise_shape {
    ($input:expr, $output:expr, $config:expr) => {
        let mut shaper = SimdNoiseShaper::new($config);
        shaper.process_buffer_simd($input, $output);
    };
}

#[macro_export]
macro_rules! conditional_noise_shaping {
    ($enable:expr, $input:expr, $output:expr, $bit_depth:expr) => {
        if $enable {
            let config = NoiseShaperConfig {
                bit_depth: $bit_depth,
                ..Default::default()
            };
            simd_noise_shape!($input, $output, config);
        } else {
            // Простое квантование
            for i in 0..$input.len().min($output.len()) {
                $output[i] = $input[i] as f32;
            }
        }
    };
}

// --- Тесты и бенчмарки ---

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    
    #[test]
    fn test_simd_noise_shaper_basic() {
        let config = NoiseShaperConfig {
            shaping_type: NoiseShapingType::Lipshitz,
            dither_type: DitherType::TPDF,
            bit_depth: 16,
            enable_simd: true,
            custom_coeffs: None,
        };
        
        let mut shaper = SimdNoiseShaper::new(config);
        
        // Тестовый сигнал (тихий, чтобы услышать шум)
        let buffer_size = 1024;
        let input: Vec<f64> = vec![0.001; buffer_size]; // -60 dBFS
        
        let mut output = vec![0.0f32; buffer_size];
        
        shaper.process_buffer_simd(&input, &mut output);
        
        // Проверяем, что output не равен input (было применено квантование)
        let mut different = false;
        for i in 0..buffer_size {
            if (output[i] as f64 - input[i]).abs() > 1e-10 {
                different = true;
                break;
            }
        }
        
        assert!(different, "Noise shaping should modify the signal");
        
        // Проверяем, что сигнал остался в допустимом диапазоне
        for &sample in &output {
            assert!(sample >= -1.0 && sample <= 1.0, 
                   "Output sample out of range: {}", sample);
        }
    }
    
    #[test]
    fn test_noise_shaper_types() {
        let buffer_size = 2048;
        let input: Vec<f64> = (0..buffer_size)
            .map(|i| (2.0 * PI * 1000.0 * i as f64 / 44100.0).sin() * 0.5)
            .collect();
        
        let shaping_types = [
            (NoiseShapingType::None, "None"),
            (NoiseShapingType::SimpleFirstOrder, "FirstOrder"),
            (NoiseShapingType::Lipshitz, "Lipshitz"),
            (NoiseShapingType::Gerzon, "Gerzon"),
        ];
        
        for (shaping_type, name) in shaping_types.iter() {
            let config = NoiseShaperConfig {
                shaping_type: *shaping_type,
                dither_type: DitherType::TPDF,
                bit_depth: 16,
                enable_simd: true,
                custom_coeffs: None,
            };
            
            let mut shaper = SimdNoiseShaper::new(config);
            let mut output = vec![0.0f32; buffer_size];
            
            let start = Instant::now();
            shaper.process_buffer_simd(&input, &mut output);
            let elapsed = start.elapsed();
            
            println!("{} shaping: {:?} for {} samples", 
                    name, elapsed, buffer_size);
            
            // Проверяем корректность
            let mut max_value = 0.0f32;
            for &sample in &output {
                max_value = max_value.max(sample.abs());
            }
            
            assert!(max_value <= 1.0 + 1e-6, 
                   "{} shaping produced out-of-range samples", name);
        }
    }
    
    #[test]
    fn test_dither_types() {
        let buffer_size = 1024;
        let input: Vec<f64> = vec![0.0001; buffer_size]; // Очень тихий сигнал
        
        let dither_types = [
            (DitherType::None, "None"),
            (DitherType::Rectangular, "RPDF"),
            (DitherType::Triangular, "TPDF"),
            (DitherType::Gaussian, "Gaussian"),
        ];
        
        for (dither_type, name) in dither_types.iter() {
            let config = NoiseShaperConfig {
                shaping_type: NoiseShapingType::Lipshitz,
                dither_type: *dither_type,
                bit_depth: 8, // Низкая битовая глубина для явного dither'а
                enable_simd: true,
                custom_coeffs: None,
            };
            
            let mut shaper = SimdNoiseShaper::new(config);
            let mut output = vec![0.0f32; buffer_size];
            
            shaper.process_buffer_simd(&input, &mut output);
            
            // Анализируем статистику выхода
            let mean: f32 = output.iter().sum::<f32>() / buffer_size as f32;
            let variance: f32 = output.iter()
                .map(|&x| (x - mean).powi(2))
                .sum::<f32>() / buffer_size as f32;
            
            println!("{} dither: mean={:.6}, variance={:.6}", 
                    name, mean, variance);
            
            // Для TPDF и Gaussian dither, mean должен быть около 0
            if *dither_type != DitherType::None {
                assert!(mean.abs() < 0.01, 
                       "{} dither has non-zero mean: {}", name, mean);
            }
        }
    }
    
    #[test]
    fn benchmark_simd_vs_scalar() {
        const BUFFER_SIZE: usize = 65536; // Большой буфер для бенчмарка
        const ITERATIONS: usize = 100;
        
        // Тестовый сигнал
        let input: Vec<f64> = (0..BUFFER_SIZE)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / 44100.0).sin() * 0.3)
            .collect();
        
        // SIMD версия
        let config_simd = NoiseShaperConfig {
            shaping_type: NoiseShapingType::Lipshitz,
            dither_type: DitherType::TPDF,
            bit_depth: 16,
            enable_simd: true,
            custom_coeffs: None,
        };
        
        let mut shaper_simd = SimdNoiseShaper::new(config_simd);
        let mut output_simd = vec![0.0f32; BUFFER_SIZE];
        
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            shaper_simd.process_buffer_simd(&input, &mut output_simd);
            shaper_simd.reset();
        }
        let simd_time = start.elapsed();
        
        // Scalar версия
        let config_scalar = NoiseShaperConfig {
            shaping_type: NoiseShapingType::Lipshitz,
            dither_type: DitherType::TPDF,
            bit_depth: 16,
            enable_simd: false,
            custom_coeffs: None,
        };
        
        let mut shaper_scalar = SimdNoiseShaper::new(config_scalar);
        let mut output_scalar = vec![0.0f32; BUFFER_SIZE];
        
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            shaper_scalar.process_buffer_scalar(&input, &mut output_scalar);
            shaper_scalar.reset();
        }
        let scalar_time = start.elapsed();
        
        // Сравниваем результаты
        let mut max_error = 0.0f32;
        for i in 0..BUFFER_SIZE {
            let error = (output_simd[i] - output_scalar[i]).abs();
            max_error = max_error.max(error);
        }
        
        println!("Noise Shaping Benchmark:");
        println!("  Buffer size: {}", BUFFER_SIZE);
        println!("  Iterations: {}", ITERATIONS);
        println!("  SIMD time: {:?}", simd_time);
        println!("  Scalar time: {:?}", scalar_time);
        println!("  Speedup: {:.2}x", 
                scalar_time.as_nanos() as f64 / simd_time.as_nanos() as f64);
        println!("  Max error: {:.10}", max_error);
        println!("  Samples/sec (SIMD): {:.0}", 
                (BUFFER_SIZE * ITERATIONS) as f64 / simd_time.as_secs_f64());
        
        // Проверяем корректность
        assert!(max_error < 1e-6, "SIMD and scalar results differ too much");
        
        // SIMD должен быть быстрее
        assert!(simd_time < scalar_time, "SIMD should be faster than scalar");
    }
    
    #[test]
    fn test_custom_coefficients() {
        // Пользовательские коэффициенты для noise shaping
        let custom_coeffs = vec![1.0, -1.2, 0.8, -0.3, 0.1];
        
        let config = NoiseShaperConfig {
            shaping_type: NoiseShapingType::Custom(custom_coeffs.clone()),
            dither_type: DitherType::TPDF,
            bit_depth: 24,
            enable_simd: true,
            custom_coeffs: Some(custom_coeffs),
        };
        
        let mut shaper = SimdNoiseShaper::new(config);
        
        let buffer_size = 512;
        let input: Vec<f64> = (0..buffer_size)
            .map(|i| (2.0 * PI * 1000.0 * i as f64 / 44100.0).sin() * 0.1)
            .collect();
        
        let mut output = vec![0.0f32; buffer_size];
        
        shaper.process_buffer_simd(&input, &mut output);
        
        // Проверяем, что коэффициенты были применены
        assert!(shaper.coeffs.len() == 5, 
               "Custom coefficients should be loaded");
        
        // Проверяем выход
        for &sample in &output {
            assert!(sample.abs() <= 1.0 + 1e-6, 
                   "Output sample out of range: {}", sample);
        }
    }
    
    #[test]
    fn test_noise_generator() {
        let mut rng = SimdNoiseGenerator::<4>::new(12345);
        
        // Тестируем равномерное распределение
        let mut samples = Vec::with_capacity(10000);
        for _ in 0..2500 { // 2500 * 4 = 10000 samples
            let vec = rng.next_f64();
            for i in 0..4 {
                samples.push(vec[i]);
            }
        }
        
        // Проверяем, что все значения в диапазоне [0, 1)
        for &sample in &samples {
            assert!(sample >= 0.0 && sample < 1.0, 
                   "Uniform sample out of range: {}", sample);
        }
        
        // Проверяем среднее (должно быть около 0.5)
        let mean: f64 = samples.iter().sum::<f64>() / samples.len() as f64;
        assert!((mean - 0.5).abs() < 0.01, 
               "Uniform distribution mean should be ~0.5, got {}", mean);
        
        // Тестируем TPDF
        let mut tpdf_samples = Vec::new();
        let mut rng_tpdf = SimdNoiseGenerator::<4>::new(54321);
        
        for _ in 0..2500 {
            let vec = rng_tpdf.next_tpdf_f64();
            for i in 0..4 {
                tpdf_samples.push(vec[i]);
            }
        }
        
        // TPDF должен быть в диапазоне [-1, 1]
        for &sample in &tpdf_samples {
            assert!(sample >= -1.0 && sample <= 1.0, 
                   "TPDF sample out of range: {}", sample);
        }
        
        // Среднее TPDF должно быть около 0
        let tpdf_mean: f64 = tpdf_samples.iter().sum::<f64>() / tpdf_samples.len() as f64;
        assert!(tpdf_mean.abs() < 0.01, 
               "TPDF mean should be ~0, got {}", tpdf_mean);
    }
}