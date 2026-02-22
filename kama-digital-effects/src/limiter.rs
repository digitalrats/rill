// kama-digital-effects/src/limiter.rs

use kama_core_traits::{
    AudioNode, AudioError, ParamValue, NodeMetadata, NodeCategory, NodeTypeId,
    param::{ParamType, ParamMetadata}
};
use kama_buffers::RingBuffer;
use crate::delay::Delay;

/// Limiter with lookahead using Delay + envelope detection
pub struct Limiter {
    /// Delay line for lookahead
    delay: Delay,
    /// Buffer for envelope detection
    analysis_buffer: RingBuffer,
    /// Threshold in dB
    threshold_db: f32,
    /// Threshold in linear scale
    threshold_linear: f32,
    /// Output gain after limiting
    output_gain: f32,
    /// Attack time in seconds
    attack: f32,
    /// Release time in seconds
    release: f32,
    /// Lookahead time in seconds
    lookahead: f32,
    /// Lookahead in samples
    lookahead_samples: usize,
    /// Current gain reduction
    current_gain: f32,
    /// Attack coefficient
    attack_coeff: f32,
    /// Release coefficient
    release_coeff: f32,
    /// Sample rate
    sample_rate: f32,
    /// Current write position
    position: usize,
    /// Buffer for direct passthrough during initialization
    init_buffer: Vec<f32>,
    /// Whether we're in initialization phase
    initializing: bool,
    /// Whether we're in warmup phase after initialization
    warming_up: bool,
}

impl Limiter {
    /// Create a new limiter
    pub fn new(threshold_db: f32, attack: f32, release: f32, output_gain: f32) -> Self {
        let threshold_db = threshold_db.clamp(-60.0, 0.0);
        let threshold_linear = 10.0_f32.powf(threshold_db / 20.0);
        
        let sample_rate = 44100.0;
        let attack = attack.clamp(0.001, 0.1);
        let release = release.clamp(0.01, 1.0);
        
        let attack_coeff = (-1.0 / (attack * sample_rate)).exp();
        let release_coeff = (-1.0 / (release * sample_rate)).exp();
        
        let lookahead = 0.005; // 5ms default
        let lookahead_samples = (lookahead * sample_rate) as usize;
        
        // Delay с нужной задержкой, feedback=0, mix=1.0 (100% wet)
        let delay = Delay::new(lookahead, 0.0, 1.0);
        
        // Буфер для анализа
        let analysis_buffer = RingBuffer::new(lookahead_samples * 2);
        
        // Буфер для временного хранения входных семплов во время инициализации
        let init_buffer = Vec::with_capacity(lookahead_samples);
        
        Self {
            delay,
            analysis_buffer,
            threshold_db,
            threshold_linear,
            output_gain: output_gain.clamp(0.0, 2.0),
            attack,
            release,
            lookahead,
            lookahead_samples,
            current_gain: 1.0,
            attack_coeff,
            release_coeff,
            sample_rate,
            position: 0,
            init_buffer,
            initializing: true,
            warming_up: false,
        }
    }
    
