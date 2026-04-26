# Архитектура Rill (версия 0.3.0)

## Общая концепция

Rill — это **модульная экосистема**, построенная вокруг минимального ядра с трейтами. Каждый крейт имеет чёткую ответственность и может использоваться независимо. После масштабного рефакторинга 0.3.0 все крейты используют единое ядро `rill-core`.

```
┌─────────────────────────────────────────────────────────────┐
│                         Продукты                             │
│  ┌──────────┐                                                │
│  │  drift   │  (сервер эффектов для live coding)            │
│  └──────────┘                                                │
├─────────────────────────────────────────────────────────────┤
│                       Инфраструктура                          │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐             │
│  │rill-server │  │rill-graph  │  │rill-patchbay│             │
│  │(в разработке)│ │(аудиограф) │ │(автоматизация)│            │
│  └────────────┘  └────────────┘  └────────────┘             │
├─────────────────────────────────────────────────────────────┤
│                      Обработка звука                          │
│  ┌─────────────────────────────────────────────────────┐    │
│  │    rill-core-dsp (алгоритмы + векторные операции)  │    │
│  │   Algorithm trait, генераторы, фильтры, задержки     │    │
│  └─────────────────────────────────────────────────────┘    │
│  ┌──────────┐ ┌───────────────┐ ┌───────────────┐ ┌──────┐ │
│  │rill-osc  │ │rill-digital-  │ │rill-digital-  │ │rill- │ │
│  │(узлы     │ │filters        │ │effects        │ │router│ │
│  │осциллят.)│ │(узлы фильтров)│ │(узлы эффектов)│ │роутер│ │
│  │ активен  │ │ активен       │ │ активен       │ │актив │ │
│  └──────────┘ └───────────────┘ └───────────────┘ └──────┘ │
│  ┌─────────────┐ ┌──────────┐                               │
│  │  rill-lofi  │ │rill-wdf  │                               │
│  │  (lo-fi)    │ │(WDF)     │                               │
│  │  активен    │ │в разработке│                              │
│  └─────────────┘ └──────────┘                               │
├─────────────────────────────────────────────────────────────┤
│                      Ввод-вывод                              │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐      │
│  │  ALSA    │ │  CPAL    │ │ PipeWire │ │   JACK   │      │
│  │(rill-io) │ │(rill-io) │ │(rill-io) │ │(rill-io) │      │
│  │ временно │ │ временно │ │ временно │ │ временно │      │
│  │ отключен │ │ отключен │ │ отключен │ │ отключен │      │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘      │
├─────────────────────────────────────────────────────────────┤
│                         Ядро                                 │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                   rill-core                          │    │
│  │  ┌─────────────┐  ┌─────────────┐                  │    │
│  │  │   traits    │  │   queues    │                  │    │
│  │  │ (трейты)    │  │  (очереди)  │                  │    │
│  │  └─────────────┘  └─────────────┘                  │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

## Единое ядро: rill-core

### Структура

```
rill-core/
├── src/
│   ├── lib.rs                 # Корневой модуль, реэкспорты
│   ├── prelude.rs             # Прелюдия для удобного импорта
│   ├── config.rs              # Конфигурация
│   ├── error.rs               # Система ошибок
│   ├── event.rs               # События и сигналы
│   ├── graph.rs               # Базовые типы для графа
│   ├── utils.rs               # Утилиты
│   ├── traits/
│   │   ├── mod.rs             # Трейты узлов (AudioNode, Source, Processor, Sink)
│   │   ├── node.rs            # Узлы и идентификаторы
│   │   ├── port.rs            # Порты
│   │   ├── param.rs           # Параметры
│   │   ├── processable.rs     # Интерфейс обработки
│   │   └── error.rs           # Ошибки трейтов
│   ├── math/
│   │   ├── mod.rs             # Абстракции числовых типов
│   │   ├── num.rs             # AudioNum трейт
│   │   ├── conversions.rs     # Преобразования
│   │   └── functions.rs       # Функции
│   ├── buffer/
│   │   ├── mod.rs             # Буферы (PipeBuffer, FanOutBuffer и др.)
│   │   ├── pipe.rs            # Прямые соединения
│   │   ├── fan.rs             # Разветвление и суммирование
│   │   ├── delay.rs           # Линия задержки
│   │   ├── ring.rs            # Кольцевой буфер
│   │   ├── storage.rs         # AtomicCell
│   │   └── pool.rs            # Пул буферов
│   ├── queues/
│   │   ├── mod.rs             # Очереди команд и телеметрии
│   │   ├── rt_queue.rs        # Real-time очередь
│   │   ├── spsc.rs            # Single-producer single-consumer
│   │   ├── mpsc.rs            # Multi-producer single-consumer
│   │   ├── ring.rs            # Кольцевая очередь
│   │   ├── command.rs         # Команды
│   │   ├── telemetry.rs       # Телеметрия
│   │   ├── signal.rs          # Сигналы
│   │   ├── observer.rs        # Наблюдатели
│   │   ├── atomic.rs          # Атомарные операции
│   │   └── error.rs           # Ошибки очередей
│   ├── time/
│   │   ├── mod.rs             # Время и тактовые сигналы
│   │   ├── clock.rs           # Трейты Clock и ClockSource
│   │   ├── source.rs          # Реализации источников времени
│   │   ├── tick.rs            # ClockTick
│   │   └── error.rs           # Ошибки времени
│   ├── macros/
│   │   ├── mod.rs             # Макросы
│   │   ├── source.rs          # source_node!
│   │   ├── processor.rs       # processor_node!
│   │   ├── sink.rs            # sink_node!
│   │   ├── params.rs          # Параметры
│   │   ├── ports.rs           # Порты
│   │   └── tests.rs           # Тесты макросов
│   └── executor/
│       └── mod.rs             # Исполнитель графа
```

### Ключевые компоненты ядра

#### buffer (буферы)

Предоставляет типы буферов для передачи аудиоданных между узлами: `PipeBuffer` (однопоточный канал), `FanOutBuffer` (разветвление), `FanInBuffer` (суммирование), `DelayLine` (линия задержки), `RingBuffer` (кольцевой буфер). Все буферы реализуют трейт `AudioBuffer` и поддерживают статистику использования.

```rust
use rill_core::buffer::{PipeBuffer, FanOutBuffer, FanInBuffer, DelayLine, RingBuffer};

