use kama_core::{AudioNode, ParamValue, NodeMetadata, NodeCategory, AudioError, node::ParamMetadata};
use crate::config::LofiConfig;
use crate::lofi_processor::LofiProcessor;

pub struct Ay38910Emulator {
    channels: [AyChannel; 3],
    noise: AyNoise,
    envelope: AyEnvelope,
    mixer: AyMixer,
    sample_rate: f32,
    chip_clock: f32,
    registers: [u8; 16],
    registers_dirty: bool,
    lofi: LofiProcessor,
}

// Добавьте это в kama-lofi/src/lib.rs, после эмулятора NES

/// Эмулятор AY-3-8910 / YM2149 (ZX Spectrum 128, Atari ST, Amstrad CPC)
pub struct Ay38910Emulator {
    /// Три независимых канала
    channels: [AyChannel; 3],
    /// Генератор шума
    noise: AyNoise,
    /// Огибающая
    envelope: AyEnvelope,
    /// Микшер (управляет смешиванием тона/шума для каждого канала)
    mixer: AyMixer,
    /// Частота дискретизации эмуляции
    sample_rate: f32,
    /// Главная частота чипа (обычно 1.75 MHz или 2 MHz)
    chip_clock: f32,
    /// Состояние регистров (16 регистров AY-3-8910)
    registers: [u8; 16],
    /// Флаг обновления регистров
    registers_dirty: bool,
    /// Lo-Fi процессор для дополнительного эффекта
    lofi: LofiProcessor,
}

/// Канал AY-3-8910
#[derive(Clone)]
struct AyChannel {
    /// Частота тона (период = значение регистра / частота чипа)
    tone_period: u16,      // 12 бит (0-4095)
    /// Громкость (0-15)
    volume: u8,
    /// Текущая фаза тона
    phase: f32,
    /// Использовать огибающую вместо фиксированной громкости
    use_envelope: bool,
}

/// Генератор шума
#[derive(Clone)]
struct AyNoise {
    /// Период шума (5 бит)
    period: u8,
    /// Регистр сдвига (17-битный)
    shift_register: u32,
    /// Частота обновления шума
    noise_freq: f32,
    /// Текущий выход шума
    output: bool,
}

/// Огибающая
#[derive(Clone)]
struct AyEnvelope {
    /// Период огибающей (16 бит)
    period: u16,
    /// Режим огибающей (0-15)
    mode: u8,
    /// Текущая фаза огибающей
    phase: f32,
    /// Текущее значение огибающей (0-15)
    value: u8,
    /// Счётчик для hold/alternate режимов
    counter: u32,
}

/// Микшер (управляет смешиванием)
#[derive(Clone)]
struct AyMixer {
    /// Для каждого канала: бит 0 = тон включен, бит 1 = шум включен
    channel_modes: [u8; 3],
    /// Глобальное включение/выключение
    io_a_enabled: bool,    // Порт A (обычно не используется в аудио)
    io_b_enabled: bool,    // Порт B (обычно не используется в аудио)
}

impl Ay38910Emulator {
    /// Создаёт новый эмулятор AY-3-8910
    pub fn new(sample_rate: f32) -> Self {
        // Типичная частота чипа в ZX Spectrum 128 - 1.75 MHz
        let chip_clock = 1_750_000.0;
        
        let lofi_config = LofiConfig::for_system(ClassicSystem::Custom {
            bit_depth: 8,
            sample_rate: 44100.0,
            nonlinear: false,
            noise_floor: -48.0,
        });
        
        Self {
            channels: [
                AyChannel {
                    tone_period: 0,
                    volume: 0,
                    phase: 0.0,
                    use_envelope: false,
                },
                AyChannel {
                    tone_period: 0,
                    volume: 0,
                    phase: 0.0,
                    use_envelope: false,
                },
                AyChannel {
                    tone_period: 0,
                    volume: 0,
                    phase: 0.0,
                    use_envelope: false,
                },
            ],
            noise: AyNoise {
                period: 0,
                shift_register: 0x0001_0000, // 17-битный регистр, начинаем с 1
                noise_freq: 0.0,
                output: false,
            },
            envelope: AyEnvelope {
                period: 0,
                mode: 0,
                phase: 0.0,
                value: 0,
                counter: 0,
            },
            mixer: AyMixer {
                channel_modes: [0, 0, 0],
                io_a_enabled: false,
                io_b_enabled: false,
            },
            sample_rate,
            chip_clock,
            registers: [0; 16],
            registers_dirty: true,
            lofi: LofiProcessor::new(lofi_config),
        }
    }
    
