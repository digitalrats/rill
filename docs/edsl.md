# Domain-Specific Languages в Rill

Rill предоставляет два встроенных предметно-ориентированных языка (eDSL) на базе `macro_rules!`:

- **Математический eDSL** — векторные операции, типонезависимая арифметика (`rill-core::math`)
- **WDF eDSL** — описание аналоговых схем через композицию элементов (`rill-core-wdf::macros`)

Оба реализованы через `macro_rules!`, не требуют внешних кодогенераторов и раскрываются в плоский код на этапе компиляции.

---

## 1. Математический eDSL

### Иерархия числовых трейтов

```
Scalar                          — арифметика: +, -, *, /, min, max, clamp, abs
  ├── f32, f64
  ├── i8, i16, i32, i64
  │
  └── Transcendental            — тригонометрия: sin, cos, sqrt, exp, ln, PI
        └── f32, f64            + from_f32, to_f32
```

`Scalar` — базовый трейт для любых числовых типов. Позволяет `Vector<T, N>` работать с `i32`, `i16` и другими целыми типами, не только с `f32`/`f64`.

`Transcendental` — расширение для типов с плавающей точкой, добавляющее sin/cos/sqrt/exp/ln.

### Векторные типы

`Vector<T: Scalar, N>` — трейт для N-мерных векторов:

| Тип | Элементов | Назначение |
|-----|-----------|------------|
| `ScalarVector1<T>` | 1 | Скалярная заглушка |
| `ScalarVector2<T>` | 2 | Стерео |
| `ScalarVector4<T>` | 4 | SIMD-совместимый (SSE, NEON) |
| `ScalarVector8<T>` | 8 | AVX-совместимый (заглушка) |
| `F32x4`, `F64x4` и др. | 4+ | Аппаратный SIMD через крейт `wide` |

Базовые операции (доступны для любого `T: Scalar`):

```rust
use rill_core::math::Scalar;
use rill_core::math::vector::ScalarVector4;

let a = ScalarVector4::new(1i32, 2, 3, 4);
let b = ScalarVector4::new(5i32, 6, 7, 8);
let c = a + b;   // поэлементное сложение
let d = a * b;   // поэлементное умножение
```

Slice-операции:

```rust
use rill_core::math::vector::ops::add_slices;
use rill_core::math::vector::math::sin_slice;

let input = [0.0f32, 0.5, 1.0, 1.5, 2.0];
let mut output = [0.0f32; 5];

// Работает с любым Scalar
add_slices::<f32, 4, ScalarVector4<f32>>(&input, &input, &mut output);

// Трансцендентные операции требуют Transcendental
sin_slice::<f32, 4, ScalarVector4<f32>>(&input, &mut output);
```

### Макрос `vec_map!`

```rust
use rill_core::prelude::*;

let input = [1.0f32, 2.0, 3.0, 4.0, 5.0];
let mut output = [0.0f32; 5];

vec_map!(&input, &mut output, |x| x * 2.0 + 1.0);
// output = [3.0, 5.0, 7.0, 9.0, 11.0]
```

Макрос применяет выражение к каждому чанку из 4 элементов через `ScalarVector4`, затем обрабатывает остаток скалярно. LLVM сворачивает операции в SIMD-инструкции.

### `VectorTranscendental`

Для операций sin/cos/sqrt над векторами:

```rust
use rill_core::math::vector::{
    ScalarVector4, Vector, VectorTranscendental,
};

fn process<T: Transcendental>(v: ScalarVector4<T>) -> ScalarVector4<T> {
    v.sin()  // только при T: Transcendental
}
```

---

## 2. WDF eDSL

Wave Digital Filter (WDF) — метод моделирования аналоговых цепей, при котором каждый элемент (резистор, конденсатор, диод) представляется как чёрный ящик с одним портом. Элементы соединяются через последовательные и параллельные адаптеры.

Базовый трейт:

```rust
pub trait WdfElement<T: Transcendental>: Send + Sync {
    fn port_resistance(&self) -> T;
    fn process_incident(&mut self, a: T) -> T;   // a → b
    fn update_state(&mut self);                    // обновление после расчёта
    fn voltage(&self) -> T;
    fn current(&self) -> T;
    fn reset(&mut self);
}
```

### 2.1 `wdf_element!` — определение элемента

Создаёт структуру и полную имплементацию `WdfElement` из описания черного ящика:

```rust
wdf_element! {
    name: RcPole<T>,
    params: { alpha: T },
    state: { state: T },
    port_resistance: |s| { T::ONE },
    scattering: |s, a| {
        let b = s.state + s.alpha * (a - s.state);
        s.state = b + s.alpha * (a - b);
        b
    },
    update: |_s| {},
    reset: |s| { s.state = T::ZERO; },
}
```

**Синтаксис:**
- `params` — константы элемента (задаются при создании)
- `state` — переменные состояния (инициализируются в `T::ZERO`)
- `port_resistance: |s| expr` — портовое сопротивление
- `scattering: |s, a| expr` — уравнение рассеяния: по падающей волне `a` вычислить отражённую `b`. `s` — mutable ссылка на self.
- `update: |s| block` — обновление состояния (вызывается после волнового расчёта)
- `reset: |s| block` — сброс в начальное состояние
- `s.voltage` и `s.current` — доступны для записи (хранят последние значения)

Генерирует:
- `struct $name<T>` с полями params, state, `voltage`, `current`
- `impl $name<T> { fn new(params...) -> Self }`
- `impl WdfElement<T> for $name<T>`

