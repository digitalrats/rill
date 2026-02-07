use std::time::Instant;

/// Автомат для изменения параметров
pub trait Automaton: Send + Sync {
    fn apply(&mut self, _time: f64, value: f32) -> f32;  // ФИКС: добавляем _ перед time
    fn reset(&mut self);
    fn name(&self) -> &str;
}

/// Параметр с автоматизацией
pub struct AutomatedParameter {
    value: f32,
    default: f32,
    min: Option<f32>,
    max: Option<f32>,
    automaton: Option<Box<dyn Automaton>>,
    automation_enabled: bool,
    last_update: Instant,
}

impl AutomatedParameter {
    pub fn new(default: f32) -> Self {
        Self {
            value: default,
            default,
            min: None,
            max: None,
            automaton: None,
            automation_enabled: false,
            last_update: Instant::now(),
        }
    }
    
    pub fn update(&mut self) -> f32 {
        if self.automation_enabled {
            if let Some(automaton) = &mut self.automaton {
                let elapsed = self.last_update.elapsed();
                let time = elapsed.as_secs_f64();
                self.value = automaton.apply(time, self.value);
                
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
    
    pub fn set_automaton(&mut self, automaton: Box<dyn Automaton>) {
        self.automaton = Some(automaton);
        self.automation_enabled = true;
    }
    
    pub fn enable_automation(&mut self) {
        self.automation_enabled = true;
    }
    
    pub fn disable_automation(&mut self) {
        self.automation_enabled = false;
    }
}

/// Простой LFO автомат
pub struct LfoAutomaton {
    frequency: f32,
    amplitude: f32,
    offset: f32,
    phase: f32,
}

impl LfoAutomaton {
    pub fn new(frequency: f32, amplitude: f32, offset: f32) -> Self {
        Self {
            frequency,
            amplitude,
            offset,
            phase: 0.0,
        }
    }
}

impl Automaton for LfoAutomaton {
    fn apply(&mut self, _time: f64, value: f32) -> f32 {  // ФИКС: добавляем _ перед time
        self.phase += 2.0 * std::f64::consts::PI as f32 * self.frequency * (1.0 / 44100.0);
        if self.phase > 2.0 * std::f64::consts::PI as f32 {
            self.phase -= 2.0 * std::f64::consts::PI as f32;
        }
        
        let modulation = self.phase.sin() * self.amplitude + self.offset;
        value * modulation
    }
    
    fn reset(&mut self) {
        self.phase = 0.0;
    }
    
    fn name(&self) -> &str {
        "LFO"
    }
}

/// Менеджер автоматизации
pub struct AutomationManager {
    parameters: std::collections::HashMap<String, AutomatedParameter>,
    bpm: f32,
    playing: bool,
    position: f64,
}

impl AutomationManager {
    pub fn new() -> Self {
        Self {
            parameters: std::collections::HashMap::new(),
            bpm: 120.0,
            playing: false,
            position: 0.0,
        }
    }
    
    pub fn update_all(&mut self) {
        for param in self.parameters.values_mut() {
            param.update();
        }
        
        if self.playing {
            let beats_per_second = self.bpm / 60.0;
            self.position += beats_per_second as f64 / 44100.0;
        }
    }
    
    pub fn add_parameter(&mut self, name: String, param: AutomatedParameter) {
        self.parameters.insert(name, param);
    }
    
    pub fn get_parameter(&mut self, name: &str) -> Option<&mut AutomatedParameter> {
        self.parameters.get_mut(name)
    }
}