# Kama Audio 🎵

**Модульная экосистема для создания аудиоприложений на Rust.**

Kama Audio — это не монолит, а набор специализированных крейтов, каждый из которых решает свою задачу. Вы можете использовать только то, что нужно для вашего проекта.

```
kama-core              # единое ядро (трейты + сигналы)
kama-graph             # аудиограф
kama-patchbay          # автоматизация параметров (временно отключен)
kama-core-dsp          # DSP инфраструктура
kama-oscillators       # осцилляторы (аудио и LFO)
kama-digital-filters   # цифровые фильтры
kama-digital-effects   # цифровые эффекты
kama-eq                # эквалайзеры
kama-mixer             # микшер (временно отключен)
kama-lofi              # Lo-Fi эмуляция
kama-io                # аудио ввод-вывод, MIDI
kama-wdf               # Wave Digital Filters (в разработке)
kama-server            # OSC-сервер (в разработке)
```

## 🎯 Зачем это нужно?

- **Для музыкантов**: создавайте свои эффекты и инструменты
- **Для разработчиков**: стройте аудиоприложения на надёжном фундаменте


## ✨ Особенности

- **Единое ядро** — `kama-core` объединяет все базовые трейты и сигнальную систему
- **Минимальные зависимости** — каждый крейт зависит только от того, что реально использует
- **Модульность** — берите только то, что нужно
- **Производительность** — zero-cost abstractions, real-time безопасность
- **Тестируемость** — 20+ тестов, всё проверено
- **Расширяемость** — легко добавить свой эффект или бэкенд

## 🚀 Быстрый старт

Добавьте нужные крейты в `Cargo.toml`:

```toml
[dependencies]
kama-core = "0.3"
kama-graph = "0.3"
kama-oscillators = "0.3"
kama-digital-effects = "0.3"
```

Создайте простой эффект (синус + задержка):

```rust
use kama_core::traits::*;
use kama_core::prelude::*;
use kama_graph::AudioGraph;
use kama_oscillators::audio::SineOsc;
use kama_digital_effects::Delay;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = AudioGraph::new(44100.0);
    
    // Генератор синуса 440Hz
    let sine = SineOsc::new(440.0).with_amplitude(0.5);
    let sine_id = graph.add_node(Box::new(sine));
    
    // Задержка 0.3 сек с обратной связью
    let delay = Delay::new(0.3, 0.4, 0.7);
    let delay_id = graph.add_node(Box::new(delay));
    
    // Соединяем
    graph.connect(
        PortId::output(sine_id, 0),
        PortId::input(delay_id, 0),
        1.0,
    )?;
    
    // Обрабатываем 1 секунду
    let mut output = vec![0.0; 44100];
    let mut outputs = [output.as_mut_slice()];
    graph.process(&[], &mut outputs)?;
    
    println!("Готово! RMS: {:.6}", calculate_rms(&output));
    Ok(())
}

fn calculate_rms(signal: &[f32]) -> f32 {
    let sum: f32 = signal.iter().map(|x| x * x).sum();
    (sum / signal.len() as f32).sqrt()
}
```

## 📦 Состояние крейтов

| Крейт | Версия | Описание |
|-------|--------|----------|
| **kama-core** | 0.3.0 | ✅ **Единое ядро** (трейты + сигналы) |
| kama-core-dsp | 0.3.0 | ✅ DSP инфраструктура, функциональные узлы |
| kama-graph | 0.3.0 | ✅ Аудиограф с топологической сортировкой |
| kama-patchbay | временно отключен | ✅ Автоматизация (LFO, огибающие, сервоприводы) |
| kama-oscillators | 0.3.0 | ✅ Осцилляторы (синус, пила, шум, LFO, огибающие) |
| kama-digital-filters | 0.3.0 | ✅ Биквадратные фильтры (LP, HP, BP, Notch, Peak) |
| kama-digital-effects | 0.3.0 | ✅ Эффекты (Delay, Distortion, Limiter) |
| kama-eq | 0.3.0 | ✅ Параметрический и графический эквалайзеры |
| kama-mixer | временно отключен | 🔊 Микшер с каналами, панорамой и aux шинами |
| kama-lofi | временно отключен | ✅ Lo-Fi эмуляция (NES, AY-3-8910, Akai S900) |
| kama-io | временно отключен | ✅ Аудио ввод-вывод (ALSA, CPAL), MIDI |
| kama-wdf | в разработке | 🔌 Wave Digital Filters (моделирование аналоговых цепей) |
| kama-server | в разработке | 🔌 OSC - сервер |
| kama-tests | планируется | 🧪 Интеграционные тесты |