    /// Process a single sample
    pub fn process_sample(&mut self, input: f32) -> f32 {
        self.position += 1;
        
        // 1. Записываем вход в analysis_buffer
        self.analysis_buffer.write(&[input]);
        
        // 2. Получаем задержанный сигнал из Delay
        let mut delayed = 0.0;
        let input_slice = [input];
        let mut output_slice = [delayed];
        let inputs = [&input_slice[..]];
        let mut outputs = [&mut output_slice[..]];
        
        if self.delay.process(&inputs, &mut outputs).is_ok() {
            delayed = output_slice[0];
        }
        
        // 3. На этапе инициализации
        if self.initializing {
            // Сохраняем вход в init_buffer
            self.init_buffer.push(input);
            
            // Проверяем, закончилась ли инициализация
            if self.position >= self.lookahead_samples {
                self.initializing = false;
                self.warming_up = true;
                
                // Очищаем Delay
                self.delay.reset();
                
                println!("Initialization complete, starting warmup...");
            }
            
            // Во время инициализации выход = вход
            if self.position <= 5 {
                println!("Init {}: in={:.3}, out={:.3}", self.position, input, input);
            }
            
            return input;
        }
        
        // 4. На этапе прогрева (первые lookahead_samples после инициализации)
        if self.warming_up {
            // Всё ещё используем вход как выход, пока Delay не заполнится реальными данными
            if self.position < self.lookahead_samples * 2 {
                if self.position == self.lookahead_samples + 1 {
                    println!("Warmup started at pos {}", self.position);
                }
                
                // Заполняем Delay реальными значениями
                if self.position - self.lookahead_samples <= self.init_buffer.len() {
                    let idx = self.position - self.lookahead_samples - 1;
                    if idx < self.init_buffer.len() {
                        let sample = self.init_buffer[idx];
                        let s = [sample];
                        let mut dummy = [0.0];
                        let ins = [&s[..]];
                        let mut outs = [&mut dummy[..]];
                        let _ = self.delay.process(&ins, &mut outs);
                    }
                }
                
                // Проверяем, закончился ли прогрев
                if self.position >= self.lookahead_samples * 2 - 1 {
                    self.warming_up = false;
                    println!("Warmup complete at pos {}", self.position);
                }
                
                return input;
            }
        }
        
        // 5. Анализируем сигнал в analysis_buffer
        let view = self.analysis_buffer.view();
        
        // Смотрим на максимальную амплитуду в окне lookahead_samples
        let mut max_amp = 0.0f32;
        for offset in 0..self.lookahead_samples {
            let sample = view.read_delayed(offset, 0);
            max_amp = max_amp.max(sample.abs());
        }
        
        // 6. Вычисляем target gain
        let target_gain = if max_amp > self.threshold_linear {
            self.threshold_linear / max_amp
        } else {
            1.0
        };
        
        // 7. Сглаживаем gain
        if target_gain < self.current_gain {
            self.current_gain = self.current_gain * self.attack_coeff + 
                                target_gain * (1.0 - self.attack_coeff);
        } else {
            self.current_gain = self.current_gain * self.release_coeff + 
                                target_gain * (1.0 - self.release_coeff);
        }
        
        // 8. Применяем gain к задержанному сигналу
        let output = delayed * self.current_gain * self.output_gain;
        
        // Отладка для высокого сигнала
        if input > 1.0 && self.position > self.lookahead_samples * 2 {
            println!("PROC: pos={}, in={:.3}, max={:.3}, target={:.3}, gain={:.3}, delay={:.3}, out={:.3}", 
                     self.position, input, max_amp, target_gain, self.current_gain, delayed, output);
        }
        
        output.clamp(-2.0, 2.0)
    }
    
    /// Process a block of samples
    pub fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        for i in 0..input.len().min(output.len()) {
            output[i] = self.process_sample(input[i]);
        }
    }
    
    /// Get current gain reduction
    pub fn current_gain(&self) -> f32 {
        self.current_gain
    }
    
    /// Get lookahead samples count
    pub fn lookahead_samples(&self) -> usize {
        self.lookahead_samples
    }
    
    /// Set threshold in dB
    pub fn set_threshold(&mut self, db: f32) {
        self.threshold_db = db.clamp(-60.0, 0.0);
        self.threshold_linear = 10.0_f32.powf(self.threshold_db / 20.0);
    }
    
    /// Set attack time
    pub fn set_attack(&mut self, attack: f32) {
        self.attack = attack.clamp(0.001, 0.1);
        self.attack_coeff = (-1.0 / (self.attack * self.sample_rate)).exp();
    }
    
    /// Set release time
    pub fn set_release(&mut self, release: f32) {
        self.release = release.clamp(0.01, 1.0);
        self.release_coeff = (-1.0 / (self.release * self.sample_rate)).exp();
    }
    
    /// Set lookahead time
    pub fn set_lookahead(&mut self, lookahead: f32) {
        self.lookahead = lookahead.clamp(0.0, 0.01);
        self.lookahead_samples = (self.lookahead * self.sample_rate) as usize;
        self.delay.set_delay_time(lookahead);
        self.analysis_buffer = RingBuffer::new(self.lookahead_samples * 2);
        self.current_gain = 1.0;
        self.position = 0;
        self.init_buffer.clear();
        self.initializing = true;
        self.warming_up = false;
    }
    
    /// Reset internal state - теперь с принудительным заполнением буферов
    pub fn reset(&mut self) {
        self.current_gain = 1.0;
        self.position = 0;
        self.init_buffer.clear();
        self.initializing = true;
        self.warming_up = false;
        self.delay.reset();
        self.analysis_buffer.reset();
    }
    
    /// Принудительно завершить инициализацию и прогрев (для тестов)
    pub fn force_ready(&mut self) {
        if self.initializing || self.warming_up {
            // Заполняем буферы тестовыми значениями
            for _ in 0..self.lookahead_samples * 2 {
                let test_val = 0.1;
                self.analysis_buffer.write(&[test_val]);
                let s = [test_val];
                let mut dummy = [0.0];
                let ins = [&s[..]];
                let mut outs = [&mut dummy[..]];
                let _ = self.delay.process(&ins, &mut outs);
            }
            self.initializing = false;
            self.warming_up = false;
            self.position = self.lookahead_samples * 2;
            println!("Force ready completed");
        }
    }
}

