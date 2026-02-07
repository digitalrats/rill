//! Продвинутая SIMD поддержка для аудиообработки
//! Автоматическое определение CPU возможностей, fallback стратегии

use core::simd::{
    f32x4, f32x8, f32x16, f64x2, f64x4, f64x8,
    Simd, SimdFloat, SimdInt, SimdUint, LaneCount, SupportedLaneCount
};
use std::arch::is_x86_feature_detected;
use std::sync::Arc;
use parking_lot::RwLock;

// --- Улучшенная конфигурация SIMD ---

#[derive(Debug, Clone, Copy)]
pub struct AdvancedSimdConfig {
    pub arch: SimdArchitecture,
    pub f32_lanes: usize,
    pub f64_lanes: usize,
    pub supports_fma: bool,
    pub supports_avx512: bool,
    pub supports_neon: bool,
    pub supports_sve: bool,      // ARM SVE
    pub supports_vsx: bool,      // PowerPC VSX
    pub optimal_alignment: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimdArchitecture {
    X86SSE,
    X86AVX,
    X86AVX2,
    X86AVX512,
    ARMNeon,
    ARMSve,
    PowerPCVSX,
    RISCVV,
    WasmSimd128,
    Generic,  // Fallback без специфичных инструкций
}

impl AdvancedSimdConfig {
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        let (sse, avx, avx2, avx512, fma) = unsafe {
            (
                is_x86_feature_detected!("sse"),
                is_x86_feature_detected!("avx"),
                is_x86_feature_detected!("avx2"),
                is_x86_feature_detected!("avx512f"),
                is_x86_feature_detected!("fma"),
            )
        };
        
        #[cfg(not(target_arch = "x86_64"))]
        let (sse, avx, avx2, avx512, fma) = (false, false, false, false, false);
        
        #[cfg(target_arch = "aarch64")]
        let (neon, sve) = unsafe {
            use std::arch::is_aarch64_feature_detected;
            (
                is_aarch64_feature_detected!("neon"),
                is_aarch64_feature_detected!("sve"),
            )
        };
        
        #[cfg(not(target_arch = "aarch64"))]
        let (neon, sve) = (false, false);
        
        #[cfg(target_arch = "powerpc64")]
        let vsx = true; // Упрощённо, на практике нужна проверка
        
        #[cfg(not(target_arch = "powerpc64"))]
        let vsx = false;
        
        #[cfg(target_arch = "wasm32")]
        let wasm_simd = true;
        
        #[cfg(not(target_arch = "wasm32"))]
        let wasm_simd = false;
        
        // Определяем архитектуру и оптимальную ширину
        let (arch, f32_lanes, f64_lanes) = if avx512 {
            (SimdArchitecture::X86AVX512, 16, 8)
        } else if avx2 {
            (SimdArchitecture::X86AVX2, 8, 4)
        } else if avx {
            (SimdArchitecture::X86AVX, 8, 4)
        } else if neon {
            (SimdArchitecture::ARMNeon, 4, 2)
        } else if sve {
            // SVE имеет переменную длину вектора
            (SimdArchitecture::ARMSve, 4, 2) // Минимальная гарантированная
        } else if vsx {
            (SimdArchitecture::PowerPCVSX, 4, 2)
        } else if wasm_simd {
            (SimdArchitecture::WasmSimd128, 4, 2)
        } else if sse {
            (SimdArchitecture::X86SSE, 4, 2)
        } else {
            (SimdArchitecture::Generic, 1, 1)
        };
        
        let alignment = match arch {
            SimdArchitecture::X86AVX512 => 64,
            SimdArchitecture::X86AVX2 | SimdArchitecture::X86AVX => 32,
            _ => 16,
        };
        
        Self {
            arch,
            f32_lanes,
            f64_lanes,
            supports_fma: fma,
            supports_avx512: avx512,
            supports_neon: neon,
            supports_sve: sve,
            supports_vsx: vsx,
            optimal_alignment: alignment,
        }
    }
    
    pub fn is_simd_available(&self) -> bool {
        self.f32_lanes > 1
    }
    
    pub fn recommended_buffer_size(&self) -> usize {
        // Рекомендуемый размер буфера для оптимального выравнивания
        self.f32_lanes * 4
    }
    
    pub fn get_simd_width_for_type<T>(&self) -> usize {
        match std::mem::size_of::<T>() {
            4 => self.f32_lanes, // f32
            8 => self.f64_lanes, // f64
            _ => 1,
        }
    }
}

