# Domain-Specific Languages in Rill

Rill provides two built-in domain-specific languages (eDSL) based on `macro_rules!`:

- **Mathematical eDSL** — vector operations, type-independent arithmetic (`rill-core::math`)
- **WDF eDSL** — analog circuit description through element composition (`rill-core-model::macros`)

Both are implemented via `macro_rules!`, require no external code generators, and expand to flat code at compile time.

---

## 1. Mathematical eDSL

### Numeric trait hierarchy

```
Scalar                          — arithmetic: +, -, *, /, min, max, clamp, abs
  ├── f32, f64
  ├── i8, i16, i32, i64
  │
  └── Transcendental            — trigonometry: sin, cos, sqrt, exp, ln, PI
        └── f32, f64            + from_f32, to_f32
```

`Scalar` — base trait for any numeric types. Allows `Vector<T, N>` to work with `i32`, `i16` and other integer types, not just `f32`/`f64`.

`Transcendental` — extension for floating-point types, adding sin/cos/sqrt/exp/ln.

### Vector types

`Vector<T: Scalar, N>` — trait for N-dimensional vectors:

| Type | Elements | Purpose |
|-----|-----------|------------|
| `ScalarVector1<T>` | 1 | Scalar stub |
| `ScalarVector2<T>` | 2 | Stereo |
| `ScalarVector4<T>` | 4 | SIMD-capable (SSE, NEON) |
| `ScalarVector8<T>` | 8 | AVX-capable (stub) |
| `F32x4`, `F64x4` etc. | 4+ | Hardware SIMD via `wide` crate |

Basic operations (available for any `T: Scalar`):

```rust
use rill_core::math::Scalar;
use rill_core::math::vector::ScalarVector4;

let a = ScalarVector4::new(1i32, 2, 3, 4);
let b = ScalarVector4::new(5i32, 6, 7, 8);
let c = a + b;   // element-wise addition
let d = a * b;   // element-wise multiplication
```

Slice operations:

```rust
use rill_core::math::vector::ops::add_slices;
use rill_core::math::vector::math::sin_slice;

let input = [0.0f32, 0.5, 1.0, 1.5, 2.0];
let mut output = [0.0f32; 5];

// Works with any Scalar
add_slices::<f32, 4, ScalarVector4<f32>>(&input, &input, &mut output);

// Transcendental operations require Transcendental
sin_slice::<f32, 4, ScalarVector4<f32>>(&input, &mut output);
```

### `vec_map!` macro

```rust
use rill_core::prelude::*;

let input = [1.0f32, 2.0, 3.0, 4.0, 5.0];
let mut output = [0.0f32; 5];

vec_map!(&input, &mut output, |x| x * 2.0 + 1.0);
// output = [3.0, 5.0, 7.0, 9.0, 11.0]
```

The macro applies the expression to each chunk of 4 elements via `ScalarVector4`, then processes the remainder scalar-wise. LLVM folds operations into SIMD instructions.

### `VectorTranscendental`

For sin/cos/sqrt operations on vectors:

```rust
use rill_core::math::vector::{
    ScalarVector4, Vector, VectorTranscendental,
};

fn process<T: Transcendental>(v: ScalarVector4<T>) -> ScalarVector4<T> {
    v.sin()  // only when T: Transcendental
}
```

---

## 2. WDF eDSL

Wave Digital Filter (WDF) — a method for modeling analog circuits where each element (resistor, capacitor, diode) is represented as a one-port black box. Elements are connected via series and parallel adapters.

Base trait:

```rust
pub trait WdfElement<T: Transcendental>: Send + Sync {
    fn port_resistance(&self) -> T;
    fn process_incident(&mut self, a: T) -> T;   // a → b
    fn update_state(&mut self);                    // update after calculation
    fn voltage(&self) -> T;
    fn current(&self) -> T;
    fn reset(&mut self);
}
```

### 2.1 `wdf_element!` — defining an element

Creates a struct and full `WdfElement` implementation from a black-box description:

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

**Syntax:**
- `params` — element constants (set at creation)
- `state` — state variables (initialized to `T::ZERO`)
- `port_resistance: |s| expr` — port resistance
- `scattering: |s, a| expr` — scattering equation: compute reflected wave `b` from incident wave `a`. `s` — mutable reference to self.
- `update: |s| block` — state update (called after wave calculation)
- `reset: |s| block` — reset to initial state
- `s.voltage` and `s.current` — writable (store latest values)

Generates:
- `struct $name<T>` with fields params, state, `voltage`, `current`
- `impl $name<T> { fn new(params...) -> Self }`
- `impl WdfElement<T> for $name<T>`