let mut pipe = PipeBuffer::new(1024);
pipe.write(&[1.0, 2.0, 3.0]);
let read = pipe.read(3);
```

#### macros (макросы)

Содержит макросы для удобного создания узлов: `processor!`, `sink!`, `source!`. Упрощают написание пользовательских процессоров, источников и приёмников без boilerplate кода.

```rust
use rill_core::macros::{processor, sink, source};

processor!(Gain, |sample, _| sample * 0.5);
sink!(Logger, |sample, _| println!("{}", sample));
source!(Silence, || 0.0);
```

#### math (математика)

Определяет трейт `AudioNum` для аудио‑специфичных числовых операций (преобразование дБ, обёртка фазы), а также функции конвертации и утилиты.

```rust
use rill_core::math::AudioNum;

let db = (-6.0).db_to_linear(); // ≈ 0.501
let phase = 3.0.wrap_phase();   // в диапазоне [0, 2π)
```

#### queues (очереди)

Реализует неблокирующие очереди команд и телеметрии для связи между аудио‑графом и внешним миром. Содержит `CommandQueue`, `TelemetryQueue`, `SignalSource`, `MicroControlObserver` и другие компоненты для управления параметрами в реальном времени.

```rust
use rill_core::queues::{CommandQueue, CommandEnum, SetParameter};

let mut queue = CommandQueue::new();
queue.send(CommandEnum::SetParameter(SetParameter {
    node_id: 1,
    param_id: "cutoff".to_string(),
    value: 1000.0,
}));
```

#### time (время)

Абстракции времени и темпа: трейты `Clock` и `ClockSource`, структуры `SystemClock`, `ClockTick`. Позволяют узлам синхронизироваться с системным временем или внешним темпом.

```rust
use rill_core::time::{Clock, SystemClock};

let clock = SystemClock::new(44100.0);
let pos = clock.position_samples();
clock.advance(64);
```

#### error (ошибки)

Крейт‑уровневые типы ошибок `AudioError` и `AudioResult`. Отделены от `traits/error.rs` (который содержит ошибки трейтов) и используются во всех публичных API ядра.

```rust
use rill_core::{AudioError, AudioResult};

