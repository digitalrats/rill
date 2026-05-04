use crate::math::Transcendental;

/// Fractional-index reading with interpolation.
pub trait Interpolate {
    /// The type produced by interpolation.
    type Output;

    /// Linear interpolation at fractional index.
    /// `index` in [0, len-1]; clamps to valid range.
    fn interpolate_linear(&self, index: f64) -> Self::Output;

    /// Cubic Hermite interpolation at fractional index.
    /// Requires index in [1, len-2] for 4-point stencil; clamps.
    fn interpolate_cubic(&self, index: f64) -> Self::Output;

    /// Nearest-neighbor (round to nearest integer index).
    fn interpolate_nearest(&self, index: f64) -> Self::Output;
}

impl<T: Transcendental + Copy> Interpolate for [T] {
    type Output = T;

    fn interpolate_linear(&self, index: f64) -> T {
        let len = self.len();
        if len == 0 {
            return T::ZERO;
        }
        let idx = index.clamp(0.0, (len - 1) as f64);
        let i0 = idx.floor() as usize;
        let i1 = (i0 + 1).min(len - 1);
        let frac = T::from_f64(idx.fract());
        let a = self[i0];
        let b = self[i1];
        a + (b - a) * frac
    }

    fn interpolate_cubic(&self, index: f64) -> T {
        let len = self.len();
        if len < 4 {
            return self.interpolate_linear(index);
        }
        let idx = index.clamp(1.0, (len - 3) as f64);
        let i = idx.floor() as usize;
        let i0 = i - 1;
        let i1 = i;
        let i2 = i + 1;
        let i3 = i + 2;
        let frac = T::from_f64(idx.fract());

        let c0 = self[i1];
        let c1 = (self[i2] - self[i0]) * T::from_f32(0.5);
        let c2 = self[i0] * T::from_f32(-1.5)
            + self[i1] * T::from_f32(2.0)
            + self[i2] * T::from_f32(-0.5);
        let c3 = self[i0] * T::from_f32(-0.5)
            + self[i1] * T::from_f32(1.5)
            + self[i2] * T::from_f32(-1.5)
            + self[i3] * T::from_f32(0.5);

        let f2 = frac * frac;
        let f3 = f2 * frac;
        c0 + c1 * frac + c2 * f2 + c3 * f3
    }

    fn interpolate_nearest(&self, index: f64) -> T {
        let len = self.len();
        if len == 0 {
            return T::ZERO;
        }
        let idx = (index.round() as usize).min(len - 1);
        self[idx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_simple() {
        let buf: [f64; 5] = [0.0, 1.0, 2.0, 3.0, 4.0];
        assert_eq!(buf.interpolate_linear(0.0), 0.0);
        assert_eq!(buf.interpolate_linear(4.0), 4.0);
        assert!((buf.interpolate_linear(0.5) - 0.5).abs() < 1e-10);
        assert!((buf.interpolate_linear(2.5) - 2.5).abs() < 1e-10);
    }

    #[test]
    fn test_linear_clamp() {
        let buf: [f64; 3] = [10.0, 20.0, 30.0];
        assert_eq!(buf.interpolate_linear(-1.0), 10.0);
        assert_eq!(buf.interpolate_linear(100.0), 30.0);
    }

    #[test]
    fn test_linear_empty() {
        let buf: [f64; 0] = [];
        assert_eq!(buf.interpolate_linear(0.0), 0.0);
    }

    #[test]
    fn test_cubic_exact_at_knots() {
        let buf: [f64; 6] = [0.0, 0.5, 1.0, 0.8, 0.3, 0.0];
        for i in 1..=3 {
            let v = buf.interpolate_cubic(i as f64);
            assert!(
                (v - buf[i]).abs() < 1e-10,
                "cubic should pass through knot {}: got {}, expected {}",
                i,
                v,
                buf[i]
            );
        }
    }

    #[test]
    fn test_cubic_interior() {
        let buf: [f64; 4] = [0.0, 0.3, 0.7, 1.0];
        for i in 0..=10 {
            let t = 1.0 + i as f64 / 10.0;
            let v = buf.interpolate_cubic(t);
            assert!(
                v >= -0.1 && v <= 1.1,
                "cubic range violated at t={}: got {}",
                t,
                v
            );
        }
    }

    #[test]
    fn test_cubic_short_fallback() {
        let buf: [f64; 2] = [0.0, 1.0];
        assert_eq!(buf.interpolate_cubic(0.5), buf.interpolate_linear(0.5));
    }

    #[test]
    fn test_nearest() {
        let buf: [f64; 3] = [10.0, 20.0, 30.0];
        assert_eq!(buf.interpolate_nearest(0.0), 10.0);
        assert_eq!(buf.interpolate_nearest(0.4), 10.0);
        assert_eq!(buf.interpolate_nearest(0.6), 20.0);
        assert_eq!(buf.interpolate_nearest(2.0), 30.0);
    }

    #[test]
    fn test_on_vec() {
        let buf: Vec<f64> = vec![0.0, 1.0, 2.0];
        assert_eq!(buf.interpolate_linear(1.5), 1.5);
    }

    #[test]
    fn test_on_boxed_slice() {
        let buf: Box<[f64]> = vec![0.0, 1.0, 2.0].into_boxed_slice();
        assert_eq!(buf.interpolate_linear(1.5), 1.5);
    }
}
