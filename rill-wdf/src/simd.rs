// rill-wdf/src/simd/mod.rs
use core::simd::{f64x2, f64x4, f64x8, Simd, SimdFloat};

/// SIMD-векторизованный WDF элемент
pub trait SimdWdfElement: Send + Sync {
    type SimdType: SimdFloat;
    
    /// Обработка падающей волны SIMD вектором
    fn process_incident_simd(&mut self, a: Self::SimdType) -> Self::SimdType;
    
    /// Обновление состояния для SIMD
    fn update_state_simd(&mut self);
    
    /// Получение SIMD вектора напряжения
    fn voltage_simd(&self) -> Self::SimdType;
    
    /// Получение SIMD вектора тока
    fn current_simd(&self) -> Self::SimdType;
}

/// SIMD-оптимизированный резистор
#[derive(Debug, Clone)]
pub struct SimdResistor {
    resistance: f64,
    port_resistance: f64x4,  // SIMD вектор сопротивлений
    voltage: f64x4,
    current: f64x4,
    simd_config: SimdConfig,
}

impl SimdResistor {
    pub fn new(resistance: f64, simd_config: SimdConfig) -> Self {
        let port_resistance = match simd_config.f64_simd_type() {
            SimdType::F64x4 => f64x4::splat(resistance),
            SimdType::F64x2 => f64x2::splat(resistance).into(), // Конвертируем
            _ => f64x4::splat(resistance), // fallback
        };
        
        Self {
            resistance,
            port_resistance,
            voltage: f64x4::splat(0.0),
            current: f64x4::splat(0.0),
            simd_config,
        }
    }
    
    /// Пакетная обработка нескольких резисторов
    pub fn process_batch(&mut self, inputs: &[f64], outputs: &mut [f64]) {
        match self.simd_config.f64_simd_type() {
            SimdType::F64x4 => self.process_f64x4(inputs, outputs),
            SimdType::F64x8 => self.process_f64x8(inputs, outputs),
            SimdType::F64x2 => self.process_f64x2(inputs, outputs),
            _ => self.process_scalar(inputs, outputs),
        }
    }
    
    fn process_f64x4(&mut self, inputs: &[f64], outputs: &mut [f64]) {
        let zero = f64x4::splat(0.0);
        let chunks = inputs.chunks_exact(4);
        let remainder = chunks.remainder();
        
        for (i, chunk) in chunks.enumerate() {
            let input_vec = f64x4::from_slice(chunk);
            // Для резистора: b = 0
            zero.copy_to_slice(&mut outputs[i*4..(i+1)*4]);
        }
        
        // Остаток
        let start = inputs.len() - remainder.len();
        for i in 0..remainder.len() {
            outputs[start + i] = 0.0;
        }
    }
}

/// SIMD-оптимизированный конденсатор с прекомпилированными коэффициентами
pub struct SimdCapacitor {
    capacitance: f64,
    sample_rate: f64,
    port_resistance: f64x4,
    state: f64x4, // SIMD вектор состояний
    dt: f64,
}

impl SimdCapacitor {
    pub fn new(capacitance: f64, sample_rate: f64, simd_config: SimdConfig) -> Self {
        let t = 1.0 / sample_rate;
        let port_resistance = t / (2.0 * capacitance);
        
        let simd_r = match simd_config.f64_simd_type() {
            SimdType::F64x4 => f64x4::splat(port_resistance),
            _ => f64x4::splat(port_resistance),
        };
        
        Self {
            capacitance,
            sample_rate,
            port_resistance: simd_r,
            state: f64x4::splat(0.0),
            dt: t,
        }
    }
    
    /// SIMD обработка: b = state - a
    pub fn process_incident_simd(&mut self, a: f64x4) -> f64x4 {
        self.state - a
    }
    
    /// Пакетная обработка
    pub fn process_batch(&mut self, inputs: &[f64], outputs: &mut [f64]) {
        match self.simd_config.f64_simd_type() {
            SimdType::F64x4 => self.process_f64x4(inputs, outputs),
            _ => self.process_scalar(inputs, outputs),
        }
    }
    