// --- Динамический SIMD диспетчер ---

pub struct SimdDispatcher {
    config: AdvancedSimdConfig,
    runtime_config: RwLock<RuntimeSimdConfig>,
    performance_monitor: Arc<PerformanceMonitor>,
}

#[derive(Debug, Clone)]
pub struct RuntimeSimdConfig {
    pub enabled: bool,
    pub auto_detect: bool,
    pub prefer_f32: bool,        // Предпочитать f32 даже если доступен f64 SIMD
    pub use_fma: bool,
    pub aggressive_unrolling: bool,
    pub cache_aligned_buffers: bool,
}

impl SimdDispatcher {
    pub fn new() -> Self {
        let config = AdvancedSimdConfig::detect();
        
        Self {
            config,
            runtime_config: RwLock::new(RuntimeSimdConfig {
                enabled: config.is_simd_available(),
                auto_detect: true,
                prefer_f32: false,
                use_fma: true,
                aggressive_unrolling: true,
                cache_aligned_buffers: true,
            }),
            performance_monitor: Arc::new(PerformanceMonitor::new()),
        }
    }
    
    /// Выполняет операцию с автоматическим выбором SIMD реализации
    pub fn execute<T, R>(
        &self,
        operation: SimdOperation,
        data: &[T],
        output: &mut [T],
    ) -> Result<R, SimdError>
    where
        T: SimdElement,
        R: Default,
    {
        if !self.runtime_config.read().enabled {
            return self.execute_scalar(operation, data, output);
        }
        
        match self.config.arch {
            SimdArchitecture::X86AVX512 => self.execute_avx512(operation, data, output),
            SimdArchitecture::X86AVX2 => self.execute_avx2(operation, data, output),
            SimdArchitecture::X86AVX => self.execute_avx(operation, data, output),
            SimdArchitecture::ARMNeon => self.execute_neon(operation, data, output),
            SimdArchitecture::WasmSimd128 => self.execute_wasm(operation, data, output),
            _ => self.execute_scalar(operation, data, output),
        }
    }
    
    // Реализации для каждой архитектуры...
}

// --- Улучшенные выровненные буферы ---

pub struct CacheAlignedBuffer<T> {
    data: Vec<T>,
    alignment: usize,
    capacity: usize,
}

impl<T> CacheAlignedBuffer<T> {
    pub fn new(size: usize, alignment: Option<usize>) -> Self {
        let config = AdvancedSimdConfig::detect();
        let alignment = alignment.unwrap_or(config.optimal_alignment);
        
        // Выделяем с запасом для выравнивания
        let total_size = size + alignment / std::mem::size_of::<T>();
        let mut data = Vec::with_capacity(total_size);
        
        // Выравниваем указатель
        let ptr = data.as_mut_ptr() as usize;
        let offset = ptr.align_offset(alignment);
        
        if offset > 0 {
            unsafe { data.set_len(offset) };
        }
        
        data.reserve(size);
        unsafe { data.set_len(size) };
        
        Self {
            data,
            alignment,
            capacity: size,
        }
    }
    
    pub fn as_simd_slice<const LANES: usize>(&self) -> &[Simd<T, LANES>]
    where
        LaneCount<LANES>: SupportedLaneCount,
        T: SimdElement,
    {
        let ptr = self.data.as_ptr() as *const Simd<T, LANES>;
        let len = self.data.len() / LANES;
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
    
    pub fn as_simd_slice_mut<const LANES: usize>(&mut self) -> &mut [Simd<T, LANES>]
    where
        LaneCount<LANES>: SupportedLaneCount,
        T: SimdElement,
    {
        let ptr = self.data.as_mut_ptr() as *mut Simd<T, LANES>;
        let len = self.data.len() / LANES;
        unsafe { std::slice::from_raw_parts_mut(ptr, len) }
    }
    
    pub fn is_aligned(&self) -> bool {
        (self.data.as_ptr() as usize) % self.alignment == 0
    }
}

// --- SIMD-оптимизированные DSP операции ---

pub mod dsp {
    use super::*;
    
    /// SIMD-оптимизированный биквадратный фильтр (Direct Form II)
    pub struct SimdBiquadFilter<const LANES: usize>
    where
        LaneCount<LANES>: SupportedLaneCount,
    {
        coeffs: SimdBiquadCoeffs<LANES>,
        state: SimdBiquadState<LANES>,
        sample_rate: f32,
    }
    