    /// Устанавливает значение регистра AY-3-8910
    pub fn write_register(&mut self, reg: usize, value: u8) {
        if reg < 16 {
            self.registers[reg] = value;
            self.registers_dirty = true;
            self.update_from_registers();
        }
    }
    
    /// Читает значение регистра
    pub fn read_register(&self, reg: usize) -> u8 {
        if reg < 16 {
            self.registers[reg]
        } else {
            0
        }
    }
    
    /// Обновляет внутреннее состояние из регистров
    fn update_from_registers(&mut self) {
        // Регистры AY-3-8910:
        // R0,R1: Канал A период тона (R0: младшие 8 бит, R1: старшие 4 бита)
        // R2,R3: Канал B период тона
        // R4,R5: Канал C период тона
        // R6: Период шума (5 бит)
        // R7: Микшер (D0: канал A тон, D1: канал A шум, D2: канал B тон, D3: канал B шум, D4: канал C тон, D5: канал C шум, D6: порт A, D7: порт B)
        // R8,R9,R10: Громкость каналов A,B,C (бит 4: использовать огибающую, биты 0-3: громкость)
        // R11,R12: Период огибающей (R11: младшие 8 бит, R12: старшие 8 бит)
        // R13: Форма огибающей (4 бита)
        // R14,R15: Порта A и B (не используются в аудио)
        
        // Канал A
        self.channels[0].tone_period = ((self.registers[1] as u16 & 0x0F) << 8) | (self.registers[0] as u16);
        
        // Канал B
        self.channels[1].tone_period = ((self.registers[3] as u16 & 0x0F) << 8) | (self.registers[2] as u16);
        
        // Канал C
        self.channels[2].tone_period = ((self.registers[5] as u16 & 0x0F) << 8) | (self.registers[4] as u16);
        
        // Период шума (только 5 бит)
        self.noise.period = self.registers[6] & 0x1F;
        if self.noise.period > 0 {
            self.noise.noise_freq = self.chip_clock / (16.0 * self.noise.period as f32);
        } else {
            self.noise.noise_freq = 0.0;
        }
        
        // Микшер
        let mixer_reg = self.registers[7];
        self.mixer.channel_modes[0] = (mixer_reg & 0x03) as u8;      // Биты 0-1: канал A
        self.mixer.channel_modes[1] = ((mixer_reg >> 2) & 0x03) as u8; // Биты 2-3: канал B
        self.mixer.channel_modes[2] = ((mixer_reg >> 4) & 0x03) as u8; // Биты 4-5: канал C
        self.mixer.io_a_enabled = (mixer_reg & 0x40) == 0;
        self.mixer.io_b_enabled = (mixer_reg & 0x80) == 0;
        
        // Громкость каналов
        for i in 0..3 {
            let vol_reg = self.registers[8 + i];
            self.channels[i].use_envelope = (vol_reg & 0x10) != 0;
            self.channels[i].volume = vol_reg & 0x0F;
        }
        
        // Период огибающей
        self.envelope.period = ((self.registers[12] as u16) << 8) | (self.registers[11] as u16);
        self.envelope.mode = self.registers[13] & 0x0F;
    }
    
