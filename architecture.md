
## architecture.md

```markdown
# Архитектура Kama Audio

## Общая концепция

Kama Audio — это модульная экосистема, построенная вокруг минимального ядра с трейтами. Каждый крейт имеет чёткую ответственность и может использоваться независимо.

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
│  │(OSC сервер)│  │(CLI утилита)│  │(MIDI/HID)  │             │
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
│  │              kama-core-traits                        │    │
│  │  (AudioNode, ParamValue, TimeProvider, и т.д.)       │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

## Базовые крейты (ядро)

### `kama-core-traits`
Минимальное ядро с трейтами и базовыми типами.

```rust
pub trait AudioNode: Send + Sync {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> AudioResult<()>;
    fn get_param(&self, name: &str) -> Option<ParamValue>;
    fn set_param(&mut self, name: &str, value: ParamValue) -> AudioResult<()>;
    fn num_inputs(&self) -> usize;
    fn num_outputs(&self) -> usize;
    // ...
}

pub enum ParamValue { Float(f32), Int(i32), Bool(bool), String(String), Choice(String) }

pub trait TimeProvider: Clock {
    fn bpm(&self) -> f64;
    fn set_bpm(&self, bpm: f64);
    fn tick_info(&self) -> TickInfo;
}
```

### `kama-signal`
Сигнальная система для коммуникации между компонентами.

- `SignalBus<T>` — многопоточная шина с политиками переполнения
- `ParameterChanged`, `ClockTick`, `SystemEvent` — готовые типы сигналов
- `SimpleSignalDispatcher` — синхронная диспетчеризация

## Утилитарные крейты

### `kama-buffers`
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

// Менеджер буферов с пулом
let manager = BufferManager::new();
let buffer = manager.acquire(256)?;
```

### `kama-graph`
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

## Автоматизация и управление

### `kama-automation`
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

### `kama-control`
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

### `kama-dsp-common`
Общие утилиты для DSP.

```rust
// Создание узла из функции
let gain_node = stateless_fn_node(
    "Gain",
    NodeCategory::Effect,
    |sample, ctx| sample * 0.5
);

// Узел с состоянием
let filter_node = stateful_fn_node(
    "OnePole",
    NodeCategory::Filter,
    0.0, // начальное состояние
    |sample, state, ctx| {
        *state = *state * 0.9 + sample * 0.1;
        *state
    }
);

// Макросы для ещё большего упрощения
effect!(Gain, |sample, ctx| sample * 0.5);
filter!(LowPass, |sample, ctx| sample * 0.5 + ctx.seconds().sin() * 0.1);
```

## Цифровые DSP крейты

### `kama-oscillators`
Унифицированные осцилляторы.

```rust
// Аудио осцилляторы (20Hz - 20kHz)
let sine = SineOsc::new(440.0).with_amplitude(0.5);
let saw = SawOsc::new(220.0).with_bandlimited(true);
let noise = NoiseOsc::new().with_type(NoiseType::Pink);

// LFO для модуляции (0.01Hz - 100Hz)
let lfo = Lfo::new(1.0, 0.5, 0.0).with_waveform(LfoWaveform::Triangle);

// Огибающие
let mut envelope = Envelope::new(0.01, 0.1, 0.7, 0.2);
envelope.trigger();
```

### `kama-digital-filters`
Цифровые фильтры.

```rust
// Биквадратные фильтры
let lp = BiquadFilter::new(FilterType::LowPass, 1000.0, 0.707, 0.0);
let hp = BiquadFilter::new(FilterType::HighPass, 200.0, 0.707, 0.0);
let peak = BiquadFilter::new(FilterType::Peak, 1000.0, 2.0, 6.0);
```

### `kama-digital-effects`
Цифровые эффекты.

```rust
let delay = Delay::new(0.3, 0.4, 0.7);
let distortion = Distortion::new(DistortionType::SoftClip, 2.0, 0.8);
let limiter = Limiter::new(-3.0, 0.005, 0.1, 1.0);
```

### `kama-eq`
Эквалайзеры.

```rust
// Параметрический эквалайзер с 5 полосами
let mut para_eq = ParametricEq::new(BiquadFactory, 5, 44100.0);
para_eq.set_band(0, 100.0, 1.0, 3.0)?;
para_eq.set_band(1, 1000.0, 2.0, -2.0)?;

// Графический эквалайзер (1/3 октавы, 31 полоса)
let graphic_eq = GraphicEq::new_third_octave(BiquadFactory, 44100.0);
```

### `kama-mixer`
Микшер с каналами и aux шинами.

```rust
let mut mixer = MixerNode::new(4, 2); // 4 канала, 2 aux шины
mixer.set_channel_pan(0, -0.5)?;
mixer.set_channel_volume(1, 0.8)?;

// Добавление send на aux шину
mixer.add_send(0, SendConfig {
    bus_index: 0,
    level: 0.3,
    send_type: SendType::PostFader,
})?;
```

