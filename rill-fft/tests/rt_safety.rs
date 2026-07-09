// rill-fft/tests/rt_safety.rs
//! RT-safety test: verifies that FFT and convolution process() calls
//! perform zero heap allocations.
//!
//! Uses a custom global allocator that panics on any allocation/deallocation.
//! Uses `thread_local!` to isolate the allocation guard per test thread,
//! so tests can run in parallel without false positives.

use num_complex::Complex;
use rill_fft::complex_fft::ComplexFft;
use rill_fft::overlap_add::OverlapAddConvolver;
use rill_fft::partitioned_conv::PartitionedConvolver;
use rill_fft::real_fft::RealFft;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

std::thread_local! {
    static ALLOC_ALLOWED: Cell<bool> = Cell::new(true);
}
static RT_TEST_ACTIVE: AtomicBool = AtomicBool::new(false);

struct PanicAllocator;

unsafe impl GlobalAlloc for PanicAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !ALLOC_ALLOWED.with(|flag| flag.get()) {
            panic!(
                "HEAP ALLOCATION IN RT PATH: size={}, align={}",
                layout.size(),
                layout.align()
            );
        }
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if !ALLOC_ALLOWED.with(|flag| flag.get()) {
            panic!("HEAP DEALLOCATION IN RT PATH");
        }
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: PanicAllocator = PanicAllocator;

fn rt_test<F: FnMut()>(mut f: F, iterations: usize) {
    struct RestoreAllocFlag;
    impl Drop for RestoreAllocFlag {
        fn drop(&mut self) {
            ALLOC_ALLOWED.with(|flag| flag.set(true));
        }
    }
    ALLOC_ALLOWED.with(|flag| flag.set(false));
    let _guard = RestoreAllocFlag;
    for _ in 0..iterations {
        f();
    }
}

fn with_rt_guard<F: FnOnce()>(f: F) {
    while RT_TEST_ACTIVE.swap(true, Ordering::Acquire) {
        std::hint::spin_loop();
    }
    f();
    RT_TEST_ACTIVE.store(false, Ordering::Release);
}

#[test]
fn test_complex_fft_no_alloc() {
    with_rt_guard(|| {
        let fft = ComplexFft::<f32>::new(1024);
        let mut data: Vec<Complex<f32>> = (0..1024)
            .map(|i| {
                let x = i as f32 * 0.01;
                Complex::new(x.sin(), x.cos())
            })
            .collect();

        rt_test(
            || {
                fft.forward(&mut data);
            },
            100,
        );
    });
}

#[test]
fn test_real_fft_no_alloc() {
    with_rt_guard(|| {
        let mut fft = RealFft::<f32>::new(1024);
        let input: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut spectrum = vec![Complex::new(0.0, 0.0); 513];

        rt_test(
            || {
                fft.forward(&input, &mut spectrum);
            },
            100,
        );
    });
}

#[test]
fn test_ola_no_alloc() {
    with_rt_guard(|| {
        let mut conv = OverlapAddConvolver::<f32, 128>::new(1024);
        let ir: Vec<f32> = (0..1024).map(|i| 0.99f32.powi(i as i32)).collect();
        conv.set_ir(&ir);
        let input = vec![0.5f32; 128];
        let mut output = vec![0.0f32; 128];

        rt_test(
            || {
                conv.process(&input, &mut output);
            },
            50,
        );
    });
}

#[test]
fn test_partitioned_conv_no_alloc() {
    with_rt_guard(|| {
        let mut conv = PartitionedConvolver::<f32, 64>::new(4096);
        let ir: Vec<f32> = (0..4096).map(|i| 0.999f32.powi(i as i32)).collect();
        conv.set_ir(&ir);
        let input = vec![0.5f32; 64];
        let mut output = vec![0.0f32; 64];

        rt_test(
            || {
                conv.process(&input, &mut output);
            },
            20,
        );
    });
}