*Примечание:* Крейт `kama-buffers` интегрирован в `kama-core`.

## 📈 Состояние проекта

Проект находится в стадии активной разработки. Некоторые крейты уже стабильны (например, `kama-core`, `kama-graph`, `kama-patchbay`), другие ещё развиваются. Крейт `kama-wdf` находится в разработке, а `kama-tests` запланирован к реализации. Актуальное состояние архитектуры и дорожная карта доступны в [architecture.md](architecture.md).

## 📊 Зависимости крейтов

Диаграмма зависимостей между крейтами (сплошные стрелки — обязательные зависимости, пунктирные — опциональные):

```mermaid
graph TD
    CORE[kama-core] --> CORE_DSP[kama-core-dsp]
    CORE --> GRAPH[kama-graph]
    CORE --> PATCHBAY[kama-patchbay]
    CORE --> IO[kama-io]
    CORE --> LOFI[kama-lofi]
    CORE_DSP --> OSC[kama-oscillators]
    CORE_DSP --> FILTERS[kama-digital-filters]
    CORE_DSP --> EFFECTS[kama-digital-effects]
    CORE_DSP --> EQ[kama-eq]
    OSC -.-> EFFECTS
    GRAPH -.-> IO
    EFFECTS -.-> IO
```

## 🏗️ Архитектура ядра

```
kama-core/
├── src/
│   ├── lib.rs           # Корневой модуль, реэкспорты
│   ├── math.rs          # Абстракции числовых типов
│   ├── buffer/
│   │   ├── mod.rs       # Буферы (AlignedBuffer, PipeBuffer)
│   │   ├── pipe.rs      # Прямые соединения
│   │   └── ring.rs      # Кольцевые буферы
│   ├── queue/
│   │   ├── mod.rs       # Очереди (RtQueue)
│   │   ├── spsc.rs      # Single-producer single-consumer
│   │   ├── mpsc.rs      # Multi-producer single-consumer
│   │   └── ring.rs      # Кольцевая очередь
│   ├── port.rs          # Порты и идентификаторы
│   ├── node.rs          # Узлы (Source/Processor/Sink)
│   ├── error.rs         # Система ошибок
│   ├── macros/
│   │   ├── mod.rs       # Макросы
│   │   ├── source.rs    # source_node!
│   │   ├── processor.rs # processor_node!
│   │   ├── sink.rs      # sink_node!
│   │   ├── params.rs    # Вспомогательные макросы для параметров
│   │   └── ports.rs     # Вспомогательные макросы для портов
│   ├── graph.rs         # Базовые типы для графа
│   ├── event.rs         # События и сигналы
│   ├── config.rs        # Конфигурация
│   └── utils.rs         # Утилиты
```

## 🧪 Тестирование

```bash
# Все тесты
cargo test --workspace

# Интеграционные тесты
cargo test -p kama-tests -- --nocapture

# Тесты конкретного крейта
cargo test -p kama-digital-effects
```

## 📚 Документация

- [Архитектура проекта](architecture.md) — детальное описание всех компонентов
- [План разработки](plan.org) — текущие задачи и планы
- [Примеры](examples/) — примеры использования каждого крейта

## 🎮 Примеры

```bash
# Базовый пример с AudioGraph
cargo run --example final_demo

# Lo-Fi эмуляция (NES, AY-3-8910)
cargo run --example chiptune_demo

# Гранулярный синтез
cargo run --example granular_processing

# MIDI управление
cargo run --example simple_midi
```

## 🔮 Планы на будущие версии