fn safe_process() -> AudioResult<()> {
    Ok(())
}
```

#### prelude (прелюдия)

Удобный реэкспорт наиболее часто используемых типов из всех модулей ядра. Рекомендуется импортировать `use rill_core::prelude::*;` в пользовательском коде.

```rust
use rill_core::prelude::*;
// Теперь доступны AudioNode, AudioNum, PipeBuffer, CommandQueue, Clock и др.
```

## Инфраструктурные крейты



### `rill-graph` (0.3.0)
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

#### Архитектура аудио-графа

Граф Rill построен на строгой математической основе — **теории категорий**, что обеспечивает типобезопасность, композиционность и предсказуемость поведения.

**Ключевые концепции:**

- **Объекты** — блоки семплов фиксированного размера (`[T; BUF_SIZE]`), значения управления (`Control`) и тактовые сигналы (`Clock`).
- **Стрелки (морфизмы)** — процессоры, преобразующие блоки (источники `Source`, процессоры `Processor`, приёмники `Sink`).
- **Композиция** — последовательное соединение узлов образует цепочку обработки.
- **Произведение** — параллельная обработка нескольких сигналов (например, многоканальный миксер).

**Типы портов:** каждый порт характеризуется типом сигнала (`Audio`, `Control`, `Clock`, `Feedback`, `Param`), направлением (вход/выход) и индексом.

**Топологическая сортировка:** граф автоматически определяет порядок обработки узлов, исключая циклические зависимости (за исключением преднамеренных петель обратной связи).

**Реальное время:** все операции над графом гарантированно выполняются за ограниченное время, что критично для аудио‑приложений.

**Блочная обработка:** данные передаются блоками фиксированного размера, что улучшает производительность за счёт локальности кэша и позволяет использовать SIMD‑оптимизации.

### `rill-patchbay` (0.3.0, ✅ активен)
Автоматизация параметров AudioGraph — унификация крейтов `rill-automation` и `rill-control`. Представляет собой центральный фреймворк автоматов (LFO, огибающие, случайные блуждания, секвенсоры), сенсоров (акустические, физические) и сервоприводов, связанных неблокирующими очередями команд и телеметрии. Подробности см. в разделе «Мир автоматов».

```rust
use rill_patchbay::prelude::*;
use rill_core::queues::MpscQueue;
use std::sync::Arc;

// Создаем очередь команд и PatchbayControl
let cmd_queue = Arc::new(MpscQueue::new(1024));
let mut control = PatchbayControl::new(cmd_queue);

// Добавляем LFO-серво
control.add_lfo(
    "vibrato", 5.0, 0.5, 0.0, LfoWaveform::Sine,
    osc_node_id, "frequency", 400.0, 480.0,
);

// Добавляем ADSR-серво
control.add_envelope(
    "amp", 0.01, 0.1, 0.7, 0.2,
    vca_node_id, "gain", 0.0, 1.0,
);

// Маппинг внешних событий (MIDI, OSC)
control.add_mapping_str(
    "midi:7:1",
    filter_node_id, "cutoff",
    20.0, 20000.0, Transform::Logarithmic,
);

// Обновляем автоматы в цикле
control.update(1.0 / 60.0);
```

Либо через `PatchbayManager` с отдельным потоком обновления:

```rust
let mut manager = PatchbayManager::new(
    PatchbayConfig::default(),
    Arc::new(MpscQueue::new(1024)),
);

manager.add_lfo_servo(
    "vibrato", 5.0, 0.5, 0.0, LfoWaveform::Sine,
    osc_node_id, "frequency",
    ParameterMapping::Linear, 400.0, 480.0,
)?;

manager.start()?;  // Автоматы начинают жить своей жизнью
```



### `rill-tests` (планируется)

`rill‑tests` — планируется (набор тестовых утилит и примеров).

## DSP инфраструктура

### `rill-core-dsp` (0.3.0)
Единая DSP инфраструктура с векторными операциями, алгоритмами и макросами. Включает:

- **Векторные абстракции** (`ScalarVector1`, `ScalarVector2`, `ScalarVector4`) — обобщённые числовые типы для переносимых SIMD-операций
- **Генераторы** (`generators/`) — осцилляторы (синус, пила, треугольник, квадрат, пульс), шум, LFO, FM-синтез, огибающие
- **Фильтры** (`filters/`) — биквадратные, однополюсные, SVF, Butterworth, Chebyshev, гребенчатые фильтры
- **Алгоритмы задержки** (`delay`) — Delay, MultiTapDelay, DiffusionDelay, ModulatedDelay
- **Макросы** (`macros/`) — `simple_algorithm!`, `parameterized_algorithm!`, `filter_algorithm!`, `effect_algorithm!`, `generator_algorithm!` для быстрого создания алгоритмов
- **Трейт `Algorithm`** — единый интерфейс для всех DSP-компонентов с блочной обработкой (`process_block`)

Все компоненты используют абстракции `AudioNum` из `rill-core/math` и векторные операции, что обеспечивает переносимость и производительность.

```rust
use rill_core::math::AudioNum;
use rill_core_dsp::generators::basic::SineOsc;
use rill_core_dsp::filters::{BiquadFilter, FilterParams, FilterType};
use rill_core_dsp::algorithm::Algorithm;

