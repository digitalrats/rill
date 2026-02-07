# Kama Audio Project

Модульная система аудиообработки на Rust.

## Структура проекта

- **kama-core/** - Базовая библиотека аудиообработки
  - Модульная архитектура
  - Графы обработки сигналов
  - Сигнальная система (pub/sub)
  - Автоматизация параметров
  - Абстракции ввода/вывода

## Быстрый старт

```bash
# Клонировать проект
git clone <repository>
cd kama-audio

# Сборка всех компонентов
cargo build --release

# Запуск примеров
cargo run --example simple_delay
cargo run --example signal_example

# Запуск тестов
cargo test
```

## Буферы с высокой точностью

### Когда использовать f64:

* Синтезаторы с сложной модуляцией (FM, PM, waveshaping)

* Фильтры высокого порядка (> 8 порядка)

* Аналитические инструменты (спектральный анализ, pitch detection)

* Профессиональные мастер-процессоры (лимитеры, компрессоры)

* Гранулярный и физический моделирующий синтез

### Когда достаточно f32:
* Простые эффекты (delay, chorus, flanger)

* Базовые фильтры (1-4 порядка)

* Микшеры и панорамирование

* Гейны и аттенюаторы

### Гибридный подход:

```rust
// Пример гибридной обработки
let input_f32: &[f32] = ...;
let mut output_f32: &mut [f32] = ...;

// 1. Конвертируем в f64 для точной обработки
let input_f64: Vec<f64> = input_f32.iter().map(|&x| x as f64).collect();

// 2. Выполняем high-precision обработку
let mut processor = HighPrecisionFMProcessor::new();
let mut output_f64 = vec![0.0; input_f64.len()];
processor.process(&input_f64, &mut output_f64);

// 3. Конвертируем обратно с dithering'ом
convert_f64_to_f32_with_dither(&output_f64, output_f32, true);
```