    #[derive(Debug, Clone, Copy)]
    pub struct SimdBiquadCoeffs<const LANES: usize> {
        pub b0: Simd<f32, LANES>,
        pub b1: Simd<f32, LANES>,
        pub b2: Simd<f32, LANES>,
        pub a1: Simd<f32, LANES>,
        pub a2: Simd<f32, LANES>,
    }
    
    #[derive(Debug, Clone, Copy)]
    pub struct SimdBiquadState<const LANES: usize> {
        pub z1: Simd<f32, LANES>,
        pub z2: Simd<f32, LANES>,
    }
    
    impl<const LANES: usize> SimdBiquadFilter<LANES>
    where
        LaneCount<LANES>: SupportedLaneCount,
    {
        pub fn new_lowpass(cutoff: Simd<f32, LANES>, q: Simd<f32, LANES>, sample_rate: f32) -> Self {
            let omega = Simd::splat(2.0 * std::f32::consts::PI) * cutoff / Simd::splat(sample_rate);
            let alpha = omega.sin() / (Simd::splat(2.0) * q);
            
            let cos_omega = omega.cos();
            
            let b0 = (Simd::splat(1.0) - cos_omega) / Simd::splat(2.0);
            let b1 = Simd::splat(1.0) - cos_omega;
            let b2 = b0;
            let a0 = Simd::splat(1.0) + alpha;
            let a1 = Simd::splat(-2.0) * cos_omega;
            let a2 = Simd::splat(1.0) - alpha;
            
            Self {
                coeffs: SimdBiquadCoeffs {
                    b0: b0 / a0,
                    b1: b1 / a0,
                    b2: b2 / a0,
                    a1: a1 / a0,
                    a2: a2 / a0,
                },
                state: SimdBiquadState {
                    z1: Simd::splat(0.0),
                    z2: Simd::splat(0.0),
                },
                sample_rate,
            }
        }
        
        pub fn process_vector(&mut self, input: Simd<f32, LANES>) -> Simd<f32, LANES> {
            // Direct Form II в SIMD
            let output = input * self.coeffs.b0 + self.state.z1;
            
            self.state.z1 = input * self.coeffs.b1 + self.state.z2 
                - output * self.coeffs.a1;
            self.state.z2 = input * self.coeffs.b2 
                - output * self.coeffs.a2;
            
            output
        }
        
        pub fn process_buffer(&mut self, input: &[f32], output: &mut [f32]) {
            let chunks = input.chunks_exact(LANES);
            let remainder = chunks.remainder();
            
            for (i, chunk) in chunks.enumerate() {
                let input_vec = Simd::from_slice(chunk);
                let output_vec = self.process_vector(input_vec);
                output_vec.copy_to_slice(&mut output[i*LANES..(i+1)*LANES]);
            }
            
            // Остаток обрабатываем скалярно
            let start = input.len() - remainder.len();
            for i in 0..remainder.len() {
                // Скалярная версия для остатка
                let x = input[start + i];
                let y = x * self.coeffs.b0[0] + self.state.z1[0];
                self.state.z1[0] = x * self.coeffs.b1[0] + self.state.z2[0] - y * self.coeffs.a1[0];
                self.state.z2[0] = x * self.coeffs.b2[0] - y * self.coeffs.a2[0];
                output[start + i] = y;
            }
        }
    }
    
    /// SIMD-оптимизированный oversampling интерполятор
    pub struct SimdOversampler<const LANES: usize, const OVERSAMPLING: usize>
    where
        LaneCount<LANES>: SupportedLaneCount,
    {
        halfband_filters: Vec<SimdBiquadFilter<LANES>>,
        temp_buffer: CacheAlignedBuffer<f32>,
        interpolation_coeffs: [Simd<f32, LANES>; 4], // Для кубической интерполяции
    }
    
