use std::sync::Arc;
use serde::{Serialize, Deserialize};
use parking_lot::RwLock;
use futures::stream::{Stream, StreamExt};
use tokio::sync::{broadcast, mpsc};
use either::Either;

// --- Алгебраические типы для микшера ---

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ChannelType {
    Mono,      // Моно канал
    Stereo,    // Стерео канал (L/R)
    DualMono,  // Два независимых моно канала
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum FilterType {
    Bitcrusher,    // Цифровой биткрашер
    LowPass,       // ФНЧ (можно добавить позже)
    HighPass,      // ФВЧ
    BandPass,      // Полосовой
    Notch,         // Режекторный
    Shelf,         // Шельфовый
    Custom(String), // Пользовательский фильтр
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MixerMode {
    Normal,        // Нормальный режим
    Parallel,      // Параллельная обработка
    Serial,        // Последовательная обработка
    Sidechain,     // Sidechain-компрессия
}

// --- Конфигурация канала микшера ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub id: usize,
    pub name: String,
    pub channel_type: ChannelType,
    pub level: f64,           // 0.0 - 1.0
    pub pan: f64,             // -1.0 (L) до 1.0 (R)
    pub mute: bool,
    pub solo: bool,
    pub filters: Vec<FilterConfig>,
    pub sends: Vec<SendConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConfig {
    pub filter_type: FilterType,
    pub enabled: bool,
    pub params: FilterParams,
    pub position: usize, // Порядок в цепи обработки
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterParams {
    pub bit_depth: Option<u8>,    // Для биткрашера: 1-24 бит
    pub sample_rate_reduction: Option<f64>, // Для биткрашера: 0.0-1.0
    pub cutoff: Option<f64>,      // Частота среза (Hz)
    pub resonance: Option<f64>,   // Резонанс (0.0-1.0)
    pub drive: Option<f64>,       // Перегрузка (0.0-1.0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendConfig {
    pub to_bus: usize,       // Номер шины
    pub level: f64,          // Уровень посыла
    pub pre_post: SendType,  // Pre/Post фейдер
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SendType {
    PreFader,    // До фейдера
    PostFader,   // После фейдера
    PostFilter,  // После фильтров
}

// --- Конфигурация микшера ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixerConfig {
    pub name: String,
    pub channels: Vec<ChannelConfig>, // До 5 каналов
    pub master: MasterConfig,
    pub buses: Vec<BusConfig>,        // Дополнительные шины
    pub mode: MixerMode,
    pub sample_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterConfig {
    pub level: f64,
    pub pan: f64,
    pub filters: Vec<FilterConfig>,
    pub limiter_enabled: bool,
    pub limiter_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusConfig {
    pub id: usize,
    pub name: String,
    pub level: f64,
    pub filters: Vec<FilterConfig>,
}

// --- Чистые функции обработки сигналов ---

pub mod audio_dsp {
    use super::*;
    
    // Тип для чистых функций обработки аудио
    pub type AudioProcessor = fn(f64, &ProcessorState) -> f64;
    pub type StereoProcessor = fn((f64, f64), &ProcessorState) -> (f64, f64);
    
    #[derive(Debug, Clone)]
    pub struct ProcessorState {
        pub params: FilterParams,
        pub sample_rate: f64,
        pub internal_state: Vec<f64>, // Для stateful фильтров
    }
    
    impl ProcessorState {
        pub fn new(params: FilterParams, sample_rate: f64) -> Self {
            Self {
                params,
                sample_rate,
                internal_state: vec![0.0; 4], // Хватит для большинства фильтров
            }
        }
    }
    
    // Библиотека чистых DSP функций
    
    pub mod filters {
        use super::*;
        
        // --- Биткрашер (Mono) ---
        pub fn bitcrusher_mono(input: f64, state: &ProcessorState) -> f64 {
            let params = &state.params;
            
            let mut sample = input;
            
            // Редукция битности
            if let Some(bit_depth) = params.bit_depth {
                let bits = bit_depth.clamp(1, 24);
                let steps = (1u32 << bits) as f64;
                sample = (sample * steps).round() / steps;
            }
            
            // Редукция частоты дискретизации
            if let Some(reduction) = params.sample_rate_reduction {
                let reduction = reduction.clamp(0.0, 1.0);
                if reduction > 0.0 {
                    // Hold последнего семпла при редукции
                    let last = state.internal_state.get(0).copied().unwrap_or(0.0);
                    let should_hold = rand::random::<f64>() < reduction;
                    sample = if should_hold { last } else { sample };
                }
            }
            
            // Перегрузка (drive)
            if let Some(drive) = params.drive {
                let drive = drive.clamp(0.0, 1.0);
                sample = sample * (1.0 + drive * 3.0);
                sample = sample.tanh(); // Soft clipping
            }
            
            sample
        }
        
        // --- Биткрашер (Stereo) ---
        pub fn bitcrusher_stereo(input: (f64, f64), state: &ProcessorState) -> (f64, f64) {
            let (left, right) = input;
            let left_processed = bitcrusher_mono(left, state);
            
            // Для стерео можно сделать немного разную обработку каналов
            let mut right_state = state.clone();
            if let Some(ref mut internal) = right_state.internal_state.get_mut(0) {
                *internal = right; // Разные состояния для каналов
            }
            
            let right_processed = bitcrusher_mono(right, &right_state);
            
            (left_processed, right_processed)
        }
        
        // --- Простой ФНЧ (для примера) ---
        pub fn lowpass_mono(input: f64, state: &ProcessorState) -> f64 {
            if let Some(cutoff) = state.params.cutoff {
                let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff);
                let dt = 1.0 / state.sample_rate;
                let alpha = dt / (rc + dt);
                
                let last = state.internal_state.get(0).copied().unwrap_or(0.0);
                let filtered = last + alpha * (input - last);
                
                filtered
            } else {
                input
            }
        }
        
        // Композиция фильтров
        pub fn chain_filters_mono(filters: &[AudioProcessor]) -> AudioProcessor {
            move |input, state| {
                let mut result = input;
                for &filter in filters {
                    result = filter(result, state);
                }
                result
            }
        }
        
        pub fn chain_filters_stereo(filters: &[StereoProcessor]) -> StereoProcessor {
            move |input, state| {
                let mut result = input;
                for &filter in filters {
                    result = filter(result, state);
                }
                result
            }
        }
    }
    
    pub mod mixer {
        use super::*;
        
        // Микширование моно в стерео с панорамой
        pub fn mono_to_stereo(input: f64, pan: f64) -> (f64, f64) {
            let pan = pan.clamp(-1.0, 1.0);
            let left_gain = if pan <= 0.0 { 1.0 } else { 1.0 - pan };
            let right_gain = if pan >= 0.0 { 1.0 } else { 1.0 + pan };
            
            (input * left_gain, input * right_gain)
        }
        
        // Суммирование стерео сигналов
        pub fn sum_stereo(signals: &[(f64, f64)]) -> (f64, f64) {
            signals.iter()
                .fold((0.0, 0.0), |(l_acc, r_acc), &(l, r)| {
                    (l_acc + l, r_acc + r)
                })
        }
        
        // Применение уровня с плавным изменением
        pub fn apply_level((left, right): (f64, f64), level: f64, last_level: f64, smoothness: f64) -> (f64, f64) {
            let smoothed_level = last_level + (level - last_level) * smoothness;
            (left * smoothed_level, right * smoothed_level)
        }
        
        // Ограничитель (limiter)
        pub fn limiter((left, right): (f64, f64), threshold: f64) -> (f64, f64) {
            let limit = |x: f64| {
                if x.abs() > threshold {
                    threshold * x.signum()
                } else {
                    x
                }
            };
            
            (limit(left), limit(right))
        }
        
        // Создание цепочки обработки канала
        pub fn create_channel_processor(
            channel_type: ChannelType,
            filters: &[FilterConfig],
            sample_rate: f64,
        ) -> Either<AudioProcessor, StereoProcessor> {
            // Собираем фильтры для этого канала
            let mono_filters: Vec<AudioProcessor> = filters.iter()
                .filter(|f| f.enabled)
                .map(|f| match f.filter_type {
                    FilterType::Bitcrusher => filters::bitcrusher_mono,
                    FilterType::LowPass => filters::lowpass_mono,
                    _ => |x, _| x, // Passthrough для неподдерживаемых
                })
                .collect();
            
            let stereo_filters: Vec<StereoProcessor> = filters.iter()
                .filter(|f| f.enabled)
                .map(|f| match f.filter_type {
                    FilterType::Bitcrusher => filters::bitcrusher_stereo,
                    _ => |(l, r), _| (l, r), // Passthrough
                })
                .collect();
            
            match channel_type {
                ChannelType::Mono => {
                    let processor = filters::chain_filters_mono(&mono_filters);
                    Either::Left(processor)
                }
                ChannelType::Stereo | ChannelType::DualMono => {
                    let processor = filters::chain_filters_stereo(&stereo_filters);
                    Either::Right(processor)
                }
            }
        }
    }
}

// --- Реактивная система управления микшером ---

#[derive(Debug, Clone)]
pub enum MixerEvent {
    LevelChanged { channel_id: usize, value: f64 },
    PanChanged { channel_id: usize, value: f64 },
    MuteToggled { channel_id: usize, muted: bool },
    SoloToggled { channel_id: usize, soloed: bool },
    FilterToggled { channel_id: usize, filter_idx: usize, enabled: bool },
    FilterParamChanged { channel_id: usize, filter_idx: usize, param: String, value: f64 },
    MasterLevelChanged(f64),
    MasterPanChanged(f64),
    SignalProcessed { inputs: Vec<f64>, outputs: (f64, f64) },
}

pub struct MixerEventSystem {
    tx: broadcast::Sender<MixerEvent>,
}

impl MixerEventSystem {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }
    
    pub fn emit(&self, event: MixerEvent) {
        let _ = self.tx.send(event);
    }
    
    pub fn subscribe(&self) -> broadcast::Receiver<MixerEvent> {
        self.tx.subscribe()
    }
    
    // Реактивные трансформеры
    pub fn channel_events(&self, channel_id: usize) -> impl Stream<Item = MixerEvent> {
        self.subscribe()
            .filter(move |event| {
                match event {
                    MixerEvent::LevelChanged { channel_id: id, .. } => *id == channel_id,
                    MixerEvent::PanChanged { channel_id: id, .. } => *id == channel_id,
                    MixerEvent::MuteToggled { channel_id: id, .. } => *id == channel_id,
                    MixerEvent::SoloToggled { channel_id: id, .. } => *id == channel_id,
                    MixerEvent::FilterToggled { channel_id: id, .. } => *id == channel_id,
                    MixerEvent::FilterParamChanged { channel_id: id, .. } => *id == channel_id,
                    _ => false,
                }
            })
    }
}

// --- Состояние микшера ---

#[derive(Debug, Clone)]
struct ChannelState {
    config: ChannelConfig,
    processor: Either<audio_dsp::AudioProcessor, audio_dsp::StereoProcessor>,
    processor_state: audio_dsp::ProcessorState,
    last_level: f64,
    meter_level: (f64, f64), // Пиковый уровень для VU метра
}

impl ChannelState {
    fn new(config: ChannelConfig, sample_rate: f64) -> Self {
        let processor_state = audio_dsp::ProcessorState::new(
            FilterParams {
                bit_depth: None,
                sample_rate_reduction: None,
                cutoff: None,
                resonance: None,
                drive: None,
            },
            sample_rate,
        );
        
        let processor = audio_dsp::mixer::create_channel_processor(
            config.channel_type,
            &config.filters,
            sample_rate,
        );
        
        Self {
            config,
            processor,
            processor_state,
            last_level: 1.0,
            meter_level: (0.0, 0.0),
        }
    }
    
    fn process(&mut self, input: f64) -> (f64, f64) {
        if self.config.mute {
            return (0.0, 0.0);
        }
        
        match self.processor {
            Either::Left(mono_processor) => {
                // Моно обработка
                let processed = mono_processor(input, &self.processor_state);
                let (left, right) = audio_dsp::mixer::mono_to_stereo(processed, self.config.pan);
                let (left_out, right_out) = audio_dsp::mixer::apply_level(
                    (left, right),
                    self.config.level,
                    self.last_level,
                    0.1, // Smoothing factor
                );
                
                self.last_level = self.config.level;
                self.update_meter(left_out, right_out);
                
                (left_out, right_out)
            }
            Either::Right(stereo_processor) => {
                // Стерео обработка (для простоты дублируем моно вход)
                let processed = stereo_processor((input, input), &self.processor_state);
                let (left_out, right_out) = audio_dsp::mixer::apply_level(
                    processed,
                    self.config.level,
                    self.last_level,
                    0.1,
                );
                
                self.last_level = self.config.level;
                self.update_meter(left_out, right_out);
                
                (left_out, right_out)
            }
        }
    }
    
    fn update_meter(&mut self, left: f64, right: f64) {
        self.meter_level.0 = self.meter_level.0.max(left.abs());
        self.meter_level.1 = self.meter_level.1.max(right.abs());
    }
    
    fn reset_meter(&mut self) {
        self.meter_level = (0.0, 0.0);
    }
}

// --- Главный микшерный модуль ---

pub struct FunctionalMixer {
    config: MixerConfig,
    channels: Vec<ChannelState>,
    master_state: MasterState,
    event_system: MixerEventSystem,
    parameter_updater: mpsc::UnboundedSender<ParameterUpdate>,
}

#[derive(Debug, Clone)]
pub enum ParameterUpdate {
    ChannelLevel { channel_id: usize, value: f64 },
    ChannelPan { channel_id: usize, value: f64 },
    ChannelMute { channel_id: usize, muted: bool },
    ChannelSolo { channel_id: usize, soloed: bool },
    FilterParam { 
        channel_id: usize, 
        filter_idx: usize, 
        param: String, 
        value: f64,
    },
    MasterLevel(f64),
    MasterPan(f64),
}

#[derive(Debug, Clone)]
struct MasterState {
    config: MasterConfig,
    processor_state: audio_dsp::ProcessorState,
    last_level: f64,
}

impl FunctionalMixer {
    pub fn new(config: MixerConfig) -> Result<Self, String> {
        // Валидация
        if config.channels.len() > 5 {
            return Err("Maximum 5 channels allowed".to_string());
        }
        
        // Инициализируем каналы
        let channels = config.channels.iter()
            .cloned()
            .map(|channel_config| {
                ChannelState::new(channel_config, config.sample_rate)
            })
            .collect();
        
        // Инициализируем мастер
        let master_state = MasterState {
            config: config.master.clone(),
            processor_state: audio_dsp::ProcessorState::new(
                FilterParams {
                    bit_depth: None,
                    sample_rate_reduction: None,
                    cutoff: None,
                    resonance: None,
                    drive: None,
                },
                config.sample_rate,
            ),
            last_level: config.master.level,
        };
        
        // Создаем event system
        let event_system = MixerEventSystem::new(100);
        
        // Канал для реактивных обновлений
        let (param_tx, param_rx) = mpsc::unbounded_channel();
        
        let mixer = Self {
            config,
            channels,
            master_state,
            event_system: event_system.clone(),
            parameter_updater: param_tx,
        };
        
        // Запускаем обработчик обновлений
        tokio::spawn(Self::parameter_update_handler(
            param_rx,
            event_system,
            mixer.clone_state(),
        ));
        
        Ok(mixer)
    }
    
    fn clone_state(&self) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(self.clone()))
    }
    
    async fn parameter_update_handler(
        mut rx: mpsc::UnboundedReceiver<ParameterUpdate>,
        events: MixerEventSystem,
        state: Arc<RwLock<Self>>,
    ) {
        while let Some(update) = rx.recv().await {
            let mut state_lock = state.write();
            
            match update {
                ParameterUpdate::ChannelLevel { channel_id, value } => {
                    if let Some(channel) = state_lock.channels.get_mut(channel_id) {
                        channel.config.level = value.clamp(0.0, 1.0);
                        events.emit(MixerEvent::LevelChanged {
                            channel_id,
                            value,
                        });
                    }
                }
                ParameterUpdate::ChannelPan { channel_id, value } => {
                    if let Some(channel) = state_lock.channels.get_mut(channel_id) {
                        channel.config.pan = value.clamp(-1.0, 1.0);
                        events.emit(MixerEvent::PanChanged {
                            channel_id,
                            value,
                        });
                    }
                }
                ParameterUpdate::ChannelMute { channel_id, muted } => {
                    if let Some(channel) = state_lock.channels.get_mut(channel_id) {
                        channel.config.mute = muted;
                        events.emit(MixerEvent::MuteToggled {
                            channel_id,
                            muted,
                        });
                    }
                }
                ParameterUpdate::ChannelSolo { channel_id, soloed } => {
                    if let Some(channel) = state_lock.channels.get_mut(channel_id) {
                        channel.config.solo = soloed;
                        events.emit(MixerEvent::SoloToggled {
                            channel_id,
                            soloed,
                        });
                    }
                }
                ParameterUpdate::FilterParam { 
                    channel_id, 
                    filter_idx, 
                    param, 
                    value 
                } => {
                    if let Some(channel) = state_lock.channels.get_mut(channel_id) {
                        if let Some(filter) = channel.config.filters.get_mut(filter_idx) {
                            match param.as_str() {
                                "bit_depth" => filter.params.bit_depth = Some(value as u8),
                                "sample_rate_reduction" => 
                                    filter.params.sample_rate_reduction = Some(value),
                                "cutoff" => filter.params.cutoff = Some(value),
                                "resonance" => filter.params.resonance = Some(value),
                                "drive" => filter.params.drive = Some(value),
                                _ => {}
                            }
                            
                            events.emit(MixerEvent::FilterParamChanged {
                                channel_id,
                                filter_idx,
                                param,
                                value,
                            });
                        }
                    }
                }
                ParameterUpdate::MasterLevel(value) => {
                    state_lock.master_state.config.level = value.clamp(0.0, 1.0);
                    events.emit(MixerEvent::MasterLevelChanged(value));
                }
                ParameterUpdate::MasterPan(value) => {
                    state_lock.master_state.config.pan = value.clamp(-1.0, 1.0);
                    events.emit(MixerEvent::MasterPanChanged(value));
                }
            }
        }
    }
    
    // Основной метод обработки
    pub fn process(&mut self, inputs: &[f64]) -> (f64, f64) {
        // Проверяем solo состояния
        let any_solo = self.channels.iter().any(|c| c.config.solo);
        
        // Обрабатываем каждый канал
        let mut channel_outputs = Vec::new();
        
        for (i, channel) in self.channels.iter_mut().enumerate() {
            let input = inputs.get(i).copied().unwrap_or(0.0);
            
            // Пропускаем muted каналы или каналы не в solo режиме
            if (any_solo && !channel.config.solo) || channel.config.mute {
                channel.reset_meter();
                continue;
            }
            
            let output = channel.process(input);
            channel_outputs.push(output);
        }
        
        // Суммируем все каналы
        let summed = audio_dsp::mixer::sum_stereo(&channel_outputs);
        
        // Применяем мастер обработку
        let master_output = self.process_master(summed);
        
        // Отправляем событие
        self.event_system.emit(MixerEvent::SignalProcessed {
            inputs: inputs.to_vec(),
            outputs: master_output,
        });
        
        master_output
    }
    
    fn process_master(&mut self, input: (f64, f64)) -> (f64, f64) {
        let (mut left, mut right) = input;
        
        // Применяем панораму мастера
        if self.master_state.config.pan != 0.0 {
            (left, right) = audio_dsp::mixer::mono_to_stereo(
                (left + right) * 0.5, // Среднее для панорамирования
                self.master_state.config.pan,
            );
        }
        
        // Применяем уровень мастера
        let (left_out, right_out) = audio_dsp::mixer::apply_level(
            (left, right),
            self.master_state.config.level,
            self.master_state.last_level,
            0.05, // Меньше сглаживания для мастера
        );
        
        self.master_state.last_level = self.master_state.config.level;
        
        // Применяем лимитер
        let (limited_left, limited_right) = if self.master_state.config.limiter_enabled {
            audio_dsp::mixer::limiter(
                (left_out, right_out),
                self.master_state.config.limiter_threshold,
            )
        } else {
            (left_out, right_out)
        };
        
        (limited_left, limited_right)
    }
    
    // Реактивные методы управления
    
    pub fn set_channel_level(&self, channel_id: usize, level: f64) {
        let _ = self.parameter_updater.send(ParameterUpdate::ChannelLevel {
            channel_id,
            value: level.clamp(0.0, 1.0),
        });
    }
    
    pub fn set_channel_pan(&self, channel_id: usize, pan: f64) {
        let _ = self.parameter_updater.send(ParameterUpdate::ChannelPan {
            channel_id,
            value: pan.clamp(-1.0, 1.0),
        });
    }
    
    pub fn toggle_channel_mute(&self, channel_id: usize) {
        let current = self.channels.get(channel_id)
            .map(|c| c.config.mute)
            .unwrap_or(false);
        
        let _ = self.parameter_updater.send(ParameterUpdate::ChannelMute {
            channel_id,
            muted: !current,
        });
    }
    
    pub fn toggle_channel_solo(&self, channel_id: usize) {
        let current = self.channels.get(channel_id)
            .map(|c| c.config.solo)
            .unwrap_or(false);
        
        let _ = self.parameter_updater.send(ParameterUpdate::ChannelSolo {
            channel_id,
            soloed: !current,
        });
    }
    
    pub fn set_filter_param(
        &self,
        channel_id: usize,
        filter_idx: usize,
        param: &str,
        value: f64,
    ) {
        let _ = self.parameter_updater.send(ParameterUpdate::FilterParam {
            channel_id,
            filter_idx,
            param: param.to_string(),
            value,
        });
    }
    
    pub fn set_master_level(&self, level: f64) {
        let _ = self.parameter_updater.send(ParameterUpdate::MasterLevel(
            level.clamp(0.0, 1.0)
        ));
    }
    
    pub fn set_master_pan(&self, pan: f64) {
        let _ = self.parameter_updater.send(ParameterUpdate::MasterPan(
            pan.clamp(-1.0, 1.0)
        ));
    }
    
    // Подписка на события
    pub fn subscribe(&self) -> broadcast::Receiver<MixerEvent> {
        self.event_system.subscribe()
    }
    
    // Получение текущих уровней для VU метра
    pub fn get_meter_levels(&self) -> Vec<(f64, f64)> {
        self.channels.iter()
            .map(|c| c.meter_level)
            .collect()
    }
    
    // Сброс VU метров
    pub fn reset_meters(&mut self) {
        for channel in &mut self.channels {
            channel.reset_meter();
        }
    }
    
    // Экспорт/импорт конфигурации
    pub fn export_config(&self) -> MixerConfig {
        self.config.clone()
    }
    
    pub fn import_config(&mut self, config: MixerConfig) -> Result<(), String> {
        *self = Self::new(config)?;
        Ok(())
    }
}

// --- Фабрики конфигураций ---

pub struct MixerFactory;

impl MixerFactory {
    pub fn five_channel_stereo() -> MixerConfig {
        let mut channels = Vec::new();
        
        // 5 стерео каналов
        for i in 0..5 {
            let filters = if i == 0 {
                // На первом канале добавляем биткрашер
                vec![
                    FilterConfig {
                        filter_type: FilterType::Bitcrusher,
                        enabled: true,
                        params: FilterParams {
                            bit_depth: Some(8),
                            sample_rate_reduction: Some(0.3),
                            cutoff: None,
                            resonance: None,
                            drive: Some(0.2),
                        },
                        position: 0,
                    }
                ]
            } else {
                Vec::new()
            };
            
            channels.push(ChannelConfig {
                id: i,
                name: format!("Channel {}", i + 1),
                channel_type: ChannelType::Stereo,
                level: if i == 0 { 0.8 } else { 0.7 },
                pan: match i {
                    0 => -0.3,
                    1 => -0.1,
                    2 => 0.0,
                    3 => 0.1,
                    4 => 0.3,
                    _ => 0.0,
                },
                mute: false,
                solo: false,
                filters,
                sends: Vec::new(),
            });
        }
        
        MixerConfig {
            name: "5-Channel Stereo Mixer".to_string(),
            channels,
            master: MasterConfig {
                level: 0.8,
                pan: 0.0,
                filters: vec![
                    FilterConfig {
                        filter_type: FilterType::Bitcrusher,
                        enabled: false, // По умолчанию выключен
                        params: FilterParams {
                            bit_depth: Some(12),
                            sample_rate_reduction: Some(0.1),
                            cutoff: None,
                            resonance: None,
                            drive: Some(0.1),
                        },
                        position: 0,
                    }
                ],
                limiter_enabled: true,
                limiter_threshold: 0.9,
            },
            buses: Vec::new(),
            mode: MixerMode::Normal,
            sample_rate: 44100.0,
        }
    }
    
    pub fn granular_mixer() -> MixerConfig {
        let mut channels = Vec::new();
        
        // 5 каналов с разными типами
        for i in 0..5 {
            let channel_type = match i {
                0 => ChannelType::Mono,
                1 => ChannelType::Stereo,
                2 => ChannelType::DualMono,
                3 => ChannelType::Stereo,
                4 => ChannelType::Mono,
                _ => ChannelType::Mono,
            };
            
            let filters = vec![
                FilterConfig {
                    filter_type: FilterType::Bitcrusher,
                    enabled: i % 2 == 0, // Четные каналы с биткрашером
                    params: FilterParams {
                        bit_depth: Some(4 + i as u8 * 2), // Разная битность
                        sample_rate_reduction: Some(0.1 + i as f64 * 0.1),
                        cutoff: None,
                        resonance: None,
                        drive: Some(0.1),
                    },
                    position: 0,
                }
            ];
            
            channels.push(ChannelConfig {
                id: i,
                name: format!("Granular Ch {}", i + 1),
                channel_type,
                level: 0.7,
                pan: (i as f64 - 2.0) * 0.25, // Распределение по панораме
                mute: false,
                solo: false,
                filters,
                sends: Vec::new(),
            });
        }
        
        MixerConfig {
            name: "Granular Mixer".to_string(),
            channels,
            master: MasterConfig {
                level: 0.8,
                pan: 0.0,
                filters: vec![],
                limiter_enabled: true,
                limiter_threshold: 0.95,
            },
            buses: Vec::new(),
            mode: MixerMode::Parallel,
            sample_rate: 44100.0,
        }
    }
}

// --- Декоратор для advanced routing ---

pub struct RoutingMixer {
    inner: FunctionalMixer,
    routing_matrix: Vec<Vec<f64>>, // [from_channel][to_channel]
    bus_processors: Vec<BusProcessor>,
}

impl RoutingMixer {
    pub fn new(mixer: FunctionalMixer) -> Self {
        let channel_count = mixer.channels.len();
        
        Self {
            inner: mixer,
            routing_matrix: vec![vec![0.0; channel_count]; channel_count],
            bus_processors: Vec::new(),
        }
    }
    
    pub fn route(&mut self, from: usize, to: usize, amount: f64) {
        if from < self.routing_matrix.len() && to < self.routing_matrix[from].len() {
            self.routing_matrix[from][to] = amount.clamp(0.0, 1.0);
        }
    }
    
    pub fn process_with_routing(&mut self, inputs: &[f64]) -> (f64, f64) {
        // Базовая обработка
        let base_output = self.inner.process(inputs);
        
        // Применяем routing матрицу
        let mut routed_signals = vec![0.0; self.routing_matrix.len()];
        
        for (from, row) in self.routing_matrix.iter().enumerate() {
            let channel_output = inputs.get(from).copied().unwrap_or(0.0);
            
            for (to, &amount) in row.iter().enumerate() {
                if amount > 0.0 && to < routed_signals.len() {
                    routed_signals[to] += channel_output * amount;
                }
            }
        }
        
        // Обрабатываем routed сигналы через внутренний микшер
        let routed_output = self.inner.process(&routed_signals);
        
        // Смешиваем с базовым выходом
        let (left_base, right_base) = base_output;
        let (left_routed, right_routed) = routed_output;
        
        (
            (left_base + left_routed) * 0.5,
            (right_base + right_routed) * 0.5,
        )
    }
}

struct BusProcessor {
    id: usize,
    filters: Vec<FilterConfig>,
    level: f64,
}

// --- Пример использования ---

#[tokio::main]
async fn main() {
    println!("=== Functional Reactive Mixer ===\n");
    
    // Создаем конфигурацию
    let config = MixerFactory::five_channel_stereo();
    
    // Инициализируем микшер
    let mut mixer = FunctionalMixer::new(config).unwrap();
    
    // Подписываемся на события
    let mut event_rx = mixer.subscribe();
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            match event {
                MixerEvent::LevelChanged { channel_id, value } => {
                    println!("Channel {} level: {:.2}", channel_id, value);
                }
                MixerEvent::SignalProcessed { inputs, outputs } => {
                    println!("Processed: inputs={:?}, outputs={:?}", 
                        inputs.iter().map(|x| format!("{:.2}", x)).collect::<Vec<_>>(),
                        outputs
                    );
                }
                _ => {}
            }
        }
    });
    
    // Обрабатываем сигналы
    println!("Processing signals:");
    let test_inputs = vec![0.5, 0.3, 0.2, 0.1, 0.4];
    
    for i in 0..3 {
        let output = mixer.process(&test_inputs);
        println!("Step {}: output L={:.3}, R={:.3}", i, output.0, output.1);
        
        // Реактивное управление
        if i == 1 {
            mixer.set_channel_level(0, 0.9);
            mixer.set_master_level(0.7);
            
            // Включаем биткрашер на мастере
            mixer.set_filter_param(0, 0, "bit_depth", 4.0);
            mixer.set_filter_param(0, 0, "sample_rate_reduction", 0.5);
        }
    }
    
    // Получаем уровни VU метра
    let levels = mixer.get_meter_levels();
    println!("\nVU Meter levels: {:?}", levels);
    
    // Тестируем гранулярный микшер
    println!("\n--- Granular Mixer ---");
    let granular_config = MixerFactory::granular_mixer();
    let granular_mixer = FunctionalMixer::new(granular_config).unwrap();
    
    // Тестируем routing микшер
    println!("\n--- Routing Mixer ---");
    let routing_mixer = RoutingMixer::new(mixer);
}