let sample_rate = 44100.0;

// Создание генератора синуса
let mut osc = SineOsc::<f32>::new(440.0, sample_rate);
osc.set_amplitude(0.5);

// Создание биквадратного фильтра низких частот
let mut filter = BiquadFilter::<f32>::new(FilterParams {
    filter_type: FilterType::LowPass,
    cutoff: 1000.0,
    q: 0.707,
    gain_db: 0.0,
});

// Обработка блока данных
let mut input = vec![0.0f32; 64];
let mut output = vec![0.0f32; 64];
osc.process_block(&[], &mut input);
filter.process_block(&input, &mut output);
```

### `rill-oscillators` (0.3.0, ✅ активен)
Узлы графа для осцилляторов (синус, пила, треугольник, квадрат, пульс), шума, LFO и огибающих. Реализует трейты `Source`/`Processor` из `rill-core`, используя DSP-алгоритмы из `rill-core-dsp::generators` и векторные абстракции `ScalarVectorN<T>`.

### `rill-digital-filters` (0.3.0, ✅ активен)
Узлы графа для цифровых фильтров: биквадратные, однополюсные, SVF, Butterworth, Chebyshev, гребенчатые. Реализует трейт `Processor` из `rill-core` на базе DSP-алгоритмов из `rill-core-dsp::filters`.

### `rill-digital-effects` (0.3.0, ✅ активен)
Узлы графа для цифровых эффектов: Delay, Distortion, Limiter. Реализует трейт `Processor` из `rill-core`, используя алгоритмы задержки из `rill-core-dsp::delay`. Опциональная фича `modulation` подключает `rill-oscillators` для LFO-модуляции.

### `rill-router` (0.3.0)
Роутер, объединяющий функциональность эквалайзеров (`rill-eq`) и микшера (`rill-mixer`) с возможностью матричной маршрутизации. Включает модули `eq` (графический и параметрический эквалайзеры) и `mixer` (микшер с каналами, посылами, мастером). Планируется добавление модуля `matrix` для гибкой маршрутизации сигналов.

```rust
use rill_router::eq::{GraphicEq, ParametricEq};
use rill_router::mixer::{MixerNode, ChannelConfig};

let mut eq = GraphicEq::new(44100.0);
eq.set_band_gain(0, 3.0)?;

let mut mixer = MixerNode::new(4, 2);
mixer.set_channel_pan(0, -0.5)?;
mixer.set_channel_volume(1, 0.8)?;
```

## Специализированные крейты

### `rill-lofi` (0.3.0, ✅ активен)
Lo-Fi эмуляция классических систем (NES, AY-3-8910, Akai S900). Реализует узлы графа (`AudioNode`) на базе `rill-core`, использующие внутренние DSP-алгоритмы для эмуляции битности, частоты дискретизации и характерных шумов ретро-систем.

```rust
// NES эмулятор
let mut nes = NesEmulator::new(44100.0);

