# Архитектура Kama Audio (версия 0.2.0)

## Общая концепция

Kama Audio — это **модульная экосистема**, построенная вокруг минимального ядра с трейтами. Каждый крейт имеет чёткую ответственность и может использоваться независимо. После масштабного рефакторинга 0.2.0 все крейты используют единое ядро `kama-core`.

```
┌─────────────────────────────────────────────────────────────┐
│                         Продукты                             │
│  ┌──────────┐                                                │
│  │  drift   │  (сервер эффектов для live coding)            │
│  └──────────┘                                                │
├─────────────────────────────────────────────────────────────┤
│                       Инфраструктура                          │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐             │
│  │kama-server │  │kama-client │  │kama-control│             │
│  │(планируется)│  │(планируется)│  │(MIDI/HID)  │             │
│  └────────────┘  └────────────┘  └────────────┘             │
├─────────────────────────────────────────────────────────────┤
│                      Обработка звука                          │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              kama-graph (аудиограф)                 │    │
│  └─────────────────────────────────────────────────────┘    │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐      │
│  │осцилляторы│ │ фильтры  │ │ эффекты  │ │эквалайзер│      │
│  │(kama-osc)│ │(kama-dig-│ │(kama-dig-│ │(kama-eq) │      │
│  │          │ │ital-filt)│ │ital-eff) │ │          │      │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘      │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐      │
│  │  микшер  │ │  lo-fi   │ │   wdf    │ │    hp    │      │
│  │(kama-mix)│ │(kama-lofi)│ │(kama-wdf)│ │(kama-hp) │      │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘      │
├─────────────────────────────────────────────────────────────┤
│                      Ввод-вывод                              │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐      │
│  │  ALSA    │ │  CPAL    │ │ PipeWire │ │   JACK   │      │
│  │(kama-io) │ │(kama-io) │ │(kama-io) │ │(kama-io) │      │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘      │
├─────────────────────────────────────────────────────────────┤
│                         Ядро                                 │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                   kama-core                          │    │
│  │  ┌─────────────┐  ┌─────────────┐                  │    │
│  │  │   traits    │  │   signal    │                  │    │
│  │  │ (трейты)    │  │  (сигналы)  │                  │    │
│  │  └─────────────┘  └─────────────┘                  │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

## Единое ядро: kama-core

### Структура

```
kama-core/
├── src/
│   ├── traits/
│   │   ├── mod.rs        # реэкспорты
│   │   ├── error.rs      # AudioError, AudioResult
│   │   ├── node.rs       # AudioNode, NodeId, NodeCategory, NodeMetadata, NodeTypeId
│   │   ├── param.rs      # ParamValue, ParamType, ParamMetadata, ParamRange
│   │   ├── port.rs       # PortId (выделен в отдельный модуль)
│   │   └── time/         # Clock, TimeProvider, TickInfo, SystemClock
│   │       ├── mod.rs
│   │       └── system_clock.rs
│   ├── signal/
│   │   ├── mod.rs        # реэкспорты
│   │   ├── bus.rs        # SignalBus, BusConfig, OverflowPolicy
│   │   ├── dispatcher.rs # SimpleSignalDispatcher
│   │   ├── error.rs      # SignalError, SignalResult
│   │   └── types.rs      # Signal, ParameterChanged, SystemEvent, SignalSource
│   └── prelude.rs        # Удобный реэкспорт всех основных типов
```

### Ключевые компоненты ядра

#### traits (базовые трейты)

```rust
//! Базовый трейт для всех аудиоузлов
pub trait AudioNode: Send + Sync {
    fn node_type_id(&self) -> NodeTypeId;
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> AudioResult<()>;
    fn get_param(&self, name: &str) -> Option<ParamValue>;
    fn set_param(&mut self, name: &str, value: ParamValue) -> AudioResult<()>;
    fn init(&mut self, sample_rate: f32);
    fn reset(&mut self);
    fn num_inputs(&self) -> usize;
    fn num_outputs(&self) -> usize;
    fn metadata(&self) -> NodeMetadata;
}

//! Типизированные параметры
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    String(String),
    Choice(String),
}

//! Идентификаторы
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortId {
    pub node: NodeId,
    pub index: u8,
    pub is_input: bool,
}

//! Временные абстракции
pub trait Clock: Send + Sync + Debug {
    fn sample_rate(&self) -> f64;
    fn position_samples(&self) -> u64;
    fn position_seconds(&self) -> f64 { ... }
    fn advance(&self, samples: u64) -> u64;
    fn reset(&self);
}