    impl<const LANES: usize, const OVERSAMPLING: usize> SimdOversampler<LANES, OVERSAMPLING>
    where
        LaneCount<LANES>: SupportedLaneCount,
    {
        pub fn new(sample_rate: f32) -> Self {
            let mut halfband_filters = Vec::new();
            
            // Создаём каскад half-band фильтров для oversampling
            for i in 0..(OVERSAMPLING.ilog2() as usize) {
                let cutoff = sample_rate * 0.45 * (1 << i) as f32;
                let filter = SimdBiquadFilter::new_lowpass(
                    Simd::splat(cutoff),
                    Simd::splat(0.707),
                    sample_rate * (1 << (i + 1)) as f32,
                );
                halfband_filters.push(filter);
            }
            
            // Коэффициенты для кубической интерполяции (Catmull-Rom)
            let interpolation_coeffs = [
                Simd::splat(-0.5),
                Simd::splat(1.5),
                Simd::splat(-1.5),
                Simd::splat(0.5),
            ];
            
            Self {
                halfband_filters,
                temp_buffer: CacheAlignedBuffer::new(4096 * OVERSAMPLING, None),
                interpolation_coeffs,
            }
        }
        
        pub fn upsample(&mut self, input: &[f32], output: &mut [f32]) {
            let os_factor = OVERSAMPLING;
            let output_len = input.len() * os_factor;
            
            if output.len() < output_len {
                return;
            }
            
            // Zero insertion с SIMD
            for (i, &sample) in input.iter().enumerate() {
                let base_idx = i * os_factor;
                output[base_idx] = sample;
                
                // Заполняем нулями с SIMD ускорением
                let zero_chunks = (base_idx + 1..base_idx + os_factor)
                    .collect::<Vec<_>>()
                    .chunks(LANES);
                
                for chunk in zero_chunks {
                    for &idx in chunk {
                        if idx < output.len() {
                            output[idx] = 0.0;
                        }
                    }
                }
            }
            
            // Применяем half-band фильтры с SIMD
            for filter in &mut self.halfband_filters {
                filter.process_buffer(&output[..output_len], output);
            }
        }
    }
    
    /// SIMD-оптимизированный конвертер форматов
    pub struct SimdFormatConverter {
        config: AdvancedSimdConfig,
        dither_generator: Option<SimdDitherGenerator>,
    }
    
    impl SimdFormatConverter {
        pub fn new(enable_dither: bool) -> Self {
            let config = AdvancedSimdConfig::detect();
            let dither_generator = if enable_dither {
                Some(SimdDitherGenerator::new())
            } else {
                None
            };
            
            Self {
                config,
                dither_generator,
            }
        }
        
        /// Конвертация f64 → f32 с SIMD и noise shaping
        pub fn convert_f64_to_f32(&mut self, input: &[f64], output: &mut [f32]) {
            match self.config.arch {
                SimdArchitecture::X86AVX512 => self.convert_f64_to_f32_avx512(input, output),
                SimdArchitecture::X86AVX2 => self.convert_f64_to_f32_avx2(input, output),
                SimdArchitecture::ARMNeon => self.convert_f64_to_f32_neon(input, output),
                _ => self.convert_f64_to_f32_scalar(input, output),
            }
        }
        
        /// Конвертация f32 → f64 с SIMD
        pub fn convert_f32_to_f64(&self, input: &[f32], output: &mut [f64]) {
            match self.config.arch {
                SimdArchitecture::X86AVX512 => self.convert_f32_to_f64_avx512(input, output),
                SimdArchitecture::X86AVX2 => self.convert_f32_to_f64_avx2(input, output),
                SimdArchitecture::ARMNeon => self.convert_f32_to_f64_neon(input, output),
                _ => self.convert_f32_to_f64_scalar(input, output),
            }
        }
    }
}

// --- SIMD-оптимизированные осцилляторы ---

pub mod oscillators {
    use super::*;
    
    /// Полифонический SIMD осциллятор (обрабатывает несколько нот параллельно)
    pub struct PolyphonicSimdOscillator<const LANES: usize, const VOICES: usize>
    where
        LaneCount<LANES>: SupportedLaneCount,
        [(); VOICES / LANES]:,
    {
        phases: [Simd<f32, LANES>; VOICES / LANES],
        frequencies: [Simd<f32, LANES>; VOICES / LANES],
        amplitudes: [Simd<f32, LANES>; VOICES / LANES],
        waveforms: [SimdWaveform; VOICES / LANES],
        sample_rate: f32,
        two_pi: Simd<f32, LANES>,
    }
    
    #[derive(Debug, Clone, Copy)]
    pub enum SimdWaveform {
        Sine,
        Saw,
        Square { pulse_width: Simd<f32, LANES> },
        Triangle,
        Noise,
    }
    