    /// Генерирует один семпл (моно)
    pub fn generate_sample(&mut self) -> f32 {
        // Обновляем состояние если регистры изменились
        if self.registers_dirty {
            self.update_from_registers();
            self.registers_dirty = false;
        }
        
        let sample_rate = self.sample_rate;
        let chip_clock = self.chip_clock;
        
        // Обновляем тон каждого канала
        let mut channel_samples = [0.0f32; 3];
        
        for i in 0..3 {
            let channel = &mut self.channels[i];
            
            if channel.tone_period > 0 {
                // Частота тона = chip_clock / (16 * tone_period)
                let tone_freq = chip_clock / (16.0 * channel.tone_period as f32);
                let phase_inc = tone_freq / sample_rate;
                
                channel.phase += phase_inc;
                if channel.phase >= 1.0 {
                    channel.phase -= 1.0;
                }
            }
            
            // Определяем, включен ли тон (бит 0 = 0 означает включен)
            let tone_enabled = (self.mixer.channel_modes[i] & 0x01) == 0;
            
            // Определяем, включен ли шум (бит 1 = 0 означает включен)
            let noise_enabled = (self.mixer.channel_modes[i] & 0x02) == 0;
            
            // Генерируем тон (меандр)
            let tone_sample = if tone_enabled && channel.tone_period > 0 {
                if channel.phase < 0.5 { 1.0 } else { -1.0 }
            } else {
                0.0
            };
            
            // Генерируем шум
            let noise_sample = if noise_enabled {
                if self.noise.output { 1.0 } else { -1.0 }
            } else {
                0.0
            };
            
            // Смешиваем тон и шум (в AY-3-8910 они просто складываются)
            let mixed = (tone_sample + noise_sample) * 0.5;
            
            // Применяем громкость
            let volume = if channel.use_envelope {
                self.envelope.value as f32 / 15.0
            } else {
                channel.volume as f32 / 15.0
            };
            
            channel_samples[i] = mixed * volume;
        }
        
        // Обновляем шум
        self.update_noise();
        
        // Обновляем огибающую
        self.update_envelope();
        
        // Смешиваем три канала (в AY-3-8910 они просто суммируются)
        let mixed = (channel_samples[0] + channel_samples[1] + channel_samples[2]) / 3.0;
        
        // Применяем lo-fi обработку для аутентичного звучания
        self.lofi.process_sample(mixed)
    }
    
    /// Обновляет генератор шума
    fn update_noise(&mut self) {
        if self.noise.period == 0 {
            return;
        }
        
        let noise_freq = self.noise.noise_freq;
        let increments_per_sample = noise_freq / self.sample_rate;
        
        static mut NOISE_PHASE: f32 = 0.0;
        unsafe {
            NOISE_PHASE += increments_per_sample;
            if NOISE_PHASE >= 1.0 {
                NOISE_PHASE -= 1.0;
                
                // 17-битный LFSR: x^17 + x^14 + 1
                let feedback = (self.noise.shift_register >> 16) ^ 
                               (self.noise.shift_register >> 13) & 1;
                self.noise.shift_register = ((self.noise.shift_register << 1) | feedback) & 0x1FFFF;
                
                // Выход - старший бит
                self.noise.output = (self.noise.shift_register >> 16) != 0;
            }
        }
    }
    
    /// Обновляет огибающую
    fn update_envelope(&mut self) {
        if self.envelope.period == 0 {
            self.envelope.value = 0;
            return;
        }
        
        let env_freq = self.chip_clock / (16.0 * self.envelope.period as f32);
        let increments_per_sample = env_freq / self.sample_rate;
        
        self.envelope.phase += increments_per_sample;
        
        if self.envelope.phase >= 1.0 {
            self.envelope.phase -= 1.0;
            self.envelope.counter += 1;
            
            // Режимы огибающей (биты 0-3):
            // Биты: C A H R (Continue, Attack, Hold, Repeat)
            let cont = (self.envelope.mode & 0x08) != 0;
            let attack = (self.envelope.mode & 0x04) != 0;
            let hold = (self.envelope.mode & 0x02) != 0;
            let repeat = (self.envelope.mode & 0x01) != 0;
            
            let max_steps = 16; // 16 шагов огибающей
            
            if !cont {
                // Одноразовая огибающая
                if self.envelope.counter < max_steps {
                    self.envelope.value = if attack {
                        self.envelope.counter as u8
                    } else {
                        (max_steps - 1 - self.envelope.counter) as u8
                    };
                } else {
                    self.envelope.value = if hold {
                        if attack { 15 } else { 0 }
                    } else {
                        0
                    };
                }
            } else {
                // Циклическая огибающая
                let cycle_pos = self.envelope.counter % (max_steps as u32);
                
                if !hold && repeat {
                    // Треугольная/пилообразная
                    if attack {
                        self.envelope.value = cycle_pos as u8;
                    } else {
                        self.envelope.value = (max_steps - 1 - cycle_pos as usize) as u8;
                    }
                } else if hold && !repeat {
                    // Одноразовая с удержанием
                    if self.envelope.counter < max_steps {
                        self.envelope.value = if attack {
                            cycle_pos as u8
                        } else {
                            (max_steps - 1 - cycle_pos as usize) as u8
                        };
                    }
                } else {
                    // Простое повторение
                    self.envelope.value = if attack {
                        cycle_pos as u8
                    } else {
                        (max_steps - 1 - cycle_pos as usize) as u8
                    };
                }
            }
        }
    }
    