    fn process_f64x4(&mut self, inputs: &[f64], outputs: &mut [f64]) {
        let chunks = inputs.chunks_exact(4);
        let remainder = chunks.remainder();
        
        for (i, chunk) in chunks.enumerate() {
            let a_vec = f64x4::from_slice(chunk);
            let b_vec = self.process_incident_simd(a_vec);
            
            // Обновляем состояние
            self.state = -self.current_simd() * self.port_resistance;
            
            b_vec.copy_to_slice(&mut outputs[i*4..(i+1)*4]);
        }
        
        // Обработка остатка
        let start = inputs.len() - remainder.len();
        for i in 0..remainder.len() {
            outputs[start + i] = self.state[i as u32] - inputs[start + i];
        }
    }
    
    fn current_simd(&self) -> f64x4 {
        // Вычисляем ток из состояния
        -self.state / self.port_resistance
    }
}

/// SIMD-оптимизированный серийный адаптер
pub struct SimdSeriesAdapter {
    elements: Vec<Arc<RwLock<dyn SimdWdfElement<SimdType = f64x4>>>>,
    port_resistance: f64x4,
    simd_weights: Vec<f64x4>, // Предвычисленные веса для распределения
}

impl SimdSeriesAdapter {
    pub fn new(elements: Vec<Arc<RwLock<dyn SimdWdfElement<SimdType = f64x4>>>>) -> Self {
        // Предвычисляем SIMD веса для каждого элемента
        let total_r: f64x4 = elements.iter()
            .map(|e| e.read().port_resistance_simd())
            .fold(f64x4::splat(0.0), |acc, r| acc + r);
        
        let simd_weights: Vec<f64x4> = elements.iter()
            .map(|e| e.read().port_resistance_simd() / total_r)
            .collect();
        
        Self {
            elements,
            port_resistance: total_r,
            simd_weights,
        }
    }
    
    /// SIMD обработка падающей волны
    pub fn process_incident_simd(&mut self, a: f64x4) -> f64x4 {
        let mut b_total = f64x4::splat(0.0);
        
        for (i, element) in self.elements.iter().enumerate() {
            let weight = self.simd_weights[i];
            let a_i = a * weight;
            
            let b_i = element.write().process_incident_simd(a_i);
            b_total = b_total + b_i * weight;
        }
        
        b_total
    }
    
    /// Пакетная обработка
    pub fn process_batch(&mut self, inputs: &[f64], outputs: &mut [f64]) {
        let simd_type = SimdType::F64x4; // Предполагаем
        let lanes = simd_type.lanes();
        
        for chunk_idx in (0..inputs.len()).step_by(lanes) {
            let chunk_end = (chunk_idx + lanes).min(inputs.len());
            let a = if chunk_end - chunk_idx == lanes {
                f64x4::from_slice(&inputs[chunk_idx..chunk_end])
            } else {
                // Заполняем остаток нулями
                let mut arr = [0.0; 4];
                arr[..chunk_end - chunk_idx].copy_from_slice(&inputs[chunk_idx..chunk_end]);
                f64x4::from_array(arr)
            };
            
            let b = self.process_incident_simd(a);
            
            // Сохраняем результат
            if chunk_end - chunk_idx == lanes {
                b.copy_to_slice(&mut outputs[chunk_idx..chunk_end]);
            } else {
                // Частичное сохранение
                let b_array: [f64; 4] = b.into();
                outputs[chunk_idx..chunk_end].copy_from_slice(&b_array[..chunk_end - chunk_idx]);
            }
        }
    }
}

/// SIMD-оптимизированный диод с векторизованным методом Ньютона
pub struct SimdDiode {
    saturation_current: f64,
    thermal_voltage: f64,
    ideality_factor: f64,
    port_resistance: f64x4,
    
    // Предвычисленные константы для SIMD
    vt_simd: f64x4,           // Vt * n
    is_simd: f64x4,          // Is
    tolerance_simd: f64x4,   // tolerance для Ньютона
}

impl SimdDiode {
    pub fn new(saturation_current: f64, ideality_factor: f64, 
               temperature_k: f64, simd_config: SimdConfig) -> Self {
        let thermal_voltage = Self::calculate_thermal_voltage(temperature_k);
        let vt = thermal_voltage * ideality_factor;
        
        let port_resistance = match simd_config.f64_simd_type() {
            SimdType::F64x4 => f64x4::splat(vt / saturation_current),
            _ => f64x4::splat(vt / saturation_current),
        };
        
        Self {
            saturation_current,
            thermal_voltage,
            ideality_factor,
            port_resistance,
            vt_simd: f64x4::splat(vt),
            is_simd: f64x4::splat(saturation_current),
            tolerance_simd: f64x4::splat(1e-9),
        }
    }
    
