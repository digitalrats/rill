# Rill 🎵

[![build](https://img.shields.io/badge/build-passing-brightgreen)](https://github.com/DigitalRats/rill)
[![tests](https://img.shields.io/badge/tests-200%2B-passing)](https://github.com/DigitalRats/rill)
[![version](https://img.shields.io/badge/version-0.3.0-blue)](https://github.com/DigitalRats/rill)
[![license](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)

**Модульная экосистема для создания аудиоприложений на Rust.**

Rill — это не монолит, а набор специализированных крейтов, каждый из которых решает свою задачу. Вы можете использовать только то, что нужно для вашего проекта.

```
┌─────────────────────────────────────────────────────────────┐
│                      Продукты                                │
│  ┌──────────┐                                                │
│  │  drift   │  (сервер эффектов для live coding)            │
│  └──────────┘                                                │
├─────────────────────────────────────────────────────────────┤
│                      Инфраструктура                           │
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
│  │  отключен   │ │в разработке│                              │
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
│                          Ядро                                │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                   rill-core                          │    │
│  │  ┌─────────────┐  ┌─────────────┐                  │    │
│  │  │   traits    │  │   queues    │                  │    │
│  │  │ (трейты)    │  │  (очереди)  │                  │    │
│  │  └─────────────┘  └─────────────┘                  │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

## 🎯 Зачем это нужно?

- **Для музыкантов**: создавайте свои эффекты и инструменты
- **Для разработчиков**: стройте аудиоприложения на надёжном фундаменте


## ✨ Особенности

- **Единое ядро** — `rill-core` объединяет все базовые трейты и сигнальную систему
- **Минимальные зависимости** — каждый крейт зависит только от того, что реально использует
- **Модульность** — берите только то, что нужно
- **Производительность** — zero-cost abstractions, real-time безопасность
- **Тестируемость** — 200+ тестов, всё проверено
- **Расширяемость** — легко добавить свой эффект или бэкенд

## 🚀 Быстрый старт

Добавьте крейт `rill-core-dsp` в `Cargo.toml` (он автоматически подтянет `rill-core`):

```toml
[dependencies]
rill-core-dsp = "0.3"
```

Создайте простой DSP pipeline (синус → задержка):

```rust
use rill_core::math::AudioNum;
use rill_core_dsp::generators::basic::SineOsc;
use rill_core_dsp::delay::Delay;
use rill_core_dsp::algorithm::Algorithm;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sample_rate = 44100.0;
    let block_size = 64;
    
    // Генератор синуса 440 Hz
    let mut osc = SineOsc::<f32>::new(440.0, sample_rate);
    osc.set_amplitude(0.5);
    
    // Задержка 0.3 сек с обратной связью
    let mut delay = Delay::<f32>::new(0.3, sample_rate);
    delay.set_feedback(0.4);
    delay.set_mix(0.7);
    
    // Буферы для обработки
    let mut input_block = vec![0.0f32; block_size];
    let mut output_block = vec![0.0f32; block_size];
    
    // Обрабатываем 1 секунду (несколько блоков)
    let total_samples = sample_rate as usize;
    let mut processed = Vec::with_capacity(total_samples);
    
    for _ in 0..(total_samples / block_size) {
        // Генерируем блок синуса
        osc.process_block(&[], &mut input_block);
        
        // Применяем задержку
        delay.process_block(&input_block, &mut output_block);
        
        // Сохраняем результат
        processed.extend_from_slice(&output_block);
    }
    
    // Вычисляем RMS
    let rms = calculate_rms(&processed);
    println!("Готово! Обработано {} семплов, RMS: {:.6}", processed.len(), rms);
    Ok(())
}

fn calculate_rms(signal: &[f32]) -> f32 {
    let sum: f32 = signal.iter().map(|x| x * x).sum();
    (sum / signal.len() as f32).sqrt()
}
```

Этот пример демонстрирует блочную обработку с использованием алгоритмов из `rill-core-dsp`. Все операции используют векторные абстракции `ScalarVector1<T>` для обеспечения переносимости и производительности.

## 📦 Состояние крейтов

| Крейт | Версия | Описание |
|-------|--------|----------|
| **rill-core** | 0.3.0 | ✅ **Единое ядро** (трейты, сигналы, буферы, очереди, математика) |
| **rill-core-dsp** | 0.3.0 | ✅ **DSP инфраструктура** (векторные операции, алгоритмы, макросы) |
| **rill-graph** | 0.3.0 | ✅ **Аудиограф** с топологической сортировкой |
| **rill-oscillators** | 0.3.0 | ✅ **Осцилляторы** (синус, пила, шум, LFO, огибающие) |
| **rill-digital-filters** | 0.3.0 | ✅ **Цифровые фильтры** (биквадратные, SVF, гребенчатые) |
| **rill-digital-effects** | 0.3.0 | ✅ **Цифровые эффекты** (Delay, Distortion, Limiter) |
| **rill-router** | 0.3.0 | ✅ **Роутер** (эквалайзеры + микшер + матричная маршрутизация) |
| **rill-patchbay** | 0.3.0 | ✅ **Автоматизация** (LFO, огибающие, сервоприводы, маппинг событий) |
| rill-lofi | временно отключен | ⏸️ Lo-Fi эмуляция (NES, AY-3-8910, Akai S900) |
| rill-io | временно отключен | ⏸️ Аудио ввод-вывод (ALSA, CPAL), MIDI |
| rill-wdf | в разработке | 🔌 Wave Digital Filters (моделирование аналоговых цепей) |
| rill-server | в разработке | 🔌 OSC-сервер |
| rill-tests | планируется | 🧪 Интеграционные тесты |

*Примечание:* Крейт `rill-buffers` интегрирован в `rill-core`. Генераторы, фильтры и эффекты (ранее отдельные крейты `rill-oscillators`, `rill-digital-filters`, `rill-digital-effects`) теперь являются частью `rill-core-dsp` и используют единую векторную инфраструктуру. Крейты `rill-eq` и `rill-mixer` объединены в `rill-router` (версия 0.3.0).

## 📈 Состояние проекта

Проект находится в стадии активной разработки. Активны крейты `rill-core` (единое ядро), `rill-core-dsp` (DSP инфраструктура с векторными операциями), `rill-graph` (аудиограф), `rill-oscillators` (осцилляторы), `rill-digital-filters` (цифровые фильтры), `rill-digital-effects` (цифровые эффекты), `rill-router` (роутер) и `rill-patchbay` (автоматизация). Крейты `rill-lofi` и `rill-io` временно отключены. Крейты `rill-wdf` и `rill-server` находятся в разработке, а `rill-tests` запланирован к реализации. Актуальное состояние архитектуры и дорожная карта доступны в [architecture.md](architecture.md).

Рефакторинг 0.3.0 завершён: все крейты переведены на единое ядро `rill-core` и блочную обработку. DSP-алгоритмы собраны в `rill-core-dsp` (трейт `Algorithm`, генераторы, фильтры, задержки, векторные операции). Специализированные крейты (`rill-oscillators`, `rill-digital-filters`, `rill-digital-effects`) предоставляют узлы графа (`AudioNode`), использующие эти алгоритмы. `rill-router` добавлен как единая точка входа для маршрутизации, микширования и эквализации аудиосигналов.

## 📊 Зависимости крейтов

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
    
    %% Временно отключенные / разрабатываемые / планируемые
    IO[rill-io<br/>(отключен)]
    LOFI[rill-lofi<br/>(отключен)]
    WDF[rill-wdf<br/>(в разработке)]
    SERVER[rill-server<br/>(в разработке)]
    TESTS[rill-tests<br/>(планируется)]
    
    CORE -.-> IO
    CORE -.-> LOFI
    CORE -.-> WDF
    CORE -.-> SERVER
    
    style IO fill:#cccccc
    style LOFI fill:#cccccc
    style WDF fill:#cccccc
    style SERVER fill:#cccccc
    style TESTS fill:#cccccc
```

## 🏗️ Архитектура ядра

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

## 🧪 Тестирование

```bash
# Все тесты
cargo test --workspace

# Тесты конкретного крейта
cargo test -p rill-patchbay

cargo test -p rill-digital-effects
```

## 📚 Документация

- [Архитектура проекта](architecture.md) — детальное описание всех компонентов
- [План разработки](plan.org) — текущие задачи и планы
- [Примеры](examples/) — примеры использования каждого крейта

Проект использует [Obsidian](https://obsidian.md/) для управления технической документацией (архитектура, планы, заметки). Конфигурация (`.obsidian/`) уже настроена. Локальные файлы состояния (`workspace.json`) добавлены в `.gitignore`. Markdown-файлы можно читать любым редактором.

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

- ⚡ **Активация отключенных крейтов** — постепенное включение `rill-io`, `rill-lofi` после интеграции с новой векторной инфраструктурой
- 🔌 **Развитие rill-core-dsp** — добавление новых алгоритмов, оптимизация векторных операций, поддержка SIMD
- 🌐 **rill-server** — выделение OSC в отдельный крейт (в разработке)
- 🧩 **rill-wdf** — Wave Digital Filters для моделирования аналоговых цепей (в разработке)
- 🚦 **Развитие rill-router** — добавление матричной маршрутизации, расширение модуля `mixer`, интеграция с аудиографом

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

Rill использует [Git Flow](https://www.atlassian.com/git/tutorials/comparing-workflows/gitflow-workflow) для управления разработкой и релизами.

### Структура веток

- `main` — стабильные релизы
- `develop` — интеграционная ветка для разработки
- `feature/*` — новые возможности
- `release/*` — подготовка релизов
- `hotfix/*` — срочные исправления

### Начало работы

```bash
# Клонируем репозиторий
git clone https://github.com/DigitalRats/rill
cd rill

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

## 🔗 Аудио-граф: математически строгая модель обработки

> **⚠️ Целевой API.** Примеры в этом разделе и в разделе `rill-graph — аудиограф для реального времени` описывают проектируемый интерфейс `rill-graph`. Текущая реализация использует `GraphBuilder` для конструирования графа. Актуальный API см. в [`rill-graph/src/graph.rs`](rill-graph/src/graph.rs).

Граф Rill построен на строгой математической основе — **теории категорий**, что обеспечивает типобезопасность, композиционность и предсказуемость поведения.

### 📐 Математическая модель

В нашей модели:
- **Объекты** — это блоки семплов фиксированного размера (`[T; BUF_SIZE]`)
- **Стрелки (морфизмы)** — это процессоры, преобразующие блоки
- **Композиция** — последовательное соединение узлов
- **Произведение** — параллельная обработка нескольких сигналов

```
Объекты:    A (аудио блок)    C (control значение)    Clock (тактовый сигнал)
             │                  │                       │
Стрелки:    Source: 1 → A      Processor: A → A        Sink: A → 1
```

### 🎯 Типы портов

Каждый порт имеет три характеристики: **тип сигнала**, **направление** и **индекс**.

| Тип порта | Описание | Примеры |
|-----------|----------|---------|
| **`Audio`** | Аудио-сигналы (высокая частота, блоки семплов) | Звук с осциллятора, выход фильтра |
| **`Control`** | Управляющие сигналы (одно значение на блок) | LFO, огибающие, выходы анализаторов |
| **`Clock`** | Тактовые сигналы для синхронизации | Синхроимпульсы от ALSA, внутренний таймер |
| **`Feedback`** | Хранилище состояния между блоками | Линии задержки, состояния фильтров |
| **`Param`** | Параметры узла (не сигналы, а настройки) | Частота среза, коэффициент усиления |

### 🔄 Направление потока: активные и пассивные узлы

В Rill реализована **симметричная модель**, где любой узел может быть активным в зависимости от того, кто инициирует обработку.

#### Сценарий 1: Активный Source, пассивный Sink (воспроизведение)

```
[Активный Source] --(audio)--> [Processor] --(audio)--> [Пассивный Sink]
     │                                                           
     └── (clock) ────────────────────────────────────────────────┘
```

В этом сценарии:
- **Source** (например, осциллятор) содержит цикл обработки
- Он генерирует блок семплов и передаёт его следующему узлу
- Процессоры пассивно обрабатывают данные
- Sink только принимает финальный блок и отправляет его на устройство вывода

```rust
// Пример: активный Source (осциллятор)
impl<T: AudioNum, const BUF_SIZE: usize> Source<T, BUF_SIZE> for SineOsc<T, BUF_SIZE> {
    fn generate(
        &mut self,
        clock: &ClockTick,
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
        outputs: &mut [&mut [T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        // Генерируем блок семплов
        for i in 0..BUF_SIZE {
            outputs[0][i] = (self.phase * T::from_f32(2.0 * PI)).sin() * self.amplitude;
            self.phase = (self.phase + self.frequency / T::from_f32(clock.sample_rate)) % T::ONE;
        }
        Ok(())
    }
}

// Запуск обработки (source.run() содержит цикл)
source.run(&mut processor, &mut sink);
```

#### Сценарий 2: Активный Sink, пассивный Source (захват)

```
[Пассивный Source] --(audio)--> [Processor] --(audio)--> [Активный Sink]
     │                                                           
     └── (clock) ────────────────────────────────────────────────┘
```

В этом сценарии:
- **Sink** (например, ALSA выход) получает тактовый сигнал от железа
- Он запрашивает данные у предыдущего узла
- Процессоры обрабатывают данные в обратном направлении
- Source предоставляет данные (с микрофона или из файла)

```rust
// Пример: активный Sink (ALSA выход)
impl<T: AudioNum, const BUF_SIZE: usize> Sink<T, BUF_SIZE> for AlsaSink<T, BUF_SIZE> {
    fn consume(
        &mut self,
        clock: &ClockTick,
        audio_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
        _feedback_inputs: &[&[T; BUF_SIZE]],
        _control_outputs: &mut [T],
        _clock_outputs: &mut [ClockTick],
    ) -> ProcessResult<()> {
        // Отправляем блок в ALSA
        self.write_buffer(audio_inputs[0])
    }
}

// ALSA callback запускает обработку
fn alsa_callback() {
    let clock = get_current_clock();
    source.acquire(&mut buffer);  // Пассивный Source заполняет буфер
    processor.process(&buffer, &mut buffer);  // Обработка
    sink.consume(&buffer);  // Активный Sink отправляет в ALSA
}
```

### 🔄 Симметрия: один механизм для всех сценариев

Благодаря тактовой синхронизации, оба сценария используют **один и тот же механизм**:

```rust
// Единый интерфейс для всех узлов
trait AudioNode<T: AudioNum, const BUF_SIZE: usize> {
    fn on_clock(
        &mut self,
        clock: &ClockTick,
        audio_inputs: &[&[T; BUF_SIZE]],
        audio_outputs: &mut [&mut [T; BUF_SIZE]],
        control_inputs: &[T],
        control_outputs: &mut [T],
        clock_inputs: &[ClockTick],
        clock_outputs: &mut [ClockTick],
        feedback_inputs: &[&[T; BUF_SIZE]],
        feedback_outputs: &mut [&mut [T; BUF_SIZE]],
    ) -> ProcessResult<()>;
}
```

Разница только в том, **кто инициирует первый тактовый импульс**:
- Воспроизведение: Source получает импульс от внутреннего таймера
- Захват: Sink получает импульс от внешнего железа

### 🧩 Композиция стрелок

В терминах теории категорий, каждый узел — это стрелка. Композиция стрелок даёт цепочку обработки:

```rust
// Композиция: Source ∘ Processor ∘ Sink
let chain = source.then(processor).then(sink);

// Запуск одним вызовом
chain.process(clock);
```

Это математически строго и позволяет строить сложные графы из простых компонентов.

### 🔁 Обратная связь (Feedback)

Реальные аудиосистемы требуют обратной связи (реверберация, резонансные фильтры). В нашей модели это реализуется через **feedback-порты**:

```rust
struct ResonantFilter<T: AudioNum, const BUF_SIZE: usize> {
    // feedback-порты хранят состояние
    feedback: [T; 2],  // y[n-1] и y[n-2]
}

impl<T: AudioNum, const BUF_SIZE: usize> Processor<T, BUF_SIZE> for ResonantFilter<T, BUF_SIZE> {
    fn process(
        &mut self,
        clock: &ClockTick,
        audio_inputs: &[&[T; BUF_SIZE]],
        audio_outputs: &mut [&mut [T; BUF_SIZE]],
        _control_inputs: &[T],
        _control_outputs: &mut [T],
        _clock_inputs: &[ClockTick],
        _clock_outputs: &mut [ClockTick],
        feedback_inputs: &[&[T; BUF_SIZE]],
        feedback_outputs: &mut [&mut [T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        // feedback_inputs[0] = y[n-1], feedback_inputs[1] = y[n-2]
        // feedback_outputs будут сохранены для следующего блока
        Ok(())
    }
}
```

### 📊 Типовые конфигурации графа

#### Линейная цепочка (самая частая)
```
[Source] → [Processor] → [Processor] → [Sink]
```

#### Параллельная обработка (сплиттинг)
```
        ┌→ [Processor A] ─┐
[Source]┤                 ├→ [Mixer] → [Sink]
        └→ [Processor B] ─┘
```

#### Обратная связь (feedback)
```
[Source] → [Processor] → [Delay] → [Sink]
    ↑                        │
    └────────[feedback]──────┘
```

#### Мульти-сценарий (запись и воспроизведение)
```
[Microphone] → [Preamp] → [Splitter] ─┬→ [Reverb] → [Speakers]
                                      └→ [Compressor] → [Recorder]
```

### 🚀 Преимущества такого подхода

1. **Математическая строгость** — каждый узел ведёт себя предсказуемо
2. **Типобезопасность** — компилятор проверяет совместимость портов
3. **Симметрия** — единый механизм для записи и воспроизведения
4. **Композиционность** — сложные системы строятся из простых компонентов
5. **Производительность** — zero-cost abstractions, прямая компиляция
6. **Расширяемость** — новые типы узлов естественно вписываются в модель

### 📚 Примеры использования

#### Простой синтезатор
```rust
let mut graph = AudioGraph::with_sample_rate(44100.0);

let osc = graph.add_source(SineOsc::new(440.0))?;
let filter = graph.add_processor(LowPassFilter::new(1000.0))?;
let gain = graph.add_processor(GainNode::new(0.8))?;
let dac = graph.add_sink(AlsaSink::new("hw:0")?)?;

graph.connect_audio(osc, 0, filter, 0)?;
graph.connect_audio(filter, 0, gain, 0)?;
graph.connect_audio(gain, 0, dac, 0)?;

// Source активен, запускает обработку
graph.run()?;
```

#### Запись с микрофона с обработкой
```rust
let mic = graph.add_source(AlsaSource::new("hw:0")?)?;
let gate = graph.add_processor(NoiseGate::new(-40.0))?;
let compressor = graph.add_processor(Compressor::new(2.0, 10.0))?;
let recorder = graph.add_sink(WavFileSink::new("recording.wav")?)?;

graph.connect_audio(mic, 0, gate, 0)?;
graph.connect_audio(gate, 0, compressor, 0)?;
graph.connect_audio(compressor, 0, recorder, 0)?;

// Sink активен, запускается по сигналу от ALSA
graph.run_capture()?;
```


## 🤖 Мир автоматов (The World of Automata)

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
    "vibrato",              // Имя
    5.0, 0.5, 0.0,          // freq, amp, offset
    LfoWaveform::Sine,      // Форма волны
    osc_node_id,            // Целевой узел AudioGraph
    "frequency",            // Целевой параметр
    400.0, 480.0,           // Диапазон
);

// Добавляем ADSR-серво
control.add_envelope(
    "amp", 0.01, 0.1, 0.7, 0.2,
    vca_node_id, "gain",
    0.0, 1.0,
);

// Внешние события обрабатываются через маппинг
control.add_mapping_str(
    "midi:7:1",             // MIDI CC 7, channel 1
    filter_node_id,
    "cutoff",
    20.0, 20000.0,
    Transform::Logarithmic,
);

// Обновляем автоматы в цикле (60 FPS)
loop {
    control.update(1.0 / 60.0);
    // control.handle_event(event) — для внешних событий
    std::thread::sleep(std::time::Duration::from_millis(16));
}
```

Либо через `PatchbayManager` с отдельным потоком обновления:

```rust
let cmd_queue = Arc::new(MpscQueue::new(1024));
let mut manager = PatchbayManager::new(
    PatchbayConfig::default(),
    cmd_queue,
);

// Добавляем LFO-серво через менеджер
manager.add_lfo_servo(
    "vibrato", 5.0, 0.5, 0.0, LfoWaveform::Sine,
    osc_node_id, "frequency",
    ParameterMapping::Linear, 400.0, 480.0,
)?;

manager.start()?;  // Автоматы начинают жить своей жизнью
```

### 🔮 Пример: Говорящий синтезатор

Представьте синтезатор, который **слышит** себя и **адаптируется**:

```rust
use rill_patchbay::prelude::*;
use rill_patchbay::sensor::acoustic::*;

// Уши — слышат громкость и высоту тона (API сенсоров в разработке)
let envelope = AcousticSensor::new("envelope",
    Box::new(EnvelopeFollower::new(44100.0)))
    .listening_to("vca_out");

let pitch = AcousticSensor::new("pitch",
    Box::new(PitchDetector::new(44100.0)))
    .listening_to("osc_out");

// Разум — функция-автомат принимает решения
let logic = FunctionAutomaton::new("decision", |t: f64| {
    // Имитация логики: комбинируем сигналы сенсоров
    (t.sin() * 0.5 + 0.5).min(1.0).max(0.0)
});

// Руки — применяют решения к звуку
let servo = Servo::new(
    "effect_servo", logic,
    effect_node_id, "bypass",
    ParameterMapping::Linear, 0.0, 1.0,
);
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

## 🔗 rill-graph — иммутабельный аудиограф (Static DAG)

Библиотека для построения и выполнения иммутабельных аудиографов со статической топологией DAG:

- **Immutable-граф** — топология фиксируется при сборке, `AudioGraph` не имеет методов модификации
- **`GraphBuilder`** — единственный способ построить граф (source → processor → sink)
- **Алгоритм Кана** — топологическая сортировка с обнаружением циклов
- **Port-маршрутизация** — соединения и буферы обратной связи хранятся на портах
- **Copy-based routing** — буферы копируются между портами (zero-copy в планах)
- **Поддержка Feedback** — отложенная обратная связь через `feedback_buffer`
- **Safe Rust** — никакого `unsafe` кода

### 🏗️ Архитектура

```
┌─────────────────────────────────────────────────────┐
│                    GraphBuilder                      │
│  add_source() → idx  add_processor() → idx          │
│  add_sink() → idx    connect_audio(from, to)        │
│  connect_feedback(from, to)    build() → AudioGraph │
└──────────────────────┬──────────────────────────────┘
                       │ consume
                       ▼
┌─────────────────────────────────────────────────────┐
│                    AudioGraph                        │
│  ┌────────┐   ┌────────────┐   ┌────────┐          │
│  │ Source │──►│ Processor  │──►│  Sink  │  ...      │
│  └────────┘   └────────────┘   └────────┘          │
│                                                     │
│  read-only: output_buffer(), current_tick(),         │
│  node_count(), topo_order()                          │
│  НЕТ методов модификации или process()              │
└─────────────────────────────────────────────────────┘
                       │ внешний processing loop
                       ▼
┌─────────────────────────────────────────────────────┐
│              Port-level processing                   │
│  pre_process(tick) → snapshot_feedback() →           │
│  node.process_block() → propagate(tick)              │
└─────────────────────────────────────────────────────┘
```

#### Ключевые компоненты

| Компонент | Назначение |
|-----------|------------|
| **`GraphBuilder`** | Mutable-строитель: добавляет узлы и соединения, собирает `AudioGraph` |
| **`AudioGraph`** | Immutable-контейнер с топологией DAG, без методов обработки |
| **`Port`** | Владение буфером, downstream-маршрутами и feedback-состоянием |
| **`BuildError`** | `CycleDetected` — возвращается при обнаружении цикла |

### 🚀 Быстрый старт

```rust
use rill_graph::prelude::*;
use rill_core::traits::*;
use rill_core::time::SystemClock;

const BUF_SIZE: usize = 64;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Строим граф через GraphBuilder
    let mut builder = GraphBuilder::<f32, BUF_SIZE>::new();

    let src = builder.add_source(Box::new(MySource::new(440.0, 44100.0)));
    let proc = builder.add_processor(Box::new(MyProcessor::new(44100.0)));
    let sink = builder.add_sink(Box::new(MySink::new(44100.0)));

    builder.connect_audio(src, 0, proc, 0);
    builder.connect_audio(proc, 0, sink, 0);

    // 2. Собираем иммутабельный граф
    let graph = builder.build(Box::new(SystemClock::with_sample_rate(44100.0)))?;

    // 3. Внешний processing loop
    // for &idx in graph.topo_order() {
    //     let node = graph.node_mut(idx);
    //     // pre_process → process_block → snapshot_feedback → propagate
    // }

    Ok(())
}
```

### 🔌 Соединения

```rust
// Аудио-соединение (становится Port::downstream при build())
builder.connect_audio(source_id, 0, processor_id, 0);

// Feedback-соединение (создаёт Port::feedback_buffer + feedback_downstream)
builder.connect_feedback(processor_id, 0, source_id, 0);
```

### 🔄 Processing Loop (внешний)

`AudioGraph` не содержит цикла обработки. Внешний код управляет им:

```rust
for &idx in graph.topo_order() {
    // Получаем node
    let node: &mut NodeVariant<f32, BUF_SIZE> = ...;

    // 1. pre_process — mix feedback во входные буферы
    node.input_ports.iter_mut().for_each(|p| p.pre_process(&tick));

    // 2. Обработка DSP узла
    node.process_block(&tick, &inputs, &mut outputs)?;

    // 3. snapshot_feedback — сохранить feedback для следующего блока
    node.output_ports.iter_mut().for_each(|p| p.snapshot_feedback());

    // 4. propagate — скопировать выходные буферы в downstream входы
    node.output_ports.iter().for_each(|p| p.propagate(&tick, &mut nodes));
}
```

### 📈 Производительность

- **Copy-based routing** — предсказуемое O(n) поведение
- **Фиксированные блоки** — детерминированное время обработки
- **Топологическая сортировка** — гарантированный порядок без deadlock
- **Port-owned данные** — нет блокировок при маршрутизации

### 🔧 Интеграция с другими крейтами

- **`rill-core`** — базовые трейты (`AudioNode`, `Source`, `Processor`, `Sink`), буферы, порты, типы времени
- **`rill-oscillators`** — готовые реализации `Source` (генераторы)
- **`rill-digital-filters`** — готовые реализации `Processor` (фильтры)
- **`rill-digital-effects`** — готовые реализации `Processor` (эффекты)

### 📚 Документация

Полная документация: [docs.rs/rill-graph](https://docs.rs/rill-graph)

Примеры: [github.com/DigitalRats/rill/tree/main/rill-graph/examples](https://github.com/DigitalRats/rill/tree/main/rill-graph/examples)

### 🤝 Философия

Граф — это **чистая топология**: неизменяемый DAG, построенный один раз.
Обработка вынесена вовне. Port — единственная точка маршрутизации: ему принадлежат буфер, downstream-связи и состояние feedback.

> "Чистая топология — детерминированная обработка" 


## 📄 Лицензия

Проект распространяется под лицензией **Apache 2.0**. Это означает, что вы можете:

- ✅ Использовать в коммерческих продуктах
- ✅ Модифицировать и распространять
- ✅ Использовать патентные права
- ❗ При изменениях указывать авторов
- ❗ Сохранять уведомление об авторстве

Полный текст лицензии: [LICENSE-APACHE](LICENSE-APACHE)

Примечание: rill-tests лицензирован под MIT. Полный текст лицензии: [LICENSE-MIT](LICENSE-MIT)

## 🌟 Благодарности

Всем, кто внёс вклад в развитие open-source аудио на Rust:
- [cpal](https://github.com/RustAudio/cpal) — кросс-платформенный аудио ввод-вывод
- [fundsp](https://github.com/SamiPerttu/fundsp) — вдохновение для DSP подходов
- [nih-plug](https://github.com/robbert-vdh/nih-plug) — архитектура плагинов

---

**Rill 0.3.0** — стабильное ядро, чистая архитектура, готовность к следующему этапу. 🚀