    /// Сброс эмулятора
    pub fn reset(&mut self) {
        self.registers = [0; 16];
        self.registers_dirty = true;
        
        for channel in &mut self.channels {
            channel.phase = 0.0;
        }
        
        self.noise.shift_register = 0x0001_0000;
        self.noise.output = false;
        
        self.envelope.phase = 0.0;
        self.envelope.value = 0;
        self.envelope.counter = 0;
        
        self.lofi.reset();
    }
}

impl AudioNode for Ay38910Emulator {
    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if outputs.is_empty() {
            return Ok(());
        }
        
        let output = &mut outputs[0];
        
        for out in output.iter_mut() {
            *out = self.generate_sample();
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "chip_clock" => Some(ParamValue::Float(self.chip_clock)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("chip_clock", ParamValue::Float(v)) => {
                self.chip_clock = v.max(1_000_000.0).min(4_000_000.0);
                self.registers_dirty = true;
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.lofi.init(sample_rate);
    }
    
    fn reset(&mut self) {
        self.reset();
    }
    
    fn num_inputs(&self) -> usize { 0 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "AY-3-8910".to_string(),
            category: NodeCategory::Generator,
            description: "AY-3-8910 / YM2149 sound chip emulation (ZX Spectrum 128, Atari ST, Amstrad CPC)".to_string(),
            author: "Kama Lo-Fi".to_string(),
            version: "1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "chip_clock".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1_750_000.0),
                    min: Some(1_000_000.0),
                    max: Some(4_000_000.0),
                    step: Some(100_000.0),
                    unit: Some("Hz".to_string()),
                    choices: Some(vec![
                        ("ZX Spectrum (1.75 MHz)".to_string(), 1_750_000.0),
                        ("Atari ST (2.0 MHz)".to_string(), 2_000_000.0),
                        ("Amstrad CPC (1.0 MHz)".to_string(), 1_000_000.0),
                    ]),
                },
            ],
        }
    }
}

// Добавим тесты
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ay38910_basic() {
        let mut ay = Ay38910Emulator::new(44100.0);
        
        // Устанавливаем простую ноту
        ay.write_register(0, 0x00); // Канал A, младшие биты периода
        ay.write_register(1, 0x01); // Канал A, старшие биты периода (период = 256)
        
        ay.write_register(8, 0x0F); // Громкость канала A = 15
        
        // Отключаем шум для канала A, оставляем тон
        ay.write_register(7, 0x3E); // Биты: 0011 1110 (канал A: тон вкл, шум выкл)
        
        let mut output = vec![0.0f32; 1024];
        let mut outputs = [&mut output[..]];
        
        ay.process(&[], &mut outputs).unwrap();
        
        // Проверяем, что генерируется сигнал
        assert!(output.iter().any(|&x| x != 0.0));
        
        // Проверяем, что сигнал в пределах [-1, 1]
        for &sample in &output {
            assert!(sample >= -1.0 && sample <= 1.0);
        }
    }
    
    #[test]
    fn test_ay38910_registers() {
        let mut ay = Ay38910Emulator::new(44100.0);
        
        ay.write_register(0, 0x34);
        ay.write_register(1, 0x02);
        
        assert_eq!(ay.read_register(0), 0x34);
        assert_eq!(ay.read_register(1), 0x02);
        
        // Проверяем, что период тона правильно сформирован
        ay.update_from_registers();
        assert_eq!(ay.channels[0].tone_period, 0x0234);
    }
}