    impl<const LANES: usize, const VOICES: usize> PolyphonicSimdOscillator<LANES, VOICES>
    where
        LaneCount<LANES>: SupportedLaneCount,
        [(); VOICES / LANES]:,
    {
        pub fn new(sample_rate: f32) -> Self {
            Self {
                phases: [Simd::splat(0.0); VOICES / LANES],
                frequencies: [Simd::splat(440.0); VOICES / LANES],
                amplitudes: [Simd::splat(1.0); VOICES / LANES],
                waveforms: [SimdWaveform::Sine; VOICES / LANES],
                sample_rate,
                two_pi: Simd::splat(2.0 * std::f32::consts::PI),
            }
        }
        
        pub fn set_frequency(&mut self, voice_idx: usize, frequency: f32) {
            let lane = voice_idx / LANES;
            let sub_idx = voice_idx % LANES;
            self.frequencies[lane][sub_idx] = frequency;
        }
        
        pub fn set_waveform(&mut self, voice_idx: usize, waveform: SimdWaveform) {
            let lane = voice_idx / LANES;
            self.waveforms[lane] = waveform;
        }
        
        pub fn generate_block(&mut self, output: &mut [f32]) {
            let block_size = output.len();
            let frames_per_lane = block_size / (VOICES / LANES);
            
            for frame in 0..frames_per_lane {
                for lane in 0..(VOICES / LANES) {
                    let phase_increment = self.frequencies[lane] / Simd::splat(self.sample_rate);
                    self.phases[lane] += phase_increment;
                    
                    // Нормализуем фазу
                    let mask = self.phases[lane].simd_ge(Simd::splat(1.0));
                    self.phases[lane] = self.phases[lane] - mask.select(Simd::splat(1.0), Simd::splat(0.0));
                    
                    // Генерируем волну
                    let sample = match self.waveforms[lane] {
                        SimdWaveform::Sine => (self.phases[lane] * self.two_pi).sin(),
                        SimdWaveform::Saw => self.phases[lane] * Simd::splat(2.0) - Simd::splat(1.0),
                        SimdWaveform::Square { pulse_width } => {
                            let mask = self.phases[lane].simd_lt(pulse_width);
                            mask.select(Simd::splat(1.0), Simd::splat(-1.0))
                        }
                        SimdWaveform::Triangle => {
                            let phase = self.phases[lane] * Simd::splat(4.0);
                            let mask1 = phase.simd_lt(Simd::splat(2.0));
                            let mask2 = phase.simd_lt(Simd::splat(1.0));
                            let mask3 = phase.simd_lt(Simd::splat(3.0));
                            
                            let a = phase - Simd::splat(1.0);
                            let b = Simd::splat(3.0) - phase;
                            
                            mask1.select(
                                mask2.select(phase, a),
                                mask3.select(b, phase - Simd::splat(4.0))
                            )
                        }
                        SimdWaveform::Noise => {
                            // Генерация псевдослучайного шума с SIMD
                            let mut rng = SimdRandom::new();
                            rng.next_f32()
                        }
                    };
                    
                    let output_sample = sample * self.amplitudes[lane];
                    
                    // Сохраняем результат
                    let output_idx = frame * (VOICES / LANES) + lane;
                    if output_idx < output.len() {
                        output[output_idx] = output_sample.reduce_sum();
                    }
                }
            }
        }
    }
    
    /// SIMD-оптимизированный FM осциллятор
    pub struct SimdFMOscillator<const LANES: usize>
    where
        LaneCount<LANES>: SupportedLaneCount,
    {
        carrier_phase: Simd<f32, LANES>,
        modulator_phase: Simd<f32, LANES>,
        carrier_freq: Simd<f32, LANES>,
        modulator_freq: Simd<f32, LANES>,
        modulation_index: Simd<f32, LANES>,
        amplitude: Simd<f32, LANES>,
        sample_rate: f32,
    }
    
