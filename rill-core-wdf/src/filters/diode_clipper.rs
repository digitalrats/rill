/// Diode clipper with anti-parallel diodes as a single nonlinear element.
use crate::constants::{BOLTZMANN, ELECTRON_CHARGE, NEWTON_TOLERANCE};
use crate::elements::Resistor;
use crate::WdfElement;
use rill_core::Transcendental;

crate::wdf_element! {
    name: AntiParallelDiode<T>,
    params: { is: T, vt: T },
    state: { rp: T },
    port_resistance: |s| { s.rp },
    scattering: |s, a| {
        let tolerance = T::from_f64(NEWTON_TOLERANCE);
        let guess = s.vt * (T::ONE + a.abs() / (T::from_f32(2.0) * s.rp * s.is)).ln();
        let mut v = guess.max(T::ZERO);
        for _ in 0..12 {
            let ev = (v / s.vt).exp();
            let env = (-v / s.vt).exp();
            let i = s.is * (ev - env);
            let g = s.is * (ev + env) / s.vt;
            let f = v + s.rp * i - a;
            if f.abs() < tolerance { break; }
            let df = T::ONE + s.rp * g;
            v -= f / df;
        }
        let ev = (v / s.vt).exp();
        let env = (-v / s.vt).exp();
        let g = s.is * (ev + env) / s.vt;
        s.rp = T::ONE / g.max(T::from_f64(1e-12));
        s.voltage = v;
        T::from_f32(2.0) * v - a
    },
    update: |_s| {},
    reset: |s| { s.rp = T::from_f64(1e-3); },
}

crate::wdf_compose! {
    name: DiodeClipper<T>,
    kind: Series,
    elements: (Resistor<T>, AntiParallelDiode<T>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WdfElement;

    fn make_clipper() -> DiodeClipper<f64> {
        let r = Resistor::new(1000.0);
        let vt = BOLTZMANN * 300.0 / ELECTRON_CHARGE;
        let mut diode = AntiParallelDiode::new(1e-15, vt);
        diode.reset();
        DiodeClipper::new(r, diode)
    }

    #[test]
    fn test_clipper_positive_clip() {
        let mut c = make_clipper();
        WdfElement::process_incident(&mut c, 10.0);
        c.update_state();
        let v: f64 = c.right.voltage();
        assert!(v > 0.0 && v < 1.0, "should clip positive to ~0.6V: got {}", v);
    }

    #[test]
    fn test_clipper_negative_clip() {
        let mut c = make_clipper();
        WdfElement::process_incident(&mut c, -10.0);
        c.update_state();
        let v: f64 = c.right.voltage();
        assert!(v < 0.0 && v > -1.0, "should clip negative to ~-0.6V: got {}", v);
    }

    #[test]
    fn test_clipper_symmetry() {
        let mut c1 = make_clipper();
        WdfElement::process_incident(&mut c1, 5.0);
        c1.update_state();
        let vp: f64 = c1.right.voltage();
        let mut c2 = make_clipper();
        WdfElement::process_incident(&mut c2, -5.0);
        c2.update_state();
        let vn: f64 = c2.right.voltage();
        assert!((vp + vn).abs() < 0.1, "pos={} neg={}", vp, vn);
    }
}