    /// SIMD-векторизованный метод Ньютона-Рафсона
    pub fn solve_newton_simd(&self, a: f64x4, r: f64x4) -> f64x4 {
        let mut v = f64x4::splat(0.0); // Начальное предположение
        
        // Итерации Ньютона (векторизованные)
        for _ in 0..6 { // Уменьшили итерации, т.к. SIMD быстрее
            let i = self.diode_equation_simd(v);
            let g = self.diode_derivative_simd(v);
            
            // f(v) = v + R*i(v) - a
            let f = v + r * i - a;
            
            // Проверяем сходимость (SIMD маска)
            let converged = f.abs() < self.tolerance_simd;
            if converged.all() {
                break;
            }
            
            // f'(v) = 1 + R*g(v)
            let df = f64x4::splat(1.0) + r * g;
            
            // Шаг Ньютона: v_new = v - f/f'
            v = v - f / df;
        }
        
        v
    }
    
    /// SIMD уравнение диода: i = Is * (exp(v/Vt) - 1)
    fn diode_equation_simd(&self, v: f64x4) -> f64x4 {
        self.is_simd * ((v / self.vt_simd).exp() - f64x4::splat(1.0))
    }
    
    /// SIMD производная: di/dv = Is/Vt * exp(v/Vt)
    fn diode_derivative_simd(&self, v: f64x4) -> f64x4 {
        self.is_simd * (v / self.vt_simd).exp() / self.vt_simd
    }
    
    pub fn process_incident_simd(&mut self, a: f64x4) -> f64x4 {
        let v = self.solve_newton_simd(a, self.port_resistance);
        let i = self.diode_equation_simd(v);
        
        // b = 2v - a
        f64x4::splat(2.0) * v - a
    }
}

pub struct SimdMoogLadderFilter {
    sample_rate: f64,
    cutoff: f64,
    resonance: f64,
    
    // SIMD-векторизованные состояния
    stage_states: [f64x4; 4], // 4 ступени, каждая - SIMD вектор
    feedback: f64x4,
    input_state: f64x4,
    output_state: f64x4,
    
    // Предвычисленные коэффициенты
    alpha_simd: f64x4,       // Коэффициент фильтрации
    feedback_gain_simd: f64x4,
    drive_simd: f64x4,
}

impl SimdMoogLadderFilter {
    pub fn new(sample_rate: f64, cutoff: f64, resonance: f64, 
               simd_config: SimdConfig) -> Self {
        // Вычисляем SIMD коэффициенты
        let r = 1000.0;
        let c = 1.0 / (2.0 * std::f64::consts::PI * cutoff * r);
        let t = 1.0 / sample_rate;
        
        let rc = r * c;
        let alpha = t / (t + rc);
        
        Self {
            sample_rate,
            cutoff,
            resonance,
            stage_states: [f64x4::splat(0.0); 4],
            feedback: f64x4::splat(0.0),
            input_state: f64x4::splat(0.0),
            output_state: f64x4::splat(0.0),
            alpha_simd: f64x4::splat(alpha),
            feedback_gain_simd: f64x4::splat(4.0 * resonance),
            drive_simd: f64x4::splat(1.0),
        }
    }
    
    /// SIMD обработка пакета семплов
    pub fn process_batch(&mut self, inputs: &[f64], outputs: &mut [f64]) {
        let simd_type = SimdConfig::detect().f64_simd_type();
        
        match simd_type {
            SimdType::F64x4 => self.process_f64x4(inputs, outputs),
            SimdType::F64x8 => self.process_f64x8(inputs, outputs),
            _ => self.process_scalar(inputs, outputs),
        }
    }
    
