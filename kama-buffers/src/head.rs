//! # Головки воспроизведения для буферов
//! 
//! Головки воспроизведения позволяют читать данные из буфера с разными параметрами:
//! - скорость воспроизведения
//! - направление (вперёд/назад)
//! - панорама
//! - громкость
//! - режим чтения (Simple, Loop, PingPong, Granular)

//! Головки воспроизведения для буферов

use crate::view::BufferView;

/// Состояние головки воспроизведения
#[derive(Debug, Clone, Copy)]
    /// Состояние головки воспроизведения.
pub struct HeadState {
    /// Текущая позиция (с плавающей точкой для интерполяции)
    pub position: f32,
    /// Скорость воспроизведения (1.0 = нормальная)
    pub speed: f32,
    /// Направление воспроизведения
    pub direction: Direction,
    /// Громкость (0.0 - 1.0)
    pub volume: f32,
    /// Панорама (-1.0 лево, 0.0 центр, 1.0 право)
    pub pan: f32,
}

impl Default for HeadState {
    fn default() -> Self {
        Self {
            position: 0.0,
            speed: 1.0,
            direction: Direction::Forward,
            volume: 1.0,
            pan: 0.0,
        }
    }
}

/// Направление воспроизведения
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// Направление воспроизведения.
pub enum Direction {
    Forward,
    Reverse,
}

impl Default for Direction {
    fn default() -> Self {
        Self::Forward
    }
}

/// Режим чтения буфера
#[derive(Debug, Clone, Copy)]
    /// Режим чтения буфера.
pub enum ReadMode {
    /// Простое последовательное чтение
    Simple,
    /// Зацикленное чтение
    Loop,
    /// Чтение вперед-назад
    PingPong,
    /// Гранулярный синтез
    Granular {
        /// Размер гранулы в семплах
        grain_size: usize,
        /// Расстояние между гранулами в семплах
        spacing: usize,
        /// Случайное смещение позиций (0.0 - 1.0)
        randomization: f32,
    },
}

impl Default for ReadMode {
    fn default() -> Self {
        Self::Simple
    }
}

/// Головка воспроизведения
#[derive(Clone)]
    /// Головка воспроизведения.
    ///
    /// Позволяет читать данные из буфера с независимыми параметрами.
pub struct BufferHead {
    /// Текущее состояние
    pub state: HeadState,
    /// Режим чтения
    pub read_mode: ReadMode,
    /// Включена ли головка
    pub enabled: bool,
    /// Уникальный идентификатор
    pub id: usize,
    // Внутреннее состояние для сложных режимов
    grain_phase: usize,
    grain_position: usize,
    pingpong_forward: bool,
    pingpong_just_switched: bool, // Флаг, чтобы не повторять крайние точки
}

impl BufferHead {
    /// Создать новую головку с указанным ID
    /// Создать новую головку с указанным ID.
    pub fn new(id: usize) -> Self {
        Self {
            state: HeadState::default(),
            read_mode: ReadMode::default(),
            enabled: true,
            id,
            grain_phase: 0,
            grain_position: 0,
            pingpong_forward: true,
            pingpong_just_switched: false,
        }
    }
    
    /// Создать головку с начальной скоростью
    /// Создать головку с начальной скоростью.
    pub fn with_speed(mut self, speed: f32) -> Self {
        self.state.speed = speed;
        self
    }
    
    /// Создать головку с панорамой
    /// Создать головку с панорамой.
    pub fn with_pan(mut self, pan: f32) -> Self {
        self.state.pan = pan.clamp(-1.0, 1.0);
        self
    }
    
    /// Создать головку с громкостью
    /// Создать головку с громкостью.
    pub fn with_volume(mut self, volume: f32) -> Self {
        self.state.volume = volume.clamp(0.0, 1.0);
        self
    }
    
    /// Создать головку с режимом чтения
    /// Создать головку с режимом чтения.
    pub fn with_read_mode(mut self, mode: ReadMode) -> Self {
        self.read_mode = mode;
        self
    }
    
    /// Прочитать один семпл из View
    /// Прочитать один семпл из View.
    pub fn read_sample(&mut self, view: &BufferView) -> f32 {
        if !self.enabled {
            return 0.0;
        }
        
        let buffer_size = view.size() as f32;
        
        // Получаем семпл в зависимости от режима
        let sample = match self.read_mode {
            ReadMode::Simple | ReadMode::Loop => {
                self.read_linear(view, buffer_size)
            }
            ReadMode::PingPong => {
                self.read_pingpong(view, buffer_size)
            }
            ReadMode::Granular { grain_size, spacing, randomization } => {
                self.read_granular(view, grain_size, spacing, randomization)
            }
        };
        
        // Обновляем позицию после чтения (для всех режимов кроме PingPong)
        if !matches!(self.read_mode, ReadMode::PingPong) {
            self.update_position(buffer_size);
        }
        
        sample * self.state.volume
    }
    
