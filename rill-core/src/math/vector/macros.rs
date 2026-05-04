//! Макросы для удобного построения векторных выражений.
//!
//! Этот модуль предоставляет макросы, которые упрощают работу с векторным eDSL,
//! позволяя писать выражения в естественной математической нотации.
//!
//! ## Примеры
//! ```
//! use rill_core::vector::prelude::*;
//! use rill_core::vector::macros::*;
//!
//! let a = ScalarVector4::splat(1.0);
//! let b = ScalarVector4::splat(2.0);
//! let c = a + b; // обычная векторная операция
//! assert_eq!(c, ScalarVector4::splat(3.0));
//!
//! // Применение выражения ко всему слайсу
//! let input = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
//! let mut output = [0.0f32; 8];
//! vec_map!(&input, &mut output, |x| x * 2.0 + 1.0);
//! // output = [3.0, 5.0, 7.0, 9.0, 11.0, 13.0, 15.0, 17.0]
//! ```
//!
//! ## Доступные макросы
//! - [`vec_map!`] – применяет векторное выражение ко всему слайсу.
//! - [`vec_expr!`] – создаёт ленивое векторное выражение (заглушка, требует исправления модуля expr).
//! - [`vec_eval!`] – немедленно вычисляет векторное выражение (заглушка).

use crate::math::vector::scalar::ScalarVector4;
use crate::math::vector::traits::Vector;

/// Map over SIMD vector chunks of size 4, applying a closure to each chunk.
#[macro_export]
macro_rules! vec_map {
    ($input:expr, $output:expr, |$x:ident| $($body:tt)*) => {{
        use $crate::math::vector::traits::Vector;
        use $crate::math::vector::scalar::ScalarVector4;
        const N: usize = 4;
        let input: &[_] = $input;
        let output: &mut [_] = $output;
        assert_eq!(input.len(), output.len(), "input and output slices must have equal length");

        if input.is_empty() {
            return;
        }

        let closure = |$x: ScalarVector4<_>| -> ScalarVector4<_> { $($body)* };

        let chunks = input.len() / N;
        let remainder = input.len() % N;

        #[allow(clippy::needless_range_loop)]
        for i in 0..chunks {
            let start = i * N;
            let x = <ScalarVector4<_>>::load(&input[start..start + N]);
            let y = closure(x);
            y.store(&mut output[start..start + N]);
        }

        if remainder > 0 {
            let start = chunks * N;
            let mut temp_input = [Default::default(); 4];
            #[allow(clippy::needless_range_loop)]
            for i in 0..remainder {
                temp_input[i] = input[start + i];
            }
            let x = <ScalarVector4<_>>::load(&temp_input[0..4]);
            let y = closure(x);
            #[allow(clippy::needless_range_loop)]
            for i in 0..remainder {
                output[start + i] = y.extract(i);
            }
        }
    }};
}

/// Создаёт ленивое векторное выражение (заглушка).
///
/// В текущей реализации модуль `expr` временно отключён из-за ошибок компиляции,
/// поэтому этот макрос возвращает переданное значение без изменений.
#[macro_export]
macro_rules! vec_expr {
    ($val:expr) => {
        $val
    };
}

/// Немедленно вычисляет векторное выражение (заглушка).
///
/// В текущей реализации просто возвращает переданное выражение.
#[macro_export]
macro_rules! vec_eval {
    ($($t:tt)*) => {
        $($t)*
    };
}

pub use crate::vec_eval;
pub use crate::vec_expr;
pub use crate::vec_map;

// -----------------------------------------------------------------------------
// Тесты
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::vector::scalar::ScalarVector4;

    #[test]
    fn test_vec_map_f32() {
        let input = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let mut output = [0.0f32; 8];

        // Замыкание: x * 2.0 + 1.0
        vec_map!(&input, &mut output, |x| x * 2.0 + 1.0);

        assert_eq!(output[0], 3.0); // 1*2 + 1
        assert_eq!(output[1], 5.0); // 2*2 + 1
        assert_eq!(output[2], 7.0);
        assert_eq!(output[3], 9.0);
        assert_eq!(output[4], 11.0);
        assert_eq!(output[5], 13.0);
        assert_eq!(output[6], 15.0);
        assert_eq!(output[7], 17.0);
    }

    #[test]
    fn test_vec_map_f64() {
        let input = [1.0f64, 2.0, 3.0, 4.0];
        let mut output = [0.0f64; 4];

        vec_map!(&input, &mut output, |x| x * 3.0 - 1.0);

        assert_eq!(output[0], 2.0); // 1*3 - 1
        assert_eq!(output[1], 5.0); // 2*3 - 1
        assert_eq!(output[2], 8.0);
        assert_eq!(output[3], 11.0);
    }

    #[test]
    fn test_vec_map_empty() {
        let input: [f32; 0] = [];
        let mut output: [f32; 0] = [];
        vec_map!(&input, &mut output, |x| x * 2.0); // не должно паниковать
    }

    #[test]
    fn test_vec_map_remainder() {
        let input = [1.0f32, 2.0, 3.0]; // три элемента
        let mut output = [0.0f32; 3];

        vec_map!(&input, &mut output, |x| x + 10.0);

        assert_eq!(output[0], 11.0);
        assert_eq!(output[1], 12.0);
        assert_eq!(output[2], 13.0);
    }

    #[test]
    fn test_vec_expr_stub() {
        let vec = ScalarVector4::splat(5.0);
        let result = vec_expr!(vec);
        assert_eq!(result, vec);
    }

    #[test]
    fn test_vec_eval_stub() {
        let a = ScalarVector4::splat(2.0);
        let b = ScalarVector4::splat(3.0);
        let result = vec_eval!(a + b);
        assert_eq!(result, ScalarVector4::splat(5.0));
    }
}