### 2.2 `wdf_compose!` — композиция элементов

**Series** — последовательное соединение:

```rust
wdf_compose! {
    name: RcSection<T>,
    kind: Series,
    elements: (Resistor<T>, Capacitor<T>),
}
```

Генерирует структуру с полями `left` и `right`, делегирующую `WdfElement`.
Портовое сопротивление — сумма: `R_total = R_left + R_right`.
Волны распределяются пропорционально сопротивлениям.

**Parallel** — параллельное соединение:

```rust
wdf_compose! {
    name: TankCircuit<T>,
    kind: Parallel,
    elements: (Capacitor<T>, Inductor<T>),
}
```

Портовое сопротивление — параллельная комбинация: `R_total = (R1·R2) / (R1 + R2)`.

### 2.3 `wdf_cascade!` — каскад N секций + feedback

```rust
wdf_cascade! {
    name: MoogLadder<T>,
    section: RcPole<T>,
    count: 4,
    params: { cutoff: T, resonance: T, sample_rate: T },
    state: { feedback_prev: T },
    feedback: |s, input, fb_prev| {
        let k = s.resonance * T::from_f32(4.0);
        let fb = fb_prev * k;
        input - fb.clamp(-T::ONE, T::ONE)
    },
    update: |s| {
        let g = T::PI * s.cutoff / s.sample_rate;
        let alpha = g / (T::ONE + g);
        for p in &mut s.poles { p.alpha = alpha; }
    },
}
```

Генерирует:
- `struct $name<T>` с полем `poles: [$section; N]` + params + state
- `fn process_sample(&mut self, input: T) -> T` — развёрнутый каскад
- `fn set_cutoff()`, `fn cutoff()`, `fn set_resonance()`, `fn resonance()`, `fn set_sample_rate()`
- `fn update_coeffs()`, `fn reset()`

Параметры замыканий:
- `feedback: |s, input, fb_prev| { ... }` — `s` это `&self`, `input` — входной семпл, `fb_prev` — предыдущее выходное значение
- `update: |s| { ... }` — обновление коэффициентов секций (вызывается при изменении cutoff/resonance)

### 2.4 Гигиена макросов

Все выражения внутри макросов получают `self` через именованный параметр замыкания:

```rust
// Правильно:
port_resistance: |s| { s.rp },
scattering: |s, a| { s.state + s.alpha * (a - s.state) },
update: |s| { },
reset: |s| { s.state = T::ZERO; },
```

`self` внутри захваченных `:tt` блоков НЕ работает из-за гигиены `macro_rules!`.
Использование `s` в качестве имени параметра — конвенция.

### 2.5 Ограничения

| Конструкция | Поддерживается | Пояснение |
|-------------|---------------|-----------|
| Двухполюсники (R, C, L, D) | ✅ `wdf_element!` | Один порт, scattering 2×2 |
| Series<A, B> | ✅ `wdf_compose!` | Статические цепи |
| Parallel<A, B> | ✅ `wdf_compose!` | Статические цепи |
| Каскад N+feedback | ✅ `wdf_cascade!` | MoogLadder |
| Трёхполюсники (транзистор) | ❌ ручной impl | Scattering matrix 3×3 |
| ОУ, OTA | ❌ ручной impl | Математическая модель |

---

## 3. Примеры

### MoogLadder (4-полюсный ФНЧ с резонансом)

```rust
use rill_core_wdf::filters::{RcPole, MoogLadder};

// RcPole — однополюсный ФНЧ (wdf_element!)
// MoogLadder — каскад 4 RcPole + resonance feedback (wdf_cascade!)

let pole = RcPole::new(0.0);        // alpha = 0 (fully open)
let mut filter = MoogLadder::new(
    pole, 1000.0, 0.0, 44100.0      // cutoff=1kHz, resonance=0
);
filter.update_coeffs();              // расчёт alpha из cutoff

// Обработка сэмпла
let input = 0.5;
let output = filter.process_sample(input);
```

### DiodeClipper (овердрайв)

```rust
use rill_core_wdf::constants::{BOLTZMANN, ELECTRON_CHARGE};
use rill_core_wdf::elements::Resistor;
use rill_core_wdf::filters::{AntiParallelDiode, DiodeClipper};
use rill_core_wdf::WdfElement;

let r = Resistor::new(1000.0);
let vt = BOLTZMANN * 300.0 / ELECTRON_CHARGE;

let mut diode = AntiParallelDiode::new(1e-15, vt);
diode.reset();

let mut clipper = DiodeClipper::new(r, diode);

// Обработка
let b = WdfElement::process_incident(&mut clipper, 10.0);
clipper.update_state();
let clipped_voltage: f64 = clipper.right.voltage();  // ≈ 0.6V
```

### Векторный MAP (SIMD)

```rust
use rill_core::prelude::*;

let input = [1.0f32; 1024];
let mut output = [0.0f32; 1024];

vec_map!(&input, &mut output, |x| (x * 2.0 + 1.0).sin());
```

---

## 4. Поток компиляции eDSL

```
Исходный код (макросы)
    │
    ▼
macro_rules! раскрытие (compile-time)
    │
    ▼
Плоский Rust-код без indirection
    │
    ▼
LLVM оптимизация (inlining, constant folding, SIMD)
    │
    ▼
Машинный код
```

Все eDSL раскрываются на этапе компиляции в плоские структуры и методы. Никаких трейт-объектов, динамической диспетчеризации или аллокаций в hot path. LLVM дополнительно сворачивает константы и векторизует циклы.