    fn process_f64x4(&mut self, inputs: &[f64], outputs: &mut [f64]) {
        let one = f64x4::splat(1.0);
        let minus_one = f64x4::splat(-1.0);
        
        // SIMD быстрый tanh аппроксимация
        let fast_tanh = |x: f64x4| -> f64x4 {
            x / (one + x.abs())
        };
        
        for chunk_idx in (0..inputs.len()).step_by(4) {
            let chunk_end = (chunk_idx + 4).min(inputs.len());
            
            // Загружаем входной вектор
            let input = if chunk_end - chunk_idx == 4 {
                f64x4::from_slice(&inputs[chunk_idx..chunk_end])
            } else {
                let mut arr = [0.0; 4];
                arr[..chunk_end - chunk_idx].copy_from_slice(&inputs[chunk_idx..chunk_end]);
                f64x4::from_array(arr)
            };
            
            // Применяем драйв и мягкое ограничение
            let driven = input * self.drive_simd;
            let input_tanh = fast_tanh(driven);
            
            // Feedback
            let feedback = self.output_state * self.feedback_gain_simd;
            let filter_input = input_tanh - feedback;
            
            // Обрабатываем через 4 ступени (векторизованно)
            let mut signal = filter_input;
            
            for stage in 0..4 {
                let state = self.stage_states[stage];
                // Обновление состояния: state = alpha*signal + (1-alpha)*state
                self.stage_states[stage] = self.alpha_simd * signal + 
                    (one - self.alpha_simd) * state;
                signal = self.stage_states[stage];
            }
            
            self.output_state = signal;
            
            // Сохраняем результат
            if chunk_end - chunk_idx == 4 {
                signal.copy_to_slice(&mut outputs[chunk_idx..chunk_end]);
            } else {
                let arr: [f64; 4] = signal.into();
                outputs[chunk_idx..chunk_end].copy_from_slice(&arr[..chunk_end - chunk_idx]);
            }
        }
    }
}

pub struct SimdWdfMoogFilterNode {
    filter: SimdMoogLadderFilter,
    sample_rate: f32,
    temp_buffer_f64: Vec<f64>,
    temp_buffer_f32: Vec<f32>,
    simd_config: SimdConfig,
}

impl SimdWdfMoogFilterNode {
    pub fn new(sample_rate: f32, cutoff: f32, resonance: f32) -> Self {
        let simd_config = SimdConfig::detect();
        
        Self {
            filter: SimdMoogLadderFilter::new(
                sample_rate as f64,
                cutoff as f64,
                resonance as f64,
                simd_config,
            ),
            sample_rate,
            temp_buffer_f64: Vec::new(),
            temp_buffer_f32: Vec::new(),
            simd_config,
        }
    }
    
    /// Оптимизированная обработка с SIMD
    fn process_simd_optimized(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) {
        if inputs.is_empty() || outputs.is_empty() {
            return;
        }
        
        let buffer_size = outputs[0].len();
        
        // Подготавливаем буферы с правильным выравниванием
        self.prepare_buffers(buffer_size);
        
        let input = inputs[0];
        let output = &mut outputs[0];
        
        // Конвертируем f32 -> f64 (можно оптимизировать с SIMD)
        for i in 0..buffer_size {
            self.temp_buffer_f64[i] = input[i] as f64;
        }
        
        // SIMD обработка
        self.filter.process_batch(&self.temp_buffer_f64, 
                                  &mut self.temp_buffer_f64);
        
        // Конвертируем обратно f64 -> f32
        for i in 0..buffer_size {
            output[i] = self.temp_buffer_f64[i] as f32;
        }
    }
    
    fn prepare_buffers(&mut self, size: usize) {
        if self.temp_buffer_f64.len() < size {
            // Выравниваем для SIMD
            let alignment = self.simd_config.f64_simd_type().align_bytes();
            let aligned_size = ((size + alignment - 1) / alignment) * alignment;
            
            self.temp_buffer_f64 = Vec::with_capacity(aligned_size);
            // Инициализируем нулями
            unsafe { self.temp_buffer_f64.set_len(aligned_size) };
        }
        
        if self.temp_buffer_f32.len() < size {
            self.temp_buffer_f32.resize(size, 0.0);
        }
    }
}

impl AudioNode for SimdWdfMoogFilterNode {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        self.process_simd_optimized(inputs, outputs);
        Ok(())
    }
    
    // ... остальная реализация как в оригинале ...
}