    impl<const LANES: usize> SimdFMOscillator<LANES>
    where
        LaneCount<LANES>: SupportedLaneCount,
    {
        pub fn new(
            carrier_freq: Simd<f32, LANES>,
            modulator_freq: Simd<f32, LANES>,
            modulation_index: Simd<f32, LANES>,
            amplitude: Simd<f32, LANES>,
            sample_rate: f32,
        ) -> Self {
            Self {
                carrier_phase: Simd::splat(0.0),
                modulator_phase: Simd::splat(0.0),
                carrier_freq,
                modulator_freq,
                modulation_index,
                amplitude,
                sample_rate,
            }
        }
        
        pub fn generate(&mut self, output: &mut [f32]) {
            let two_pi = Simd::splat(2.0 * std::f32::consts::PI);
            let sample_rate_vec = Simd::splat(self.sample_rate);
            
            for out in output.iter_mut() {
                // Модулирующий сигнал
                let modulation = (self.modulator_phase * two_pi).sin() * self.modulation_index;
                
                // Модулированная несущая
                let modulated_phase = self.carrier_phase * two_pi + modulation;
                *out = modulated_phase.sin() * self.amplitude.reduce_sum();
                
                // Обновляем фазы
                self.carrier_phase += self.carrier_freq / sample_rate_vec;
                self.modulator_phase += self.modulator_freq / sample_rate_vec;
                
                // Нормализуем фазы
                let carrier_mask = self.carrier_phase.simd_ge(Simd::splat(1.0));
                let modulator_mask = self.modulator_phase.simd_ge(Simd::splat(1.0));
                
                self.carrier_phase = self.carrier_phase 
                    - carrier_mask.select(Simd::splat(1.0), Simd::splat(0.0));
                self.modulator_phase = self.modulator_phase 
                    - modulator_mask.select(Simd::splat(1.0), Simd::splat(0.0));
            }
        }
    }
}

// --- SIMD-оптимизированные эффекты ---

pub mod effects {
    use super::*;
    
    /// SIMD delay эффект с интерполяцией
    pub struct SimdDelay<const LANES: usize>
    where
        LaneCount<LANES>: SupportedLaneCount,
    {
        buffer: CacheAlignedBuffer<f32>,
        write_pos: usize,
        delay_samples: Simd<f32, LANES>,
        feedback: Simd<f32, LANES>,
        mix: Simd<f32, LANES>,
        sample_rate: f32,
    }
    
    impl<const LANES: usize> SimdDelay<LANES>
    where
        LaneCount<LANES>: SupportedLaneCount,
    {
        pub fn new(max_delay_seconds: f32, sample_rate: f32) -> Self {
            let buffer_size = (max_delay_seconds * sample_rate) as usize;
            let buffer_size = ((buffer_size + LANES - 1) / LANES) * LANES; // Выравниваем
            
            Self {
                buffer: CacheAlignedBuffer::new(buffer_size, None),
                write_pos: 0,
                delay_samples: Simd::splat(sample_rate * 0.5), // 500ms по умолчанию
                feedback: Simd::splat(0.6),
                mix: Simd::splat(0.5),
                sample_rate,
            }
        }
        
        pub fn process(&mut self, input: Simd<f32, LANES>) -> Simd<f32, LANES> {
            // Читаем задержанный сигнал с интерполяцией
            let delayed = self.read_delayed();
            
            // Смешиваем dry/wet
            let wet = delayed * self.mix;
            let dry = input * (Simd::splat(1.0) - self.mix);
            let output = wet + dry;
            
            // Пишем в буфер с feedback
            let feedback_signal = delayed * self.feedback;
            self.write_to_buffer(input + feedback_signal);
            
            output
        }
        
        fn read_delayed(&self) -> Simd<f32, LANES> {
            let buffer_len = self.buffer.data.len();
            let mut result = Simd::splat(0.0);
            
            for i in 0..LANES {
                let delay = self.delay_samples[i];
                let read_pos_f = self.write_pos as f32 - delay;
                
                if read_pos_f < 0.0 {
                    continue;
                }
                
                let read_pos = read_pos_f as usize % buffer_len;
                let frac = read_pos_f.fract();
                
                // Линейная интерполяция
                let idx1 = read_pos % buffer_len;
                let idx2 = (read_pos + 1) % buffer_len;
                
                let s1 = self.buffer.data[idx1];
                let s2 = self.buffer.data[idx2];
                
                result[i] = s1 + frac * (s2 - s1);
            }
            
            result
        }
        
        fn write_to_buffer(&mut self, samples: Simd<f32, LANES>) {
            let buffer_len = self.buffer.data.len();
            
            for i in 0..LANES {
                let idx = (self.write_pos + i) % buffer_len;
                self.buffer.data[idx] = samples[i];
            }
            
            self.write_pos = (self.write_pos + LANES) % buffer_len;
        }
    }
    