pub trait TimeProvider: Clock {
    fn bpm(&self) -> f64;
    fn set_bpm(&self, bpm: f64);
    fn tick_info(&self) -> TickInfo;
}
```

#### signal (сигнальная система)

```rust
//! Сигнал об изменении параметра (версия 0.2.0)
#[derive(Debug, Clone)]
pub struct ParameterChanged {
    pub node_id: String,        // временно String для совместимости
    pub parameter_id: String,    // временно String для совместимости
    pub value: f32,
    pub normalized_value: f32,
    pub timestamp: u64,
    pub source: SignalSource,
}

//! Многопоточная шина сигналов
pub struct SignalBus<T: Signal> {
    tx: Sender<T>,
    rx: Receiver<T>,
    config: BusConfig,
}

impl<T: Signal> SignalBus<T> {
    pub fn new(config: BusConfig) -> Self;
    pub fn send(&self, signal: T) -> SignalResult<()>;
    pub fn try_recv(&self) -> Option<T>;
    pub fn receiver(&self) -> Receiver<T>;
    pub fn sender(&self) -> Sender<T>;
}
```

## Инфраструктурные крейты

### `kama-buffers` (0.2.0)
Управление аудио буферами.

```rust
// Кольцевой буфер с интерполяцией
let mut buffer = RingBuffer::new(1024);
buffer.write(&samples);
buffer.read_interpolated(1.5, &mut output);

// Многоголовый буфер для гранулярного синтеза
let mut multi = MultiHeadBuffer::new(4096, 44100.0);
let head = multi.add_head_with_params(1.0, 0.0, 1.0, ReadMode::Granular {
    grain_size: 256,
    spacing: 512,
    randomization: 0.3,
});
```

### `kama-graph` (0.2.0)
Аудиограф с топологической сортировкой.

```rust
let mut graph = AudioGraph::new(44100.0);
let osc_id = graph.add_node(Box::new(SineOsc::new(440.0)));
let filter_id = graph.add_node(Box::new(BiquadFilter::lowpass(1000.0, 0.707)));

graph.connect(PortId::output(osc_id, 0), PortId::input(filter_id, 0), 1.0)?;

// Автоматическая топологическая сортировка
for &node_id in graph.processing_order() {
    // узлы в правильном порядке
}
```

### `kama-automation` (0.2.0)
Автоматизация параметров.

```rust
let mut manager = AutomationManager::new(time_provider, clock);

// LFO для модуляции частоты фильтра
let lfo = FunctionAutomaton::new(
    "LFO",
    move |time| 500.0 + 400.0 * (time * 0.5).sin(),
    "filter",
    "cutoff"
);

manager.add_servo(Servo::new(
    "lfo1".to_string(),
    Arc::new(lfo),
    "filter".to_string(),
    "cutoff".to_string(),
    ParameterMapping::Linear,
    context,
));
```

### `kama-control` (0.2.0)
MIDI и HID управление.

```rust
let mut midi = MidiBackend::new("Kama Control")?;
midi.open_port(0)?;

let event_rx = midi.subscribe();
let mut control_node = ControlNode::new(event_rx);

// Маппинг MIDI контроллера на параметр
control_node.add_mapping(Mapping::new(
    EventPattern::MidiControl { channel: None, controller: 7 },
    Target { node_id: gain_id, param_name: "gain".to_string(), min: 0.0, max: 1.0 },
    Transform::Exponential,
));
```

## DSP инфраструктура

### `kama-dsp-common` (0.2.0)
Общие утилиты для DSP.

```rust
// Создание узла из функции
let gain_node = stateless_fn_node(
    "Gain",
    NodeCategory::Effect,
    |sample, ctx| sample * 0.5
);

// Макросы для упрощения
effect!(Gain, |sample, ctx| sample * 0.5);
filter!(LowPass, |sample, ctx| sample * 0.5 + ctx.seconds().sin() * 0.1);
```

### `kama-oscillators` (0.2.0)
Унифицированные осцилляторы.

```rust
// Аудио осцилляторы (20Hz - 20kHz)
let sine = SineOsc::new(440.0).with_amplitude(0.5);
let saw = SawOsc::new(220.0).with_bandlimited(true);

// LFO для модуляции (0.01Hz - 100Hz)
let lfo = Lfo::new(1.0, 0.5, 0.0).with_waveform(LfoWaveform::Triangle);

