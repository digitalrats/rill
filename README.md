# Rill 🎵

[![build](https://img.shields.io/badge/build-passing-brightgreen)](https://github.com/DigitalRats/rill)
[![tests](https://img.shields.io/badge/tests-20%2B-passing)](https://github.com/DigitalRats/rill)
[![version](https://img.shields.io/badge/version-0.3.0-blue)](https://github.com/DigitalRats/rill)
[![license](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)

**Модульная экосистема для создания аудиоприложений на Rust.**

Rill — это не монолит, а набор специализированных крейтов, каждый из которых решает свою задачу. Вы можете использовать только то, что нужно для вашего проекта.

```
rill-core              # единое ядро (трейты + сигналы)
rill-core-dsp          # DSP инфраструктура
rill-graph             # аудиограф
rill-patchbay          # автоматизация параметров (временно отключен)
rill-oscillators       # осцилляторы (аудио и LFO)
rill-digital-filters   # цифровые фильтры
rill-digital-effects   # цифровые эффекты
rill-router            # роутер (эквалайзеры + микшер)
rill-lofi              # Lo-Fi эмуляция (временно отключен)
rill-io                # аудио ввод-вывод, MIDI (в разработке)
rill-wdf               # Wave Digital Filters (планируется)
rill-server            # OSC-сервер (в разработке)
rill-sequencer         # секвенсеры (планируется)
```

## 🎯 Зачем это нужно?

- **Для музыкантов**: создавайте свои эффекты и инструменты
- **Для разработчиков**: стройте аудиоприложения на надёжном фундаменте


## ✨ Особенности

- **Единое ядро** — `rill-core` объединяет все базовые трейты и сигнальную систему
- **Минимальные зависимости** — каждый крейт зависит только от того, что реально использует
- **Модульность** — берите только то, что нужно
- **Производительность** — zero-cost abstractions, real-time безопасность
- **Тестируемость** — 20+ тестов, всё проверено
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
| **rill-router** | 0.4.0 | ✅ **Роутер** (эквалайзеры + микшер + матричная маршрутизация) |
| rill-patchbay | 0.3.0 | ⏸️ Автоматизация (LFO, огибающие, сервоприводы) (временно отключен) |
| rill-lofi | временно отключен | ⏸️ Lo-Fi эмуляция (NES, AY-3-8910, Akai S900) |
| rill-io | временно отключен | ⏸️ Аудио ввод-вывод (ALSA, CPAL), MIDI |
| rill-wdf | в разработке | 🔌 Wave Digital Filters (моделирование аналоговых цепей) |
| rill-server | в разработке | 🔌 OSC-сервер |
| rill-tests | планируется | 🧪 Интеграционные тесты |

*Примечание:* Крейт `rill-buffers` интегрирован в `rill-core`. Генераторы, фильтры и эффекты (ранее отдельные крейты `rill-oscillators`, `rill-digital-filters`, `rill-digital-effects`) теперь являются частью `rill-core-dsp` и используют единую векторную инфраструктуру. Крейты `rill-eq` и `rill-mixer` объединены в `rill-router` (версия 0.4.0).

## 📈 Состояние проекта

Проект находится в стадии активной разработки. Активны крейты `rill-core` (единое ядро), `rill-core-dsp` (DSP инфраструктура с векторными операциями), `rill-graph` (аудиограф), `rill-oscillators` (осцилляторы), `rill-digital-filters` (цифровые фильтры), `rill-digital-effects` (цифровые эффекты) и `rill-router` (роутер). Крейты `rill-patchbay`, `rill-lofi`, `rill-io` временно отключены. Крейты `rill-wdf` и `rill-server` находятся в разработке, а `rill-tests` запланирован к реализации. Актуальное состояние архитектуры и дорожная карта доступны в [architecture.md](architecture.md).

## 📊 Зависимости крейтов

Диаграмма зависимостей между крейтами (сплошные стрелки — обязательные зависимости, пунктирные — опциональные):

```mermaid
graph TD
    CORE[rill-core] --> CORE_DSP[rill-core-dsp]
    CORE_DSP --> GRAPH[rill-graph]
    CORE_DSP --> OSC[rill-oscillators]
    CORE_DSP --> FILTERS[rill-digital-filters]
    CORE_DSP --> EFFECTS[rill-digital-effects]
    CORE_DSP --> ROUTER[rill-router]
    
    style CORE fill:#90ee90
    style CORE_DSP fill:#90ee90
    style GRAPH fill:#90ee90
    style OSC fill:#90ee90
    style FILTERS fill:#90ee90
    style EFFECTS fill:#90ee90
    style ROUTER fill:#90ee90
    
    %% Временно отключенные крейты
    PATCHBAY[rill-patchbay(отключен)]
    IO[rill-io(отключен)]
    LOFI[rill-lofi(отключен)]
    
    CORE -.-> PATCHBAY
    CORE -.-> IO
    CORE -.-> LOFI
    
    style PATCHBAY fill:#cccccc
    style IO fill:#cccccc
    style LOFI fill:#cccccc
    
    %% Объединенные крейты
    %% rill-eq и rill-mixer объединены в rill-router
```

## 🏗️ Архитектура ядра

```
rill-core/
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
cargo test -p rill-tests -- --nocapture

# Тесты конкретного крейта
cargo test -p rill-digital-effects
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
- 🌐 **rill-server** — выделение OSC в отдельный крейт
- 🔌 **Унификация IO** — объединение audio/MIDI/CV в rill-io

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

## 🔗 rill-graph — аудиограф для реального времени

Библиотека для построения и выполнения гибких аудиографов с поддержкой:
- **Аудио- и control-сигналов** в единой системе
- **Фиксированных блоков** для предсказуемой производительности
- **Топологической сортировки** с обнаружением циклов
- **Zero-copy соединений** через lock-free буферы
- **Двухпоточной архитектуры** с неблокирующими очередями
- **Двунаправленной связи** с миром автоматов (rill-patchbay)

### 🏗️ Основные концепции

```
┌─────────────────────────────────────────────────────────────────────┐
│                         МИР АВТОМАТОВ                                │
│                         (rill-patchbay)                               │
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

### 🤖 Связь с миром автоматов (rill-patchbay)

Граф и мир автоматов общаются через **две неблокирующие очереди**:

#### 1. **Command Queue** (от автоматов к графу)
```rust
// В мире автоматов (rill-patchbay)
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
use rill_graph::prelude::*;
use rill_core::queues::CommandQueue;
use rill_oscillators::SineOsc;
use rill_digital_filters::LowPassFilter;

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

- **`rill-core`** — базовые трейты, буферы, очереди
- **`rill-patchbay`** — мир автоматов (управление и автоматизация)
- **`rill-oscillators`** — готовые генераторы для `Source`
- **`rill-digital-filters`** — фильтры для `Processor`
- **`rill-digital-effects`** — эффекты для `Processor`
- **`rill-io`** — аудио-бэкенды для `Sink` (ALSA, CPAL, JACK)

### 📚 Документация

Полная документация: [docs.rs/rill-graph](https://docs.rs/rill-graph)

Примеры: [github.com/DigitalRats/rill/tree/main/rill-graph/examples](https://github.com/DigitalRats/rill/tree/main/rill-graph/examples)

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

Примечание: rill-tests лицензирован под MIT. Полный текст лицензии: [LICENSE-MIT](LICENSE-MIT)

## 🌟 Благодарности

Всем, кто внёс вклад в развитие open-source аудио на Rust:
- [cpal](https://github.com/RustAudio/cpal) — кросс-платформенный аудио ввод-вывод
- [fundsp](https://github.com/SamiPerttu/fundsp) — вдохновение для DSP подходов
- [nih-plug](https://github.com/robbert-vdh/nih-plug) — архитектура плагинов

---

**Rill 0.3.0** — стабильное ядро, чистая архитектура, готовность к следующему этапу. 🚀