// Akai S900 (12-bit)
let akai_config = LofiConfig::for_system(ClassicSystem::AkaiS900);
let mut akai = LofiProcessor::new(akai_config);
```



### `rill-io` (0.3.0, активен)
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

1. **Единое ядро** — `rill-core` объединяет все базовые трейты и сигнальную систему
2. **Минимальные зависимости** — каждый крейт зависит только от того, что реально использует
3. **Модульность** — каждый крейт имеет чёткую ответственность
4. **Композиция** — сложные узлы строятся из простых
5. **Производительность** — zero-cost abstractions, real-time safety
6. **Тестируемость** — все компоненты тестируются изолированно

## Зависимости между крейтами (версия 0.3.0)

Диаграмма зависимостей между крейтами (сплошные стрелки — обязательные зависимости, пунктирные — опциональные):

```mermaid
graph TD
    CORE[rill-core] --> CORE_DSP[rill-core-dsp]
    CORE --> GRAPH[rill-graph]
    CORE_DSP --> OSC[rill-oscillators]
    CORE_DSP --> FILTERS[rill-digital-filters]
    CORE_DSP --> EFFECTS[rill-digital-effects]
    CORE_DSP --> ROUTER[rill-router]
    CORE --> PATCHBAY[rill-patchbay]
    
    style CORE fill:#90ee90
    style CORE_DSP fill:#90ee90
    style GRAPH fill:#90ee90
    style OSC fill:#90ee90
    style FILTERS fill:#90ee90
    style EFFECTS fill:#90ee90
    style ROUTER fill:#90ee90
    style PATCHBAY fill:#90ee90
    
    %% Разрабатываемые / планируемые / отключенные
    IO[rill-io]
    LOFI[rill-lofi]
    WDF[rill-wdf<br/>(в разработке)]
    SERVER[rill-server<br/>(в разработке)]
    TESTS[rill-tests<br/>(планируется)]
    
    CORE --> IO
    CORE --> LOFI
    CORE -.-> WDF
    CORE -.-> SERVER
    
    style IO fill:#90ee90
    style LOFI fill:#90ee90
    style WDF fill:#cccccc
    style SERVER fill:#cccccc
    style TESTS fill:#cccccc
```

## Мир автоматов

**Rill Patchbay** — это не просто система управления. Это **мир**, в котором живут **автоматы** — загадочные существа, которые чувствуют окружающую среду и влияют на неё. Они общаются на языке сигналов, слышат звук через сенсоры и через серво воздействуют на AudioGraph.

### 🧠 Архитектура мира

```
┌─────────────────────────────────────────────────────┐
│                 МИР АВТОМАТОВ                         │
│  (ваше приложение на Rill)                      │
│                                                       │
│  ┌─────────────────────────────────────────────────┐ │
│  │                    PATCHBAY                       │ │
│  │  ┌─────────────────────────────────────────┐    │ │
│  │  │           АВТОМАТЫ (разум)              │    │ │
│  │  │  ┌──────────┐  ┌──────────┐  ┌──────────┐ │ │
│  │  │  │   LFO    │  │   ENV    │  │  RANDOM  │ │ │
│  │  │  └────┬─────┘  └────┬─────┘  └────┬─────┘ │ │
│  │  │       │             │             │       │ │
│  │  └───────┼─────────────┼─────────────┼───────┘ │ │
│  │          │             │             │         │ │
│  │          ▼             ▼             ▼         │ │
│  │  ┌─────────────────────────────────────────┐   │ │
│  │  │           СЕНСОРЫ (чувства)              │   │ │
│  │  │  • Слышат звук (акустические)           │   │ │
│  │  │  • Чувствуют прикосновения (физические) │   │ │
│  │  │  • Видят MIDI/CV                         │   │ │
│  │  └─────────────────────────────────────────┘   │ │
│  │                   │                              │ │
│  │                   │ Сигналы                      │ │
│  │                   ▼                              │ │
│  │  ┌─────────────────────────────────────────┐   │ │
│  │  │           СЕРВО (руки)                   │   │ │
│  │  │    Применяют сигналы к AudioGraph       │   │ │
│  │  └─────────────────────────────────────────┘   │ │
│  └──────────────────────┬──────────────────────────┘ │
│                         │ Неблокирующие очереди      │
│                         ▼ (Command/Telemetry)        │
│  ┌─────────────────────────────────────────────────┐ │
│  │                 AUDIOGRAPH                        │ │
│  │          (внутренняя схема устройства)            │ │
│  │                                                   │ │
│  │  Осцилляторы → Фильтры → Эффекты → Микшер        │ │
│  └─────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────┘
```

### 🦾 Автоматы — разум (Automaton)

Автоматы — это разумные существа, которые принимают решения и генерируют сигналы. Они могут быть простыми (LFO, огибающая) или сложными (логические схемы, математические преобразователи).

| Автомат | Описание | Как выглядит в коде |
|---------|----------|---------------------|
| **LFO** | Пульсирует с заданной частотой | `LfoAutomaton::new("vibrato", 5.0, 0.5, 0.0, LfoWaveform::Sine)` |
| **Envelope** | Реагирует на события (нажатия) | `EnvelopeAutomaton::adsr("amp", 0.01, 0.1, 0.7, 0.2)` |
| **Random Walk** | Блуждает случайным образом | `RandomAutomaton::walk("chaos", 10.0)` |
| **Sequencer** | Проигрывает последовательность шагов | `SequencerAutomaton::new("seq", steps)` |
| **Function** | Произвольная функция времени | `FunctionAutomaton::new("math", \|t\| (t * 0.5).sin())` |
| **Cellular** | Клеточный автомат (Game of Life, Rule 30) | `CellularAutomaton::game_of_life("life", 16, 16)` |

### 👁️ Сенсоры — чувства (Sensors)

Чтобы автоматы могли воспринимать мир, им нужны органы чувств. Сенсоры преобразуют внешние воздействия в сигналы, понятные автоматам.

#### Акустические сенсоры (слышат звук)

```rust
// Слышит высоту тона
let pitch = AcousticSensor::new("pitch", 
    Box::new(PitchDetector::new(44100.0)))
    .listening_to("osc1_out");  // Слушает выход осциллятора