- ⚡ **Двухпоточная автоматизация** — разделение на control-поток и audio-поток
- 🌐 **kama-server** — выделение OSC в отдельный крейт
- 🔌 **Унификация IO** — объединение audio/MIDI/CV в kama-io

## 🤝 Участие в разработке

Проект открыт для вклада! Особенно нужна помощь с:

- **Аудио бэкендами**: ALSA, CoreAudio, WASAPI, JACK, PipeWire
- **DSP алгоритмами**: новые эффекты, оптимизация существующих
- **Документацией**: примеры, туториалы, переводы
- **Тестированием**: на разных платформах и с разным железом

### Как начать

1. Форкните репозиторий
2. Создайте ветку для фичи (`git checkout -b feature/amazing-effect`)
3. Запустите тесты (`cargo test`)
4. Отправьте пулл-реквест

## 🔄 Процесс разработки (Git Flow)

Kama Audio использует [Git Flow](https://www.atlassian.com/git/tutorials/comparing-workflows/gitflow-workflow) для управления разработкой и релизами.

### Структура веток

- `main` — стабильные релизы
- `develop` — интеграционная ветка для разработки
- `feature/*` — новые возможности
- `release/*` — подготовка релизов
- `hotfix/*` — срочные исправления

### Начало работы

```bash
# Клонируем репозиторий
git clone https://github.com/DigitalRats/kama-audio
cd kama-audio

# Инициализируем git-flow (один раз)
git flow init -d
```

### Создание новой возможности

```bash
# Начинаем новую фичу (от develop)
git flow feature start my-awesome-effect

# Работаем...
git add .
git commit -m "feat(effects): add awesome effect"

# Публикуем (если нужно поделиться)
git flow feature publish my-awesome-effect

# Завершаем фичу (мерж в develop)
git flow feature finish my-awesome-effect
```

### Подготовка релиза

```bash
# Начинаем релиз (от develop)
git flow release start 0.3.0

# Обновляем версии во всех Cargo.toml
./scripts/bump-version.sh 0.3.0

# Обновляем CHANGELOG.md
git add .
git commit -m "chore(release): prepare 0.3.0"

# Финальное тестирование
cargo test --workspace
cargo run --example final_demo

# Завершаем релиз (мерж в main и develop, создаёт тег)
git flow release finish 0.3.0

# Пушим всё (включая теги)
git push --all origin
git push --tags origin
```

### Горячие исправления

```bash
# Начинаем hotfix (от main)
git flow hotfix start 0.2.1

# Фиксим баг
git add .
git commit -m "fix(automation): prevent crash on zero frequency"

# Завершаем hotfix
git flow hotfix finish 0.2.1

# Пушим
git push --all origin
git push --tags origin
```

### Правила коммитов

Используем [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

**Типы:**
- `feat` — новая возможность
- `fix` — исправление бага
- `docs` — документация
- `style` — форматирование кода
- `refactor` — рефакторинг
- `test` — тесты
- `chore` — обслуживание

**Примеры:**
```bash
feat(core): add ParameterId with validation
fix(automation): prevent crash when LFO frequency is zero
docs(readme): add git flow section
test(eq): add frequency response tests
```

### Версионирование

Следуем [Semantic Versioning](https://semver.org/):

- **MAJOR** — несовместимые изменения API
- **MINOR** — новая функциональность с обратной совместимостью
- **PATCH** — исправления багов с обратной совместимостью

Все крейты в workspace версионируются синхронно (одинаковая версия).

## 🤖 Мир автоматов (The World of Automata)

**Kama Patchbay** — это не просто система управления. Это **мир**, в котором живут **автоматы** — загадочные существа, которые чувствуют окружающую среду и влияют на неё. Они общаются на языке сигналов, слышат звук через сенсоры и через серво воздействуют на AudioGraph.

### 🧠 Архитектура мира

```
┌─────────────────────────────────────────────────────┐
│                 МИР АВТОМАТОВ                         │
│  (ваше приложение на Kama Audio)                      │
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
| **LFO** | Пульсирует с заданной частотой | `LfoAutomaton::new("vibrato").with_frequency(5.0)` |
| **Envelope** | Реагирует на события (нажатия) | `EnvelopeAutomaton::new("amp").with_adsr(0.01, 0.1, 0.7, 0.2)` |
| **Random Walk** | Блуждает случайным образом | `RandomWalkAutomaton::new("chaos").with_step(0.1)` |
| **Logic** | Принимает логические решения | `AndAutomaton::new("gate")` |
| **Math** | Вычисляет | `SumAutomaton::new("mixer")` |

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

```rust
// MIDI сенсор
let midi_note = MidiSensor::note("keyboard")
    .with_channel(1);

// CV сенсор (Control Voltage)
let cv_in = CvSensor::new("expression")
    .with_range(0.0, 5.0);
```

### 🎯 Серво — руки (Servo)

Серво — это **исполнительные механизмы** автоматов. Подчиняясь законам природы (неблокирующим очередям), они передают сигналы из мира автоматов в AudioGraph, изменяя параметры звука.

```rust
// Серво, управляющее частотой фильтра
let filter_servo = Servo::new(
    "filter_servo",
    lfo_automaton,  // Какой автомат дает сигнал
    ParameterTarget::new(
        filter_port,
        ParameterId::new("cutoff")?,
        20.0, 20000.0
    )
);

// Серво с обратной связью (адаптивное)
let adaptive_servo = Servo::new(
    "adaptive_servo",
    envelope_automaton,
    ParameterTarget::new(vca_port, ParameterId::new("gain")?, 0.0, 1.0)
).with_feedback(pitch_sensor);  // Может корректировать поведение на основе услышанного
```

### ⚡ Законы природы (неблокирующие очереди)

Мир автоматов и мир звука существуют параллельно. Они связаны **неблокирующими очередями**:

- **Command Queue** — серво отправляют команды в AudioGraph
- **Telemetry Queue** — сенсоры получают данные из AudioGraph

Это позволяет автоматам "думать" в своем темпе, не мешая звуковому потоку.

### 🏭 Пространство автоматов (Patchbay)

**Patchbay** — это место, где живут все ваши автоматы, где расположены их чувства и руки.

```rust
// Создаем новое пространство
let mut world = Patchbay::new("Моя Студия");

// Добавляем автоматы (разум)
world.create_lfo("vibrato");
world.create_envelope("amp");

// Добавляем сенсоры (чувства)
world.add_sensor(Box::new(
    AcousticSensor::new("pitch", Box::new(PitchDetector::new(44100.0)))
        .listening_to("osc_out")
));

// Добавляем серво (руки)
world.add_servo(Box::new(
    Servo::new("vibrato_servo", 
        world.get_automaton("vibrato")?,
        ParameterTarget::new(osc_port, ParameterId::new("frequency")?, 400.0, 480.0))
));

// Оживляем мир
world.awaken();  // Автоматы начинают жить своей жизнью
```

### 🔮 Пример: Говорящий синтезатор

Представьте синтезатор, который **слышит** себя и **адаптируется**:

```rust
// Уши — слышат громкость и высоту тона
let envelope = AcousticSensor::new("envelope",
    Box::new(EnvelopeFollower::new(44100.0)))
    .listening_to("vca_out");

let pitch = AcousticSensor::new("pitch",
    Box::new(PitchDetector::new(44100.0)))
    .listening_to("osc_out");

// Разум — принимает решения
let logic = LogicAutomaton::new("decision")
    .rule("if envelope > 0.8 and pitch < 0.3 then gate = 1");

// Руки — применяют решения к звуку
let servo = Servo::new("effect_servo", logic,
    ParameterTarget::new(effect_port, ParameterId::new("bypass")?, 0.0, 1.0));

world.awaken();  // Синтезатор начинает слышать, думать и реагировать
```

### 📜 Философия

Наши создания:

- **Обладают разумом** — автоматы принимают решения
- **Имеют чувства** — сенсоры воспринимают мир
- **Могут действовать** — серво изменяют звук
- **Подчиняются законам природы** — неблокирующие очереди связывают миры
- **Живут в своем пространстве** — Patchbay объединяет всё

Создавайте своих автоматов, наделяйте их чувствами, давайте им руки и стройте удивительные миры звука.

---

*"В каждом автомате живет частичка души своего создателя"*

[![build](https://img.shields.io/badge/build-passing-brightgreen)](https://github.com/DigitalRats/kama-audio)
[![tests](https://img.shields.io/badge/tests-20%2B-passing)](https://github.com/DigitalRats/kama-audio)
[![version](https://img.shields.io/badge/version-0.3.0-blue)](https://github.com/DigitalRats/kama-audio)
[![license](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)

## Исправленный раздел README.md: kama-graph

## 🔗 kama-graph — аудиограф для реального времени

Библиотека для построения и выполнения гибких аудиографов с поддержкой:
- **Аудио- и control-сигналов** в единой системе
- **Фиксированных блоков** для предсказуемой производительности
- **Топологической сортировки** с обнаружением циклов
- **Zero-copy соединений** через lock-free буферы
- **Двухпоточной архитектуры** с неблокирующими очередями
- **Двунаправленной связи** с миром автоматов (kama-patchbay)

### 🏗️ Основные концепции

```
┌─────────────────────────────────────────────────────────────────────┐
│                         МИР АВТОМАТОВ                                │
│                         (kama-patchbay)                               │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  Автоматы (LFO, Env)   Сенсоры (анализаторы)               │    │
│  │        │                      ▲                             │    │
│  └────────┼──────────────────────┼─────────────────────────────┘    │
│           │ Control Queue        │ Telemetry Queue                   │
│           ▼ (команды)             │ (данные)                          │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                     AUDIOGRAPH                                │    │
│  │  ┌────────┐    ┌────────────┐    ┌────────┐    ┌────────┐   │    │
│  │  │ Source │───►│ Processor  │───►│Processor│───►│  Sink  │   │    │
│  │  └────────┘    └────────────┘    └────────┘    └────────┘   │    │
│  │       │              │                 │            │        │    │
│  │       │  Анализ      │  Телеметрия     │  Контроль  │        │    │
│  │       ▼              ▼                 ▼            ▼        │    │
│  │  ┌──────────────────────────────────────────────────────┐   │    │
│  │  │           Очереди (Command/Telemetry)                │   │    │
│  │  └──────────────────────────────────────────────────────┘   │    │
│  └─────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
```

#### Ключевые компоненты

| Компонент | Назначение |
|-----------|------------|
| **`AudioGraph`** | Контейнер для узлов, управление соединениями, топологическая сортировка |
| **`Processor`** | Пассивный процессор (преобразует входные блоки в выходные) |
| **`Source`** | Активный источник (генерирует аудио, не имеет входов) |
| **`Sink`** | Активный приёмник (потребляет аудио, не имеет выходов) |
| **`PipeBuffer`** | Lock-free соединение между узлами (zero-copy) |
| **`CommandQueue`** | Очередь для получения команд от мира автоматов |
| **`TelemetryQueue`** | Очередь для отправки данных в мир автоматов |
| **`MicroControlObserver`** | Наблюдатель за микро-контролем (нарушения real-time) |

### 🤖 Связь с миром автоматов (kama-patchbay)

Граф и мир автоматов общаются через **две неблокирующие очереди**:

#### 1. **Command Queue** (от автоматов к графу)
```rust
// В мире автоматов (kama-patchbay)
let cmd = SetParameter::new(
    PortId::control_in(filter_id, 0),
    ParameterId::new("cutoff")?,
    1000.0,
    SignalSource::Automaton("lfo1".into())
);
cmd_queue.send(CommandEnum::SetParameter(cmd))?;

// В аудиографе (проверяется перед каждым блоком)
while let Ok(cmd) = graph.command_queue().try_recv() {
    match cmd {
        CommandEnum::SetParameter(sp) => {
            node.set_parameter(&sp.parameter, ParamValue::Float(sp.value))?;
        }
        _ => {} // другие команды игнорируются
    }
}
```

#### 2. **Telemetry Queue** (от графа к автоматам)
```rust
// В аудиографе (после обработки блока)
graph.telemetry_queue().send_parameter(port, param, value)?;
graph.telemetry_queue().send_peak(port, peak_value)?;

// В мире автоматов (сенсоры слушают)
while let Ok(telemetry) = telemetry_rx.try_recv() {
    match telemetry {
        Telemetry::ParameterValue { port, value, .. } => {
            sensor.update(value);
        }
        Telemetry::Peak { port, value, .. } => {
            envelope_follower.update(value);
        }
        _ => {}
    }
}
```

### 🎯 Микро-контроль и наблюдатель

```rust
// Создаём наблюдателя
let observer = MicroControlObserver::with_sender(telemetry_tx);

// Подключаем к графу
graph.connect_observer(observer);

// Где-то в коде (если очень нужно нарушить законы природы)
let result = graph.with_parameter_observed(
    port,
    &param,
    "wild_servo",
    |node, param| {
        // Прямой доступ к параметру (микро-контроль)
        node.set_parameter(param, ParamValue::Float(0.5))
    }
);

// Наблюдатель зафиксирует нарушение, если время превысило порог
```

### 🚀 Быстрый старт

```rust
use kama_graph::prelude::*;
use kama_core::queues::CommandQueue;
use kama_oscillators::SineOsc;
use kama_digital_filters::LowPassFilter;

const BLOCK_SIZE: usize = 64;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Создаём граф
    let mut graph = AudioGraph::<BLOCK_SIZE>::new(44100.0);
    
    // 2. Подключаем очереди для связи с миром автоматов
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (tel_tx, tel_rx) = crossbeam_channel::unbounded();
    
    graph.connect_command_queue(cmd_rx);
    graph.connect_telemetry(tel_tx);
    
    // 3. Добавляем узлы
    let osc_id = graph.add_processor(Box::new(SineOsc::new(440.0)))?;
    let filter_id = graph.add_processor(Box::new(LowPassFilter::new(1000.0)))?;
    let sink_id = graph.add_sink(Box::new(NullSink::new()))?;
    
    // 4. Соединяем
    graph.connect_audio(osc_id, 0, filter_id, 0)?;
    graph.connect_audio(filter_id, 0, sink_id, 0)?;
    
    // 5. Запускаем обработку (Sink активен)
    graph.start()?;
    
    // 6. Где-то в другом потоке мир автоматов может посылать команды
    std::thread::spawn(move || {
        let cmd = SetParameter::new(
            PortId::control_in(filter_id, 0),
            ParameterId::new("cutoff").unwrap(),
            2000.0,
            SignalSource::Manual,
        );
        cmd_tx.send(CommandEnum::SetParameter(cmd)).unwrap();
    });
    
    Ok(())
}
```

### 📊 Типы сигналов

Граф поддерживает два типа сигналов:

| Тип | Описание | Примеры |
|-----|----------|---------|
| **`Audio`** | Высокочастотные аудио-сигналы (обычно 44.1/48 kHz) | Звук с осциллятора, выход фильтра |
| **`Control`** | Низкочастотные управляющие сигналы | LFO, огибающие, выходы анализаторов |

```rust
// Аудио-соединение
graph.connect_audio(osc_id, 0, filter_id, 0)?;

// Control-соединение (для модуляции)
graph.connect_control(lfo_id, 0, filter_id, 1)?;
```

### 🔌 Соединения и буферы

Все соединения между узлами используют **lock-free кольцевые буферы**:

```rust
// Прямое соединение (точка-точка)
graph.connect_audio(source_id, 0, processor_id, 0)?;

// Разветвление (один источник — много получателей)
graph.connect_audio(source_id, 0, processor1_id, 0)?;
graph.connect_audio(source_id, 0, processor2_id, 0)?;

// Суммирование (много источников — один получатель)
graph.connect_audio(source1_id, 0, mixer_id, 0)?;
graph.connect_audio(source2_id, 0, mixer_id, 1)?;
```

### ⚡ Двухпоточная архитектура

```
[Аудио-поток (высокий приоритет)]       [Поток автоматов (низкий приоритет)]
────────────────────────────────────────────────────────────────────────────
         │                                           │
    Обработка блоков                           Автоматы (LFO, Env)
    (pull от Sink)                              │
         │                                      │
    Проверка очереди команд ◄───неблокирующая─── Команды
         │                     очередь
    Отправка телеметрии ─────неблокирующая─────► Сенсоры
         │                     очередь
    Микро-контроль                              │
    (с наблюдением)                              │
         │                                           │
    Статистика и мониторинг                    Обработка сенсоров
```

### 🧪 Примеры

#### Связь с миром автоматов
```rust
// В аудиографе
let mut graph = AudioGraph::<64>::new(44100.0);
graph.connect_command_queue(cmd_rx);
graph.connect_telemetry(tel_tx);
graph.connect_observer(observer);

// В мире автоматов
let lfo = LfoAutomaton::new(5.0);
let servo = Servo::new(
    "filter_servo",
    lfo,
    ParameterTarget::new(filter_port, ParameterId::new("cutoff")?),
    cmd_tx,
);

// LFO будет посылать команды, граф их применять,
// а сенсоры получать телеметрию о результате
```

#### Простой синтезатор
```rust
// Осциллятор → фильтр → усиление
let osc_id = graph.add_processor(SineOsc::new(440.0))?;
let filter_id = graph.add_processor(LowPassFilter::new(1000.0))?;
let gain_id = graph.add_processor(GainNode::new(0.8))?;

graph.connect_audio(osc_id, 0, filter_id, 0)?;
graph.connect_audio(filter_id, 0, gain_id, 0)?;
```

### 📈 Производительность

- **Zero-copy** соединения между узлами
- **Lock-free** операции для real-time безопасности
- **Фиксированные блоки** для предсказуемого поведения
- **Минимальные накладные расходы** (< 5% на граф)
- **Неблокирующие очереди** для межпоточного взаимодействия

### 🔧 Интеграция с другими крейтами

- **`kama-core`** — базовые трейты, буферы, очереди
- **`kama-patchbay`** — мир автоматов (управление и автоматизация)
- **`kama-oscillators`** — готовые генераторы для `Source`
- **`kama-digital-filters`** — фильтры для `Processor`
- **`kama-digital-effects`** — эффекты для `Processor`
- **`kama-io`** — аудио-бэкенды для `Sink` (ALSA, CPAL, JACK)

### 📚 Документация

Полная документация: [docs.rs/kama-graph](https://docs.rs/kama-graph)

Примеры: [github.com/DigitalRats/kama-audio/tree/main/kama-graph/examples](https://github.com/DigitalRats/kama-audio/tree/main/kama-graph/examples)

### 🤝 Философия

Граф — это **звуковой мир**, где всё происходит быстро и детерминированно. 
Мир автоматов — это **мир разума**, где живут LFO, огибающие и сенсоры.
Они общаются через очереди — **законы природы**, которые нельзя нарушать без последствий.

> "В каждом звуке живёт частичка своего создателя, а в каждом автомате — частичка души" 


## 📄 Лицензия

Проект распространяется под лицензией **Apache 2.0**. Это означает, что вы можете:

- ✅ Использовать в коммерческих продуктах
- ✅ Модифицировать и распространять
- ✅ Использовать патентные права
- ❗ При изменениях указывать авторов
- ❗ Сохранять уведомление об авторстве

Полный текст лицензии: [LICENSE-APACHE](LICENSE-APACHE)

Примечание: kama-tests лицензирован под MIT. Полный текст лицензии: [LICENSE-MIT](LICENSE-MIT)

## 🌟 Благодарности

Всем, кто внёс вклад в развитие open-source аудио на Rust:
- [cpal](https://github.com/RustAudio/cpal) — кросс-платформенный аудио ввод-вывод
- [fundsp](https://github.com/SamiPerttu/fundsp) — вдохновение для DSP подходов
- [nih-plug](https://github.com/robbert-vdh/nih-plug) — архитектура плагинов

---

**Kama Audio 0.3.0** — стабильное ядро, чистая архитектура, готовность к следующему этапу. 🚀
