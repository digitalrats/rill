/// Generate a single WDF element struct implementing `WdfElement`.
///
/// Defines port resistance, scattering, state update, and reset behaviour
/// via closures provided at the macro call site.
#[macro_export]
macro_rules! wdf_element {
    (
        name: $name:ident<T>,
        params: { $($pname:ident: $ptype:ty),* },
        state: { $($sname:ident: $stype:ty),* },
        port_resistance: |$pr_self:ident| $pr:expr,
        scattering: |$p:ident, $a:ident| $scatter:expr,
        update: |$u:ident| $update:expr,
        reset: |$r:ident| $reset:expr,
    ) => {
        /// A single WDF element.
        ///
        /// `params` fields are element parameters (e.g. resistance, capacitance);
        /// `state` fields hold per-sample state. `voltage` and `current` are
        /// computed by the WDF traversal.
        #[derive(Debug, Clone, Copy)]
        pub struct $name<T> {
            $($pname: $ptype,)*
            $($sname: $stype,)*
            voltage: T,
            current: T,
        }

        impl<T: $crate::Transcendental> $name<T> {
            /// Create a new element with the given parameters and zeroed state.
            pub fn new($($pname: $ptype),*) -> Self {
                Self { $($pname,)* $($sname: T::ZERO,)* voltage: T::ZERO, current: T::ZERO }
            }
        }

        impl<T: $crate::Transcendental> $crate::WdfElement<T> for $name<T> {
            fn port_resistance(&self) -> T { let $pr_self = self; $pr }
            fn process_incident(&mut self, a: T) -> T { let $p = self; let $a = a; $scatter }
            fn update_state(&mut self) { let $u = self; $update }
            fn voltage(&self) -> T { self.voltage }
            fn current(&self) -> T { self.current }
            fn reset(&mut self) { let $r = self; $reset }
        }
    };
}
