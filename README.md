Вот общий `README.md` для корня проекта:

```markdown
# Kama Audio 🎵

[![build](https://img.shields.io/badge/build-passing-brightgreen)](https://github.com/DigitalRats/kama-audio)
[![tests](https://img.shields.io/badge/tests-150%2B-passing)](https://github.com/DigitalRats/kama-audio)
[![license](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)

**Модульная экосистема для создания аудиоприложений на Rust.**

Kama Audio — это не монолит, а набор специализированных крейтов, каждый из которых решает свою задачу. Вы можете использовать только то, что нужно для вашего проекта.

```
kama-core-traits     # ядро с трейтами (минимум)
kama-buffers         # работа с буферами
kama-graph           # аудиограф
kama-signal          # сигнальная система
kama-automation      # автоматизация параметров
kama-control         # MIDI/HID управление
kama-dsp-common      # DSP инфраструктура
kama-oscillators     # осцилляторы (аудио и LFO)
kama-digital-filters # цифровые фильтры
kama-digital-effects # цифровые эффекты## 📄 Лицензия

Проект распространяется под лицензиями **MIT** или **Apache-2.0** (на ваш выбор).
kama-eq              # эквалайзеры
kama-lofi            # Lo-Fi эмуляция
kama-mixer           # микшер
kama-hp              # high-precision вычисления
kama-io              # аудио ввод-вывод (в разработке)
```

## 🎯 Зачем это нужно?

- **Для музыкантов**: создавайте свои эффекты и инструменты## 📄 Лицензия

Проект распространяется под лицензиями **MIT** или **Apache-2.0** (на ваш выбор).
- **Для разработчиков**: стройте аудиоприложения на надёжном фундаменте
- **Для live coding**: Drift — сервер эффектов для TidalCycles, SuperCollider и других сред

## ✨ Особенности

- **Минимальное ядро** — только трейты, всё остальное в крейтах
- **Модульность** — берите только то, что нужно
- **Производительность** — zero-cost abstractions, real-time безопасность
- **Тестируемость** — 150+ тестов, всё проверено
- **Расширяемость** — легко добавить свой эффект или бэкенд

## 🚀 Быстрый старт

Добавьте нужные крейты в `Cargo.toml`:

```toml
[dependencies]
kama-core-traits = "0.1"
kama-graph = "0.1"
kama-oscillators = "0.1"
kama-digital-effects = "0.1"
```

Создайте простой эффект (синус + задержка):

```rust## 📄 Лицензия

Проект распространяется под лицензиями **MIT** или **Apache-2.0** (на ваш выбор).
use kama_core::traits::{AudioNode, PortId};
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
    let sum: f32 = signal.iter().map(|x| x * x).sum();## 📄 Лицензия

Проект распространяется под лицензиями **MIT** или **Apache-2.0** (на ваш выбор).
    (sum / signal.len() as f32).sqrt()
}
```

## 📦 Состояние крейтов

| Крейт | Статус | Описание |
|-------|--------|----------|
| `kama-core-traits` | ✅ стабильный | Базовые трейты (`AudioNode`, `ParamValue`, `TimeProvider`) |
| `kama-buffers` | ✅ стабильный | Кольцевые буферы, многоголовые буферы, пулы |
| `kama-graph` | ✅ стабильный | Аудиограф с топологической сортировкой |
| `kama-signal` | ✅ стабильный | Сигнальная система (`SignalBus`, `ParameterChanged`) |
| `kama-automation` | ✅ стабильный | Автоматизация (LFO, огибающие, сервоприводы) |
| `kama-control` | ✅ стабильный | MIDI управление, маппинг событий |
| `kama-dsp-common` | ✅ стабильный | DSP инфраструктура, функциональные узлы |
| `kama-oscillators` | ✅ стабильный | Осцилляторы (синус, пила, шум, LFO, огибающие) |
| `kama-digital-filters` | ✅ стабильный | Биквадратные фильтры (LP, HP, BP, Notch, Peak) |
| `kama-digital-effects` | ✅ стабильный | Эффекты (Delay, Distortion, Limiter) |
| `kama-eq` | ✅ стабильный | Параметрический и графический эквалайзеры |
| `kama-lofi` | ✅ стабильный | Lo-Fi эмуляция (NES, AY-3-8910, Akai S900) |
| `kama-mixer` | ✅ стабильный | Микшер с каналами, панорамой и aux шинами |
| `kama-hp` | ✅ стабильный | High-precision вычисления (f64) |
| `kama-io` | 🔄 в работе | Аудио ввод-вывод (ALSA, CPAL) |
| `kama-wdf` | ⏳ планируется | Wave Digital Filters (аналоговая эмуляция) |
| `kama-server` | ⏳ планируется | OSC сервер для удалённого управления |
| `drift` | ⏳ планируется | Продукт: сервер эффектов для live coding |

## 🧪 Тестирование

```bash
# Все тесты
cargo test --workspace

# Интеграционные тесты цифровой части
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

## 📄 Лицензия

Проект распространяется под лицензией **Apache 2.0**. Это означает, что вы можете:

- ✅ Использовать в коммерческих продуктах
- ✅ Модифицировать и распространять
- ✅ Использовать патентные права
- ❗ При изменениях указывать авторов
- ❗ Сохранять уведомление об авторстве

Полный текст лицензии: [LICENSE-APACHE](LICENSE-APACHE)

Примечание: kama-tests лиценизирован под MIT. Полный текст лицензии: [LICENSE-MIT](LICENSE-MIT)

### Зависимости

Все зависимости проекта совместимы с Apache-2.0 (MIT, Apache-2.0, MIT/Apache-2.0). 
Ни одной GPL/AGPL зависимости не используется.

## 🌟 Благодарности

Всем, кто внёс вклад в развитие open-source аудио на Rust:
- [cpal](https://github.com/RustAudio/cpal) — кросс-платформенный аудио ввод-вывод
- [fundsp](https://github.com/SamiPerttu/fundsp) — вдохновение для DSP подходов
- [nih-plug](https://github.com/robbert-vdh/nih-plug) — архитектура плагинов

---

**Kama Audio** — делаем звук на Rust доступным и модульным. 🚀
```