// Слышит громкость
let envelope = AcousticSensor::new("envelope",
    Box::new(EnvelopeFollower::new(44100.0)
        .with_attack(0.01)
        .with_release(0.1)))
    .listening_to("vca_out");

// Слышит ритм (пересечения нуля)
let rhythm = AcousticSensor::new("rhythm",
    Box::new(ZeroCrossing::new(44100.0)))
    .listening_to("kick_out");
```

#### Физические сенсоры (чувствуют прикосновения)

```rust
// Ручка на передней панели
let cutoff = PhysicalSensor::knob("filter_cutoff")
    .with_range(20.0, 20000.0)
    .with_curve(KnobCurve::Logarithmic);

// Кнопка
let button = PhysicalSensor::button("arpeggio_on");

// Переключатель
let mode = PhysicalSensor::switch("filter_mode")
    .with_positions(vec!["LPF", "BPF", "HPF"]);
```

#### MIDI/CV сенсоры (видят внешний мир)

> **API в разработке.** MIDI и CV сенсоры пока не реализованы — в текущей версии внешние события обрабатываются через `PatchbayControl::handle_event()` и `Mapping`.

```rust
// Планируемый API:
// let midi_note = MidiSensor::note("keyboard")
//     .with_channel(1);
// 
// let cv_in = CvSensor::new("expression")
//     .with_range(0.0, 5.0);
```

### 🎯 Серво — руки (Servo)

Серво — это **исполнительные механизмы** автоматов. Подчиняясь законам природы (неблокирующим очередям), они передают сигналы из мира автоматов в AudioGraph, изменяя параметры звука.

```rust
// Серво, управляющее частотой фильтра
let filter_servo = Servo::new(
    "filter_servo",
    lfo_automaton,          // Какой автомат дает сигнал
    filter_node_id,         // ID узла в AudioGraph
    "cutoff",               // Имя параметра
    ParameterMapping::Linear,
    20.0, 20000.0           // Диапазон значений
);
```

### ⚡ Законы природы (неблокирующие очереди)

Мир автоматов и мир звука существуют параллельно. Они связаны **неблокирующими очередями**:

- **Command Queue** — серво отправляют команды в AudioGraph
- **Telemetry Queue** — сенсоры получают данные из AudioGraph

Это позволяет автоматам "думать" в своем темпе, не мешая звуковому потоку.

### 🏭 Пространство автоматов (Patchbay)

**Patchbay** — это место, где живут все ваши автоматы, где расположены их чувства и руки.

```rust
use rill_patchbay::prelude::*;
use rill_core::queues::MpscQueue;
use std::sync::Arc;

// Создаем очередь команд и PatchbayControl
let cmd_queue = Arc::new(MpscQueue::new(1024));
let mut control = PatchbayControl::new(cmd_queue);

// Добавляем LFO-серво (разум + руки)
control.add_lfo(
    "vibrato", 5.0, 0.5, 0.0,
    LfoWaveform::Sine,
    osc_node_id, "frequency",
    400.0, 480.0,
);

// Добавляем ADSR-серво
control.add_envelope(
    "amp", 0.01, 0.1, 0.7, 0.2,
    vca_node_id, "gain",
    0.0, 1.0,
);