### 2.2 `wdf_compose!` — composing elements

**Series** — series connection:

```rust
wdf_compose! {
    name: RcSection<T>,
    kind: Series,
    elements: (Resistor<T>, Capacitor<T>),
}
```

Generates a struct with `left` and `right` fields, delegating `WdfElement`.
Port resistance — sum: `R_total = R_left + R_right`.
Waves distribute proportionally to resistances.

**Parallel** — parallel connection:

```rust
wdf_compose! {
    name: TankCircuit<T>,
    kind: Parallel,
    elements: (Capacitor<T>, Inductor<T>),
}
```

Port resistance — parallel combination: `R_total = (R1·R2) / (R1 + R2)`.

### 2.3 `wdf_cascade!` — cascade of N sections + feedback

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

Generates:
- `struct $name<T>` with field `poles: [$section; N]` + params + state
- `fn process_sample(&mut self, input: T) -> T` — unrolled cascade
- `fn set_cutoff()`, `fn cutoff()`, `fn set_resonance()`, `fn resonance()`, `fn set_sample_rate()`
- `fn update_coeffs()`, `fn reset()`

Closure parameters:
- `feedback: |s, input, fb_prev| { ... }` — `s` is `&self`, `input` is the input sample, `fb_prev` is the previous output value
- `update: |s| { ... }` — updates section coefficients (called when cutoff/resonance changes)

### 2.4 Macro hygiene

All expressions inside macros receive `self` through a named closure parameter:

```rust
// Correct:
port_resistance: |s| { s.rp },
scattering: |s, a| { s.state + s.alpha * (a - s.state) },
update: |s| { },
reset: |s| { s.state = T::ZERO; },
```

`self` inside captured `:tt` blocks does NOT work due to `macro_rules!` hygiene.
Using `s` as the parameter name is a convention.

### 2.5 Limitations

| Construct | Supported | Description |
|-------------|---------------|-----------|
| Two-terminal (R, C, L, D) | ✅ `wdf_element!` | One port, scattering 2×2 |
| Series<A, B> | ✅ `wdf_compose!` | Static circuits |
| Parallel<A, B> | ✅ `wdf_compose!` | Static circuits |
| Cascade N+feedback | ✅ `wdf_cascade!` | MoogLadder |
| Three-terminal (transistor) | ❌ manual impl | Scattering matrix 3×3 |
| Op-amp, OTA | ❌ manual impl | Mathematical model |

---

## 3. Examples

### MoogLadder (4-pole low-pass with resonance)

```rust
use rill_core_model::filters::{RcPole, MoogLadder};

// RcPole — one-pole low-pass filter (wdf_element!)
// MoogLadder — cascade of 4 RcPole + resonance feedback (wdf_cascade!)

let pole = RcPole::new(0.0);        // alpha = 0 (fully open)
let mut filter = MoogLadder::new(
    pole, 1000.0, 0.0, 44100.0      // cutoff=1kHz, resonance=0
);
filter.update_coeffs();              // calculate alpha from cutoff

// Process sample
let input = 0.5;
let output = filter.process_sample(input);
```

### DiodeClipper (overdrive)

```rust
use rill_core_model::constants::{BOLTZMANN, ELECTRON_CHARGE};
use rill_core_model::elements::Resistor;
use rill_core_model::filters::{AntiParallelDiode, DiodeClipper};
use rill_core_model::WdfElement;

let r = Resistor::new(1000.0);
let vt = BOLTZMANN * 300.0 / ELECTRON_CHARGE;

let mut diode = AntiParallelDiode::new(1e-15, vt);
diode.reset();

let mut clipper = DiodeClipper::new(r, diode);

// Process
let b = WdfElement::process_incident(&mut clipper, 10.0);
clipper.update_state();
let clipped_voltage: f64 = clipper.right.voltage();  // ≈ 0.6V
```

### Vector MAP (SIMD)

```rust
use rill_core::prelude::*;

let input = [1.0f32; 1024];
let mut output = [0.0f32; 1024];

vec_map!(&input, &mut output, |x| (x * 2.0 + 1.0).sin());
```

---

## 4. eDSL compilation flow

```
Source code (macros)
    │
    ▼
macro_rules! expansion (compile-time)
    │
    ▼
Flat Rust code with no indirection
    │
    ▼
LLVM optimization (inlining, constant folding, SIMD)
    │
    ▼
Machine code
```

All eDSLs expand at compile time into flat structures and methods. No trait objects, dynamic dispatch, or allocations in the hot path. LLVM additionally folds constants and vectorizes loops.
