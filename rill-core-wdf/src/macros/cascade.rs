/// Generate a cascaded WDF filter from identical sections.
///
/// Creates a struct with an array of `count` WDF sections connected in series,
/// along with parameter and state fields. Provides methods for sample processing,
/// cutoff/resonance control, and reset.
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
        /// Cascaded WDF filter made of identical sections.
        ///
        /// The `params` fields are filter coefficients; `state` fields hold
        /// per-sample persistent state (e.g. feedback_prev).
        #[derive(Debug, Clone)]
        pub struct $name<T: $crate::Transcendental> {
            /// Array of WDF sections connected in series.
            pub poles: [$section; $count],
            $($pname: $ptype,)*
            $($sname: $stype,)*
        }

        impl<T: $crate::Transcendental> $name<T> {
            /// Create a new cascaded filter from a prototype section and initial parameters.
            pub fn new(section: $section, $($pname: $ptype),*) -> Self {
                Self {
                    poles: [section; $count],
                    $($pname,)*
                    $($sname: T::ZERO,)*
                }
            }

            /// Recalculate internal coefficients after parameter changes.
            pub fn update_coeffs(&mut self) { let $u_self = self; $update }

            /// Process one input sample and return the output sample.
            ///
            /// The signal passes through each WDF section in sequence.
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

            /// Return the current cutoff frequency.
            pub fn cutoff(&self) -> T { self.cutoff }
            /// Set the cutoff frequency, clamped to `[20, sample_rate / 2]`.
            pub fn set_cutoff(&mut self, cutoff: T) {
                let half_sr = self.sample_rate / T::from_f32(2.0);
                self.cutoff = cutoff.clamp(T::from_f32(20.0), half_sr);
                self.update_coeffs();
            }
            /// Return the current resonance factor.
            pub fn resonance(&self) -> T { self.resonance }
            /// Set the resonance factor, clamped to `[0, 1]`.
            pub fn set_resonance(&mut self, resonance: T) {
                self.resonance = resonance.clamp(T::ZERO, T::ONE);
            }
            /// Set the sample rate and recalculate coefficients.
            pub fn set_sample_rate(&mut self, sample_rate: T) {
                self.sample_rate = sample_rate;
                self.update_coeffs();
            }

            /// Reset all WDF section state and the feedback sample.
            pub fn reset(&mut self) {
                for p in &mut self.poles { $crate::WdfElement::reset(p); }
                self.feedback_prev = T::ZERO;
            }
        }
    };
}