    /// SIMD-оптимизированный биткрашер
    pub struct SimdBitCrusher<const LANES: usize>
    where
        LaneCount<LANES>: SupportedLaneCount,
    {
        bit_depth: Simd<u32, LANES>,
        reduction_factor: Simd<f32, LANES>,
        last_samples: Simd<f32, LANES>,
        sample_counters: Simd<u32, LANES>,
    }
    
    impl<const LANES: usize> SimdBitCrusher<LANES>
    where
        LaneCount<LANES>: SupportedLaneCount,
    {
        pub fn new(bit_depth: u32, reduction_factor: f32) -> Self {
            Self {
                bit_depth: Simd::splat(bit_depth),
                reduction_factor: Simd::splat(reduction_factor),
                last_samples: Simd::splat(0.0),
                sample_counters: Simd::splat(0),
            }
        }
        
        pub fn process(&mut self, input: Simd<f32, LANES>) -> Simd<f32, LANES> {
            // Sample rate reduction
            let should_update = self.sample_counters.simd_ge(
                (Simd::splat(1.0) / self.reduction_factor).cast()
            );
            
            let new_samples = should_update.select(
                self.quantize(input),
                self.last_samples
            );
            
            // Обновляем счётчики
            self.sample_counters = should_update.select(
                Simd::splat(0),
                self.sample_counters + Simd::splat(1)
            );
            
            self.last_samples = new_samples;
            new_samples
        }
        
        fn quantize(&self, samples: Simd<f32, LANES>) -> Simd<f32, LANES> {
            let steps = Simd::splat(1u32 << 24) >> (Simd::splat(24) - self.bit_depth);
            let steps_f = steps.cast::<f32>();
            
            let scaled = (samples * steps_f).round();
            scaled / steps_f
        }
    }
}

// --- Мониторинг производительности ---

pub struct PerformanceMonitor {
    cycles_per_sample: RwLock<Vec<f64>>,
    memory_usage: RwLock<Vec<usize>>,
    simd_efficiency: RwLock<f64>,
}

impl PerformanceMonitor {
    pub fn new() -> Self {
        Self {
            cycles_per_sample: RwLock::new(Vec::new()),
            memory_usage: RwLock::new(Vec::new()),
            simd_efficiency: RwLock::new(1.0),
        }
    }
    
    pub fn record_processing_time(&self, samples: usize, nanoseconds: u64) {
        let cps = nanoseconds as f64 / samples as f64;
        self.cycles_per_sample.write().push(cps);
        
        // Сохраняем только последние 1000 измерений
        if self.cycles_per_sample.read().len() > 1000 {
            self.cycles_per_sample.write().remove(0);
        }
    }
    
    pub fn calculate_efficiency(&self, scalar_time: u64, simd_time: u64) -> f64 {
        if simd_time > 0 {
            let efficiency = scalar_time as f64 / simd_time as f64;
            *self.simd_efficiency.write() = efficiency;
            efficiency
        } else {
            1.0
        }
    }
}

// --- Макросы для удобства ---

#[macro_export]
macro_rules! simd_select {
    ($arch:expr, $sse:expr, $avx:expr, $avx2:expr, $avx512:expr, $neon:expr, $generic:expr) => {
        match $arch {
            SimdArchitecture::X86SSE => $sse,
            SimdArchitecture::X86AVX => $avx,
            SimdArchitecture::X86AVX2 => $avx2,
            SimdArchitecture::X86AVX512 => $avx512,
            SimdArchitecture::ARMNeon => $neon,
            SimdArchitecture::ARMSve => $neon, // fallback на Neon
            _ => $generic,
        }
    };
}

#[macro_export]
macro_rules! simd_auto {
    ($input:expr, $output:expr, $operation:ident) => {
        let config = AdvancedSimdConfig::detect();
        match config.arch {
            SimdArchitecture::X86AVX512 => $operation::<16>($input, $output),
            SimdArchitecture::X86AVX2 => $operation::<8>($input, $output),
            SimdArchitecture::X86AVX => $operation::<8>($input, $output),
            SimdArchitecture::ARMNeon => $operation::<4>($input, $output),
            SimdArchitecture::X86SSE => $operation::<4>($input, $output),
            _ => $operation::<1>($input, $output),
        }
    };
}

// --- Пример использования ---

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simd_config() {
        let config = AdvancedSimdConfig::detect();
        println!("SIMD Configuration:");
        println!("  Architecture: {:?}", config.arch);
        println!("  f32 lanes: {}", config.f32_lanes);
        println!("  f64 lanes: {}", config.f64_lanes);
        println!("  FMA: {}", config.supports_fma);
        println!("  Alignment: {} bytes", config.optimal_alignment);
        
        assert!(config.f32_lanes >= 1);
    }
    