    /// Линейное чтение (Simple и Loop)
    fn read_linear(&self, view: &BufferView, buffer_size: f32) -> f32 {
        match self.state.direction {
            Direction::Forward => {
                view.get_interpolated(self.state.position)
            }
            Direction::Reverse => {
                // В reverse режиме читаем с конца буфера
                let read_pos = buffer_size - 1.0 - self.state.position;
                view.get_interpolated(read_pos)
            }
        }
    }
    
/// Ping-pong режим
fn read_pingpong(&mut self, view: &BufferView, buffer_size: f32) -> f32 {
    // Используем отдельную переменную для текущей позиции в направлении
    let pos_in_direction = if self.pingpong_forward {
        self.state.position
    } else {
        buffer_size - 1.0 - self.state.position
    };
    
    let sample = view.get_interpolated(pos_in_direction);
    
    // Обновляем основную позицию
    self.state.position += 1.0;
    
    // Проверяем, не достигли ли мы конца направления
    if self.pingpong_forward && self.state.position >= buffer_size - 0.1 {
        self.pingpong_forward = false;
        self.state.position = 1.0; // Начинаем обратный отсчет с 1
        println!("  Switching to backward");
    } else if !self.pingpong_forward && self.state.position >= buffer_size - 0.1 {
        self.pingpong_forward = true;
        self.state.position = 1.0; // Начинаем прямой отсчет с 1
        println!("  Switching to forward");
    }
    
    sample
}
    
    /// Гранулярный режим
    fn read_granular(&mut self, view: &BufferView, grain_size: usize, _spacing: usize, randomization: f32) -> f32 {
        let buffer_size = view.size();
        
        // Если гранула закончилась или это первый вызов
        if self.grain_phase >= grain_size || self.grain_phase == 0 {
            self.grain_phase = 0;
            
            // Выбираем новую позицию для гранулы
            use rand::Rng;
            let mut rng = rand::thread_rng();
            
            let random_offset = if randomization > 0.0 {
                (rng.gen::<f32>() * 2.0 - 1.0) * randomization * buffer_size as f32
            } else {
                0.0
            };
            
            // Текущая позиция + случайное смещение
            let mut pos = self.state.position + random_offset;
            
            // Нормализуем в диапазон [0, buffer_size)
            while pos < 0.0 {
                pos += buffer_size as f32;
            }
            while pos >= buffer_size as f32 {
                pos -= buffer_size as f32;
            }
            
            self.grain_position = pos.floor() as usize;
        }
        
        // Читаем семпл из текущей гранулы
        let read_pos = self.grain_position + self.grain_phase;
        let sample = if read_pos < buffer_size {
            view.get(read_pos)
        } else {
            // Если вышли за границы, зацикливаем
            view.get(read_pos % buffer_size)
        };
        
        // Применяем оконную функцию Ханна
        let window = if grain_size > 0 {
            let x = self.grain_phase as f32 / grain_size as f32;
            0.5 * (1.0 - (2.0 * std::f32::consts::PI * x).cos())
        } else {
            1.0
        };
        
        self.grain_phase += 1;
        
        sample * window
    }
    
    /// Обновить позицию на основе направления и скорости (для Simple, Loop, Granular)
    fn update_position(&mut self, buffer_size: f32) {
        match self.state.direction {
            Direction::Forward => {
                self.state.position += self.state.speed;
            }
            Direction::Reverse => {
                self.state.position += self.state.speed;
            }
        }
        
        // Обрабатываем границы в зависимости от режима
        match self.read_mode {
            ReadMode::Simple => {
                if self.state.position >= buffer_size {
                    self.state.position = buffer_size - 1.0;
                }
            }
            ReadMode::Loop => {
                while self.state.position >= buffer_size {
                    self.state.position -= buffer_size;
                }
            }
            ReadMode::Granular { .. } => {
                while self.state.position >= buffer_size {
                    self.state.position -= buffer_size;
                }
            }
            _ => {}
        }
    }
    