## Специализированные крейты

### `kama-lofi`
Lo-Fi эмуляция классических систем.

```rust
// NES эмулятор
let mut nes = NesEmulator::new(44100.0);

// Akai S900 (12-bit)
let akai_config = LofiConfig::for_system(ClassicSystem::AkaiS900);
let mut akai = LofiProcessor::new(akai_config);

// AY-3-8910 (ZX Spectrum)
let mut ay = Ay38910Emulator::new(44100.0);
ay.write_register(0, 0x00); // программирование регистров
```

### `kama-hp`
High-precision вычисления (f64).

```rust
let mut hp_buffer = HighPrecisionBuffer::new(1024, 2, 44100.0);
let mut hp_filter = HighPrecisionBiquad::new_lowpass(1000.0, 0.707, 44100.0);
```

## Аудио ввод-вывод (kama-io) - в разработке

### Архитектура kama-io

```
┌─────────────────────────────────────────────────────────┐
│                      AudioEngine                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │              Поток обработки                     │   │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐         │   │
│  │  │ читаем  │→│ процесс │→│ пишем   │         │   │
│  │  │ из буфера│  │ (граф)  │  │ в буфер │         │   │
│  │  └─────────┘  └─────────┘  └─────────┘         │   │
│  └─────────────────────────────────────────────────┘   │
│                         │                               │
│  ┌──────────────────────┼───────────────────────────┐  │
│  ▼                      ▼                           ▼  │
│ ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│ │  ALSABackend │  │  CpalBackend │  │  NullBackend │  │
│ │   (Linux)    │  │(кроссплатформ)│  │  (тестирование)│  │
│ └──────────────┘  └──────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────┘
```

### Ключевые компоненты

```rust
// Трейт AudioBackend
pub trait AudioBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn init(&mut self) -> IoResult<()>;
    fn start(&mut self) -> IoResult<()>;
    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize>;
    fn write(&mut self, buffer: &[f32]) -> IoResult<usize>;
    // ...
}

// Основной движок
pub struct AudioEngine<B: AudioBackend, P: AudioProcessor> {
    backend: B,
    processor: P,
    // ...
}

impl<B, P> AudioEngine<B, P> {
    pub fn start(&mut self) -> IoResult<()>;
    pub fn stop(&mut self) -> IoResult<()>;
    pub fn update_processor<F>(&self, f: F) -> IoResult<()>
    where F: FnOnce(&mut P) + Send + 'static;
}
```

### Процессоры для AudioEngine

```rust
// Базовые процессоры
let pass = PassThroughProcessor;        // пропускает без изменений
let silence = SilenceProcessor;         // генерирует тишину
let gain = GainProcessor::new(0.8);     // усиливает сигнал

// Интеграция с AudioGraph
let graph_processor = GraphProcessor::new(
    graph,
    Some(input_node_id),
    Some(output_node_id)
);
```

## Планируемые крейты

### `kama-wdf`
Wave Digital Filters для аналоговой эмуляции.

```rust
let moog = WdfMoogFilterNode::new(44100.0, 1000.0, 0.7);
let cassette = CassetteDeckNode::new(44100.0);
```

### `kama-server`
OSC сервер для удалённого управления.

```
/node/add sine 440
/connect sine 0 filter 0 1.0
/node/set filter cutoff 800
```

### `drift`
Продукт: сервер эффектов для live coding.

```rust
// Запуск сервера со всеми эффектами
drift --backend alsa --port 9000
```

## Ключевые принципы архитектуры

1. **Минимальное ядро** — только трейты, без реализаций
2. **Модульность** — каждый крейт имеет чёткую ответственность
3. **Композиция** — сложные узлы строятся из простых
4. **Производительность** — zero-cost abstractions, real-time safety
5. **Тестируемость** — все компоненты тестируются изолированно
6. **Расширяемость** — реестр фабрик для динамической загрузки

## Зависимости между крейтами

```
kama-core-traits
├── kama-buffers
├── kama-signal
├── kama-automation
├── kama-control
├── kama-dsp-common
│   ├── kama-oscillators
│   ├── kama-digital-filters
│   ├── kama-digital-effects
│   └── kama-eq
├── kama-graph
├── kama-mixer
├── kama-lofi
├── kama-hp
├── kama-wdf
└── kama-io
    ├── backends (ALSA, CPAL, Null)
    └── processors
```

## Заключение

Архитектура Kama Audio обеспечивает:
- **Гибкость** — можно использовать только нужные крейты
- **Производительность** — оптимизирована для real-time
- **Надёжность** — все компоненты тщательно протестированы
- **Расширяемость** — легко добавлять новые эффекты и бэкенды

Цифровая часть полностью готова. Следующий шаг — завершение аудио-ввода/вывода (kama-io) и создание первого продукта Drift.
```