    #[test]
    fn test_simd_biquad() {
        let config = AdvancedSimdConfig::detect();
        
        // Тестируем на разных SIMD ширинах
        match config.f32_lanes {
            16 => test_biquad::<16>(),
            8 => test_biquad::<8>(),
            4 => test_biquad::<4>(),
            _ => test_biquad::<1>(),
        }
    }
    
    fn test_biquad<const LANES: usize>()
    where
        LaneCount<LANES>: SupportedLaneCount,
    {
        let mut filter = dsp::SimdBiquadFilter::<LANES>::new_lowpass(
            Simd::splat(1000.0),
            Simd::splat(0.707),
            44100.0,
        );
        
        let input = vec![1.0f32; 1024];
        let mut output = vec![0.0f32; 1024];
        
        filter.process_buffer(&input, &mut output);
        
        // Проверяем, что фильтр работает
        assert!(output[0].abs() > 0.0);
        assert!(output.iter().any(|&x| x != 0.0));
        
        println!("Biquad test passed for LANES={}", LANES);
    }
    
    #[test]
    fn test_simd_oscillator() {
        const LANES: usize = 4;
        const VOICES: usize = 16;
        
        let mut osc = oscillators::PolyphonicSimdOscillator::<LANES, VOICES>::new(44100.0);
        
        // Настраиваем разные частоты
        for i in 0..VOICES {
            osc.set_frequency(i, 440.0 * (i + 1) as f32);
        }
        
        let mut output = vec![0.0f32; 1024];
        osc.generate_block(&mut output);
        
        // Проверяем, что осциллятор генерирует сигнал
        assert!(output.iter().any(|&x| x != 0.0));
        
        let max_amplitude = output.iter()
            .map(|&x| x.abs())
            .fold(0.0f32, |a, b| a.max(b));
        
        assert!(max_amplitude <= 1.0 + 1e-6);
        
        println!("Polyphonic SIMD oscillator test passed");
    }
    
    #[test]
    fn benchmark_simd_vs_scalar() {
        use std::time::Instant;
        
        const BUFFER_SIZE: usize = 65536;
        const ITERATIONS: usize = 100;
        
        let config = AdvancedSimdConfig::detect();
        
        if !config.is_simd_available() {
            println!("SIMD not available, skipping benchmark");
            return;
        }
        
        // Тестовые данные
        let input: Vec<f32> = (0..BUFFER_SIZE)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 44100.0).sin() * 0.5)
            .collect();
        
        // SIMD фильтр
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            match config.f32_lanes {
                16 => benchmark_filter::<16>(&input),
                8 => benchmark_filter::<8>(&input),
                4 => benchmark_filter::<4>(&input),
                _ => benchmark_filter::<1>(&input),
            }
        }
        let simd_time = start.elapsed();
        
        // Scalar фильтр
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            benchmark_filter::<1>(&input);
        }
        let scalar_time = start.elapsed();
        
        let speedup = scalar_time.as_nanos() as f64 / simd_time.as_nanos() as f64;
        
        println!("SIMD vs Scalar Benchmark:");
        println!("  Architecture: {:?}", config.arch);
        println!("  SIMD lanes: {}", config.f32_lanes);
        println!("  Buffer size: {}", BUFFER_SIZE);
        println!("  Iterations: {}", ITERATIONS);
        println!("  SIMD time: {:?}", simd_time);
        println!("  Scalar time: {:?}", scalar_time);
        println!("  Speedup: {:.2}x", speedup);
        println!("  Samples/sec (SIMD): {:.0}", 
                (BUFFER_SIZE * ITERATIONS) as f64 / simd_time.as_secs_f64());
        
        // SIMD должен быть быстрее
        assert!(simd_time < scalar_time || config.f32_lanes == 1);
    }
    
    fn benchmark_filter<const LANES: usize>(input: &[f32]) 
    where
        LaneCount<LANES>: SupportedLaneCount,
    {
        let mut filter = dsp::SimdBiquadFilter::<LANES>::new_lowpass(
            Simd::splat(1000.0),
            Simd::splat(0.707),
            44100.0,
        );
        
        let mut output = vec![0.0f32; input.len()];
        filter.process_buffer(input, &mut output);
    }
}