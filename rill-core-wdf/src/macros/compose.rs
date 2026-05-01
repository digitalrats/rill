/// Compose two WDF elements into Series or Parallel.
#[macro_export]
macro_rules! wdf_compose {
    (
        name: $name:ident<T>,
        kind: Series,
        elements: ($left:ty, $right:ty),
    ) => {
        #[derive(Debug, Clone, Copy)]
        pub struct $name<T: $crate::Transcendental> {
            pub left: $left,
            pub right: $right,
        }

        impl<T: $crate::Transcendental> $name<T> {
            pub fn new(left: $left, right: $right) -> Self {
                Self { left, right }
            }
        }

        impl<T: $crate::Transcendental> $crate::WdfElement<T> for $name<T> {
            fn port_resistance(&self) -> T {
                self.left.port_resistance() + self.right.port_resistance()
            }
            fn process_incident(&mut self, a: T) -> T {
                let r1 = self.left.port_resistance();
                let r2 = self.right.port_resistance();
                let total = r1 + r2;
                let a1 = a * (r1 / total);
                let a2 = a * (r2 / total);
                let b1 = self.left.process_incident(a1);
                let b2 = self.right.process_incident(a2);
                b1 * (r1 / total) + b2 * (r2 / total)
            }
            fn update_state(&mut self) {
                self.left.update_state();
                self.right.update_state();
            }
            fn voltage(&self) -> T {
                self.left.voltage() + self.right.voltage()
            }
            fn current(&self) -> T { self.left.current() }
            fn reset(&mut self) {
                self.left.reset();
                self.right.reset();
            }
        }
    };
    (
        name: $name:ident<T>,
        kind: Parallel,
        elements: ($left:ty, $right:ty),
    ) => {
        #[derive(Debug, Clone, Copy)]
        pub struct $name<T: $crate::Transcendental> {
            pub left: $left,
            pub right: $right,
        }

        impl<T: $crate::Transcendental> $name<T> {
            pub fn new(left: $left, right: $right) -> Self {
                Self { left, right }
            }
        }

        impl<T: $crate::Transcendental> $crate::WdfElement<T> for $name<T> {
            fn port_resistance(&self) -> T {
                let r1 = self.left.port_resistance();
                let r2 = self.right.port_resistance();
                (r1 * r2) / (r1 + r2)
            }
            fn process_incident(&mut self, a: T) -> T {
                let r1 = self.left.port_resistance();
                let r2 = self.right.port_resistance();
                let g1 = T::ONE / r1;
                let g2 = T::ONE / r2;
                let total_g = g1 + g2;
                let two = T::from_f32(2.0);
                let a1 = two * g1 / total_g;
                let a2 = two * g2 / total_g;
                let b1 = self.left.process_incident(a);
                let b2 = self.right.process_incident(a);
                a1 * b1 + a2 * b2 - a
            }
            fn update_state(&mut self) {
                self.left.update_state();
                self.right.update_state();
            }
            fn voltage(&self) -> T { self.left.voltage() }
            fn current(&self) -> T {
                self.left.current() + self.right.current()
            }
            fn reset(&mut self) {
                self.left.reset();
                self.right.reset();
            }
        }
    };
}