// --- Тесты ---

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_mixer_creation() {
        let config = MixerFactory::five_channel_stereo();
        let mixer = FunctionalMixer::new(config).unwrap();
        
        assert_eq!(mixer.channels.len(), 5);
        assert_eq!(mixer.config.name, "5-Channel Stereo Mixer");
    }
    
    #[tokio::test]
    async fn test_bitcrusher_filter() {
        let mut state = audio_dsp::ProcessorState::new(
            FilterParams {
                bit_depth: Some(8),
                sample_rate_reduction: Some(0.0),
                cutoff: None,
                resonance: None,
                drive: None,
            },
            44100.0,
        );
        
        let input = 0.75;
        let output = audio_dsp::filters::bitcrusher_mono(input, &state);
        
        // Биткрашер должен изменить сигнал
        assert_ne!(input, output);
        assert!(output.abs() <= 1.0);
    }
    
    #[tokio::test]
    async fn test_channel_processing() {
        let config = ChannelConfig {
            id: 0,
            name: "Test".to_string(),
            channel_type: ChannelType::Mono,
            level: 0.8,
            pan: -0.5,
            mute: false,
            solo: false,
            filters: Vec::new(),
            sends: Vec::new(),
        };
        
        let mut channel = ChannelState::new(config, 44100.0);
        let output = channel.process(0.5);
        
        // Проверяем, что выход стерео и в допустимом диапазоне
        assert!(output.0.abs() <= 1.0);
        assert!(output.1.abs() <= 1.0);
        
        // При панораме -0.5 левый канал должен быть громче
        assert!(output.0 > output.1);
    }
    
    #[tokio::test]
    async fn test_reactive_updates() {
        let config = MixerFactory::five_channel_stereo();
        let mixer = FunctionalMixer::new(config).unwrap();
        
        let mut rx = mixer.subscribe();
        
        // Изменяем параметр
        mixer.set_channel_level(0, 0.9);
        
        // Ждем событие
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        // Проверяем, что событие было отправлено
        let mut event_found = false;
        while let Ok(event) = rx.try_recv() {
            if let MixerEvent::LevelChanged { channel_id: 0, value: 0.9 } = event {
                event_found = true;
            }
        }
        
        assert!(event_found);
    }
}