// Обновляем автоматы в цикле
loop {
    control.update(1.0 / 60.0);
    std::thread::sleep(std::time::Duration::from_millis(16));
}
```

Либо через `PatchbayManager` с отдельным потоком обновления:

```rust
let mut manager = PatchbayManager::new(
    PatchbayConfig::default(),
    Arc::new(MpscQueue::new(1024)),
);

manager.add_lfo_servo(
    "vibrato", 5.0, 0.5, 0.0, LfoWaveform::Sine,
    osc_node_id, "frequency",
    ParameterMapping::Linear, 400.0, 480.0,
)?;

manager.start()?;  // Автоматы начинают жить своей жизнью
```

## Планы на будущие версии

- ⚡ **Активация отключенных крейтов** — `rill-io` снова активен (ALSA, CPAL, PipeWire, JACK)
- 🔌 **Развитие rill-core-dsp** — добавление новых алгоритмов, оптимизация векторных операций, поддержка SIMD
- 🌐 **rill-server** — выделение OSC в отдельный крейт (в разработке)
- 🧩 **rill-wdf** — Wave Digital Filters для моделирования аналоговых цепей (в разработке)
- 🚦 **Развитие rill-router** — добавление матричной маршрутизации, расширение модуля `mixer`, интеграция с аудиографом

### 🧪 Тестирование

Rill использует комплексную систему тестирования. Для запуска всех тестов выполните:

```bash
# Все тесты
cargo test --workspace

# Тесты конкретного крейта
cargo test -p rill-patchbay

cargo test -p rill-digital-effects
```

### 📚 Документация

- [README.md](README.md) — общее описание проекта и быстрый старт
- [Архитектура проекта](architecture.md) — детальное описание всех компонентов
- [План разработки](plan.org) — текущие задачи и планы
- [Примеры](examples/) — примеры использования каждого крейта

### 📄 Лицензия

Проект распространяется под лицензией **Apache 2.0**. Это означает, что вы можете:

- ✅ Использовать в коммерческих продуктах
- ✅ Модифицировать и распространять
- ✅ Использовать патентные права
- ❗ При изменениях указывать авторов
- ❗ Сохранять уведомление об авторстве

Полный текст лицензии: [LICENSE-APACHE](LICENSE-APACHE)

Примечание: rill-tests лицензирован под MIT. Полный текст лицензии: [LICENSE-MIT](LICENSE-MIT)

## Заключение

Архитектура Rill версии 0.3.0 обеспечивает:

- ✅ **Стабильное ядро** — единый крейт `rill-core` с чётким API
- ✅ **DSP-алгоритмы** — `rill-core-dsp` содержит трейт `Algorithm` и реализации DSP-алгоритмов (генераторы, фильтры, задержка) с векторными операциями; специализированные крейты (`rill-oscillators`, `rill-digital-filters`, `rill-digital-effects`) предоставляют узлы графа (`AudioNode`) на их основе
- ✅ **Векторные абстракции** — переносимость и производительность через `ScalarVectorN<T>` и трейт `AudioNum`
- ✅ **Чистую модульность** — каждый крейт имеет свою ответственность (некоторые временно отключены)
- ✅ **Производительность** — оптимизирована для real-time, блочная обработка
- ✅ **Надёжность** — все компоненты тщательно протестированы (200+ unit-тестов во всём workspace)
- ✅ **Расширяемость** — легко добавлять новые алгоритмы через макросы и трейт `Algorithm`
- ✅ **Согласованность** — все крейты используют одну версию ядра
- ✅ **Объединение функциональности** — крейты `rill-eq` и `rill-mixer` объединены в `rill-router` (0.3.0) с модулями эквалайзеров и микшера

Рефакторинг 0.3.0 завершён: все крейты переведены на единое ядро `rill-core` и блочную обработку. DSP-алгоритмы собраны в `rill-core-dsp` (трейт `Algorithm`, генераторы, фильтры, задержки, векторные операции). Специализированные крейты (`rill-oscillators`, `rill-digital-filters`, `rill-digital-effects`) предоставляют узлы графа (`AudioNode`), использующие эти алгоритмы. `rill-router` добавлен как единая точка входа для маршрутизации, микширования и эквализации аудиосигналов. Ядро стабилизировано и готово к следующему этапу развития.