impl AudioNode for Limiter {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }
        
        let input = inputs[0];
        let output = &mut outputs[0];
        let len = input.len().min(output.len());
        
        for i in 0..len {
            output[i] = self.process_sample(input[i]);
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "threshold" => Some(ParamValue::Float(self.threshold_db)),
            "attack" => Some(ParamValue::Float(self.attack)),
            "release" => Some(ParamValue::Float(self.release)),
            "output_gain" => Some(ParamValue::Float(self.output_gain)),
            "lookahead" => Some(ParamValue::Float(self.lookahead)),
            "current_gain" => Some(ParamValue::Float(self.current_gain)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("threshold", ParamValue::Float(t)) => {
                self.set_threshold(t);
                Ok(())
            }
            ("attack", ParamValue::Float(a)) => {
                self.set_attack(a);
                Ok(())
            }
            ("release", ParamValue::Float(r)) => {
                self.set_release(r);
                Ok(())
            }
            ("output_gain", ParamValue::Float(g)) => {
                self.output_gain = g.clamp(0.0, 2.0);
                Ok(())
            }
            ("lookahead", ParamValue::Float(l)) => {
                self.set_lookahead(l);
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.attack_coeff = (-1.0 / (self.attack * sample_rate)).exp();
        self.release_coeff = (-1.0 / (self.release * sample_rate)).exp();
        
        self.lookahead_samples = (self.lookahead * sample_rate) as usize;
        self.analysis_buffer = RingBuffer::new(self.lookahead_samples * 2);
        self.current_gain = 1.0;
        self.position = 0;
        self.init_buffer.clear();
        self.initializing = true;
        self.warming_up = false;
        
        self.delay.init(sample_rate);
        self.delay.set_delay_time(self.lookahead);
    }
    
    fn reset(&mut self) {
        self.reset();
    }
    
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Limiter".to_string(),
            category: NodeCategory::Effect,
            description: "Lookahead limiter using Delay".to_string(),
            author: "Kama Digital Effects".to_string(),
            version: "0.1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "threshold".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.0),
                    min: Some(-60.0),
                    max: Some(0.0),
                    step: Some(1.0),
                    unit: Some("dB".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "attack".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.005),
                    min: Some(0.001),
                    max: Some(0.1),
                    step: Some(0.001),
                    unit: Some("s".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "release".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.1),
                    min: Some(0.01),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("s".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "output_gain".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    min: Some(0.0),
                    max: Some(2.0),
                    step: Some(0.1),
                    unit: Some("gain".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "lookahead".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.005),
                    min: Some(0.0),
                    max: Some(0.01),
                    step: Some(0.0001),
                    unit: Some("s".to_string()),
                    choices: None,
                },
            ],
        }
    }
}