    /// Сбросить внутреннее состояние
    /// Сбросить внутреннее состояние.
    pub fn reset(&mut self) {
        self.state.position = 0.0;
        self.state.direction = Direction::Forward;
        self.grain_phase = 0;
        self.grain_position = 0;
        self.pingpong_forward = true;
        self.pingpong_just_switched = false;
    }
    
    /// Установить скорость воспроизведения
    /// Установить скорость воспроизведения.
    pub fn set_speed(&mut self, speed: f32) {
        self.state.speed = speed;
    }
    
    /// Установить панораму
    /// Установить панораму.
    pub fn set_pan(&mut self, pan: f32) {
        self.state.pan = pan.clamp(-1.0, 1.0);
    }
    
    /// Установить громкость
    /// Установить громкость.
    pub fn set_volume(&mut self, volume: f32) {
        self.state.volume = volume.clamp(0.0, 1.0);
    }
    
    /// Включить/выключить головку
    /// Включить/выключить головку.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    
    /// Получить текущую позицию
    /// Получить текущую позицию.
    pub fn position(&self) -> f32 {
        self.state.position
    }
    
    /// Установить позицию
    /// Установить позицию.
    pub fn set_position(&mut self, position: f32) {
        self.state.position = position;
    }
    
    /// Получить фазу гранулы (для отладки)
    pub fn grain_phase(&self) -> usize {
        self.grain_phase
    }
    
    /// Получить позицию гранулы (для отладки)
    pub fn grain_position(&self) -> usize {
        self.grain_position
    }
}

impl Default for BufferHead {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ring::RingBuffer;
    
    #[test]
    fn test_head_simple_read() {
        let mut buffer = RingBuffer::new(8);
        let test_data: Vec<f32> = (1..=8).map(|i| i as f32).collect();
        buffer.write(&test_data);
        
        let view = buffer.view();
        let mut head = BufferHead::new(1).with_speed(1.0);
        
        assert_eq!(head.read_sample(&view), 1.0);
        assert_eq!(head.read_sample(&view), 2.0);
        assert_eq!(head.read_sample(&view), 3.0);
        assert_eq!(head.read_sample(&view), 4.0);
        assert_eq!(head.read_sample(&view), 5.0);
        assert_eq!(head.read_sample(&view), 6.0);
        assert_eq!(head.read_sample(&view), 7.0);
        assert_eq!(head.read_sample(&view), 8.0);
        assert_eq!(head.read_sample(&view), 8.0);
    }
    
    #[test]
    fn test_head_reverse() {
        let mut buffer = RingBuffer::new(8);
        let test_data: Vec<f32> = (1..=8).map(|i| i as f32).collect();
        buffer.write(&test_data);
        
        let view = buffer.view();
        let mut head = BufferHead::new(1)
            .with_speed(1.0);
        
        head.state.direction = Direction::Reverse;
        head.state.position = 0.0;
        
        assert_eq!(head.read_sample(&view), 8.0);
        assert_eq!(head.read_sample(&view), 7.0);
        assert_eq!(head.read_sample(&view), 6.0);
        assert_eq!(head.read_sample(&view), 5.0);
        assert_eq!(head.read_sample(&view), 4.0);
        assert_eq!(head.read_sample(&view), 3.0);
        assert_eq!(head.read_sample(&view), 2.0);
        assert_eq!(head.read_sample(&view), 1.0);
    }
    
    #[test]
    fn test_head_pingpong() {
        let mut buffer = RingBuffer::new(4);
        let test_data: Vec<f32> = (1..=4).map(|i| i as f32).collect();
        buffer.write(&test_data);
        
        let view = buffer.view();
        let mut head = BufferHead::new(1)
            .with_speed(1.0)
            .with_read_mode(ReadMode::PingPong);
        
        println!("Buffer contents: {:?}", test_data);
        println!("Starting PingPong test...\n");
        
        // Ожидаемая последовательность: 1,2,3,4,3,2,1,2,3,4
        let expected = vec![1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0, 2.0, 3.0, 4.0];
        
        for i in 0..expected.len() {
            let pos_before = head.state.position;
            let dir_before = head.pingpong_forward;
            let sample = head.read_sample(&view);
            println!("Step {}: pos_before={:.2}, dir_before={}, pos_after={:.2}, dir_after={}, sample={}, expected={}", 
                     i, pos_before, dir_before, head.state.position, head.pingpong_forward, sample, expected[i]);
            assert_eq!(sample, expected[i], "Failed at step {}", i);
        }
    }
}