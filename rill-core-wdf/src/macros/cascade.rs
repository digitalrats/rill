#[macro_export]
macro_rules! wdf_cascade {
    (
        name: $name:ident<T>,
        section: $section:ty,
        count: $count:expr,
        params: { $($pname:ident: $ptype:ty),* },
        state: { $($sname:ident: $stype:ty),* },
        feedback: |$f_self:ident, $f_in:ident, $f_out:ident| $feedback:tt,
        update: |$u_self:ident| $update:tt,
    ) => {
        #[derive(Debug, Clone)]
        pub struct $name<T: $crate::Transcendental> {
            pub poles: [$section; $count],
            $($pname: $ptype,)*
            $($sname: $stype,)*
        }

        impl<T: $crate::Transcendental> $name<T> {
            pub fn new(section: $section, $($pname: $ptype),*) -> Self {
                Self {
                    poles: [section; $count],
                    $($pname,)*
                    $($sname: T::ZERO,)*
                }
            }

            pub fn update_coeffs(&mut self) { let $u_self = self; $update }

            pub fn process_sample(&mut self, input: T) -> T {
                let a0 = {
                    let $f_self = &self;
                    let $f_in = input;
                    let $f_out = $f_self.feedback_prev;
                    $feedback
                };
                let mut a = a0;
                a = $crate::WdfElement::process_incident(&mut self.poles[0], a);
                a = $crate::WdfElement::process_incident(&mut self.poles[1], a);
                a = $crate::WdfElement::process_incident(&mut self.poles[2], a);
                a = $crate::WdfElement::process_incident(&mut self.poles[3], a);
                self.feedback_prev = a;
                a
            }

            pub fn cutoff(&self) -> T { self.cutoff }
            pub fn set_cutoff(&mut self, cutoff: T) {
                let half_sr = self.sample_rate / T::from_f32(2.0);
                self.cutoff = cutoff.clamp(T::from_f32(20.0), half_sr);
                self.update_coeffs();
            }
            pub fn resonance(&self) -> T { self.resonance }
            pub fn set_resonance(&mut self, resonance: T) {
                self.resonance = resonance.clamp(T::ZERO, T::ONE);
            }
            pub fn set_sample_rate(&mut self, sample_rate: T) {
                self.sample_rate = sample_rate;
                self.update_coeffs();
            }

            pub fn reset(&mut self) {
                for p in &mut self.poles { $crate::WdfElement::reset(p); }
                self.feedback_prev = T::ZERO;
            }
        }
    };
}