// Огибающие
let mut envelope = Envelope::new(0.01, 0.1, 0.7, 0.2);
envelope.trigger();
```

### `kama-digital-filters` (0.2.0)
Цифровые фильтры.

```rust
let lp = BiquadFilter::new(FilterType::LowPass, 1000.0, 0.707, 0.0);
let hp = BiquadFilter::new(FilterType::HighPass, 200.0, 0.707, 0.0);
let peak = BiquadFilter::new(FilterType::Peak, 1000.0, 2.0, 6.0);
```

### `kama-digital-effects` (0.2.0)
Цифровые эффекты.

```rust
let delay = Delay::new(0.3, 0.4, 0.7);
let distortion = Distortion::new(DistortionType::SoftClip, 2.0, 0.8);
let limiter = Limiter::new(-3.0, 0.005, 0.1, 1.0);
```

### `kama-eq` (0.2.0)
Эквалайзеры.

```rust
// Параметрический эквалайзер с 5 полосами
let mut para_eq = ParametricEq::new(BiquadFactory, 5, 44100.0);
para_eq.set_band(0, 100.0, 1.0, 3.0)?;

// Графический эквалайзер (1/3 октавы, 31 полоса)
let graphic_eq = GraphicEq::new_third_octave(BiquadFactory, 44100.0);
```

### `kama-mixer` (0.2.0)
Микшер с каналами и aux шинами.

```rust
let mut mixer = MixerNode::new(4, 2);
mixer.set_channel_pan(0, -0.5)?;
mixer.set_channel_volume(1, 0.8)?;
```

## Специализированные крейты

### `kama-lofi` (0.2.0)
Lo-Fi эмуляция классических систем.

```rust
// NES эмулятор
let mut nes = NesEmulator::new(44100.0);

// Akai S900 (12-bit)
let akai_config = LofiConfig::for_system(ClassicSystem::AkaiS900);
let mut akai = LofiProcessor::new(akai_config);
```

### `kama-hp` (0.2.0)
High-precision вычисления (f64).

```rust
let mut hp_buffer = HighPrecisionBuffer::new(1024, 2, 44100.0);
let mut hp_filter = HighPrecisionBiquad::new_lowpass(1000.0, 0.707, 44100.0);
```

### `kama-io` (0.2.0)
Аудио ввод-вывод.

```rust
pub trait AudioBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn init(&mut self) -> IoResult<()>;
    fn start(&mut self) -> IoResult<()>;
    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize>;
    fn write(&mut self, buffer: &[f32]) -> IoResult<usize>;
}

// Основной движок
pub struct AudioEngine<B: AudioBackend, P: AudioProcessor> {
    backend: B,
    processor: P,
    // ...
}
```

## Ключевые принципы архитектуры

1. **Единое ядро** — `kama-core` объединяет все базовые трейты и сигнальную систему
2. **Минимальные зависимости** — каждый крейт зависит только от того, что реально использует
3. **Модульность** — каждый крейт имеет чёткую ответственность
4. **Композиция** — сложные узлы строятся из простых
5. **Производительность** — zero-cost abstractions, real-time safety
6. **Тестируемость** — все компоненты тестируются изолированно

## Зависимости между крейтами (версия 0.2.0)

```
kama-core (0.2.0)
├── kama-buffers (0.2.0)
├── kama-graph (0.2.0)
├── kama-automation (0.2.0)
├── kama-control (0.2.0)
├── kama-dsp-common (0.2.0)
├── kama-oscillators (0.2.0)
├── kama-digital-filters (0.2.0)
├── kama-digital-effects (0.2.0)
├── kama-eq (0.2.0)
├── kama-mixer (0.2.0)
├── kama-lofi (0.2.0)
├── kama-hp (0.2.0)
└── kama-io (0.2.0)
```

## Планы на 0.3.0

- 🔄 **ParameterId** — замена `String` на типобезопасный идентификатор
- 📐 **kama-core-math** — обобщённые математические абстракции (Float, AudioNum)
- 🎛️ **Source/Processor/Sync** — типизация узлов по ролям
- ⚡ **Двухпоточная автоматизация** — разделение на control-поток и audio-поток
- 🌐 **kama-osc** — выделение OSC в отдельный крейт
- 🔌 **Унификация IO** — объединение audio/MIDI/CV в kama-io

## Заключение

Архитектура Kama Audio версии 0.2.0 обеспечивает:

- ✅ **Стабильное ядро** — единый крейт с чётким API
- ✅ **Чистую модульность** — каждый крейт имеет свою ответственность
- ✅ **Производительность** — оптимизирована для real-time
- ✅ **Надёжность** — все компоненты тщательно протестированы
- ✅ **Расширяемость** — легко добавлять новые эффекты и бэкенды
- ✅ **Согласованность** — все крейты используют одну версию ядра

Ядро стабилизировано и готово к следующему этапу развития.