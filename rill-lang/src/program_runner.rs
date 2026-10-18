//! `ProgramRunner` — the executor of a compiled `main : IO ()` program.
//!
//! The runner owns the pair the world-state split separates: the compiled
//! [`Ir`] (pure code) and the [`WorldState`] (all runtime state, including the
//! foreign factory registry the world owns by value). It runs `main : IO ()` to
//! completion against that world, performing whatever memory-stream and stdio
//! effects the program requests. It takes no input/output buffers or backend
//! handles from outside — streams are created inside the program.
//!
//! The executable is built with [`compile`](crate::compile), which compiles in
//! `Complete` (lazy) mode unless the source carries its own
//! `{-# MODE … #-}` pragma and rejects a `main` that does not type as `IO ()`.
//!
//! # Seating and repeated runs
//!
//! [`run`](ProgramRunner::run) seats the world for the IR before executing, so
//! the runner needs no eager assembly: a caller may pre-seed the world
//! (`world_mut().params.sample_rate = 48_000.0`, `set_param(…)`) and the seat
//! keeps every supplied value (declared parameter defaults are seeded only
//! where the bag has no value). Re-seating re-instantiates the built-in
//! instances and re-reads the parameter store, so a `set_param` between two runs
//! is what the second run observes.
//!
//! # Console
//!
//! The runner owns the program's realized stdio. `stdout` and `stderr` are
//! **captured**, not echoed to the process console, and the captured consoles
//! are **cumulative across runs**: the world's console is never cleared, so
//! after `n` calls to [`ProgramRunner::run`] the accessors return everything
//! written since construction, in program order. A driver that wants
//! process-visible output must drain [`stdout`](ProgramRunner::stdout) /
//! [`stderr`](ProgramRunner::stderr) itself.
//!
//! # Stack depth
//!
//! The lazy `IO` interpreter nests one `bind` per statement in a `do` block:
//! evaluating `main`'s action to completion recurses once per statement, and in
//! a debug build one statement costs roughly 300 KiB of native stack. A
//! multi-statement program therefore exceeds a default 2 MiB thread stack and
//! aborts with a stack overflow — a documented limit, not a clean error. Run
//! such programs on a grown-stack thread with [`run_on_stack`] (or the
//! equivalent `std::thread::Builder::stack_size`). Trampolining the `bind`
//! chain to remove the limit is a planned follow-up.

use rill_core::math::Transcendental;

use crate::error::CompileError;
use crate::ir::Ir;
use crate::program::WorldState;

/// The executor of a compiled rill-lang program.
///
/// [`run`](Self::run) seats the world for the runner's IR and runs the
/// program's `main : IO ()` to completion. The captured console is available
/// through [`stdout`](Self::stdout) and [`stderr`](Self::stderr); it is captured
/// (not echoed) and cumulative across runs. Multi-statement programs need a
/// grown stack — see [`run_on_stack`].
pub struct ProgramRunner<T: Transcendental, const BUF: usize> {
    /// The compiled program: pure code, no runtime state.
    pub(crate) ir: Ir,
    /// The realized `realWorld`: streams, captured console, signal runtime,
    /// parameters, the foreign factory registry, and the value track.
    pub(crate) world: WorldState<T, BUF>,
}

impl<T: Transcendental, const BUF: usize> ProgramRunner<T, BUF> {
    /// Wrap a compiled IR and its world in a runner.
    pub fn new(ir: Ir, world: WorldState<T, BUF>) -> Self {
        Self { ir, world }
    }

    /// Inject the console input lines that `read stdin` yields.
    ///
    /// The runner owns the program's realized stdio: `stdout`/`stderr` are
    /// captured into the world's console (not echoed to the process console),
    /// and `stdin` is a finite, line-oriented input the caller supplies (rill
    /// has no `ByteString`, so each line is one `String` element). Injecting
    /// rewinds the stream, so the next [`run`](Self::run) reads the lines from
    /// the start and reports EOF after them.
    pub fn set_stdin(&mut self, lines: &[String]) {
        self.world.inject_stdin(lines);
    }

    /// Seat the world for the runner's IR and run `main : IO ()` to
    /// completion.
    ///
    /// The seat is idempotent for the parameter store (declared defaults are
    /// seeded only where the world's bag has no value, so a caller's pre-seed
    /// and any `set_param` between runs survive) and re-instantiates the
    /// built-in instances from the world's registry, so a parameter written
    /// between two runs reaches the built-ins bound to it.
    ///
    /// A program whose `main` is not an executable `IO ()` action (including an
    /// action whose final result is not `()`), or whose evaluation exhausts the
    /// fixed arena, yields a [`CompileError`] rather than panicking; an
    /// unresolvable built-in yields a [`CompileError`] at seat time.
    ///
    /// This method runs on the calling thread's stack. The lazy `IO`
    /// interpreter costs roughly 300 KiB of stack per `do` statement in a debug
    /// build, so run multi-statement programs via [`run_on_stack`] (or a
    /// `std::thread::Builder::stack_size` thread of your own).
    pub fn run(&mut self) -> Result<(), CompileError> {
        self.world.seat(&self.ir)?;
        crate::backend::interp::run_main_io(&self.ir, &mut self.world)
            .map_err(|e| CompileError::Unsupported(format!("run failed: {e}")))
    }

    /// Lines written to `stdout` by **all runs since construction**, in order.
    ///
    /// The console is cumulative: it is not cleared between runs, and it is
    /// captured rather than echoed to the process console.
    pub fn stdout(&self) -> &[String] {
        &self.world.stdout
    }

    /// Lines written to `stderr` by **all runs since construction**, in order.
    ///
    /// The console is cumulative: it is not cleared between runs, and it is
    /// captured rather than echoed to the process console.
    pub fn stderr(&self) -> &[String] {
        &self.world.stderr
    }

    /// The compiled program's IR (pure code), for inspection.
    pub fn ir(&self) -> &Ir {
        &self.ir
    }

    /// The world the runner executes against, for inspection (parameters, the
    /// value arena, the captured console).
    pub fn world(&self) -> &WorldState<T, BUF> {
        &self.world
    }

    /// Mutable access to the world (parameter control, pre-seeding the sample
    /// rate, injecting console input).
    pub fn world_mut(&mut self) -> &mut WorldState<T, BUF> {
        &mut self.world
    }
}

/// Run `f` on a dedicated thread with at least `stack_size` bytes of stack,
/// returning its result.
///
/// # Why this exists
///
/// The lazy `IO` interpreter nests one `bind` per statement in a `do` block:
/// evaluating `main`'s action to completion recurses once per statement and, in
/// a debug build, one statement costs roughly 300 KiB of native stack. A
/// multi-statement stream program therefore overflows libtest's default 2 MiB
/// thread (an abort, not a clean error). [`ProgramRunner`] is not `Send` — it
/// owns single-threaded buffer state — so a built runner cannot be moved into
/// the thread; the whole compile-then-run sequence must happen **inside** `f`:
///
/// ```
/// use rill_lang::{compile, ffi::ForeignRegistry, run_on_stack};
///
/// let (stdout, ()) = run_on_stack(16 << 20, || {
///     let mut r = compile::<f32, 256>(
///         r#"main = do { write (Just "hi") stdout; return () };"#,
///         ForeignRegistry::new(),
///     )
///     .unwrap();
///     r.run().unwrap();
///     (r.stdout().to_vec(), ())
/// });
/// assert_eq!(stdout, ["hi"]);
/// ```
///
/// Using `stack_size` below the interpreter's requirement is the caller's
/// choice; the documented limit is the interpreter's recursion depth, not this
/// helper's.
///
/// A panic inside `f` is resumed on the calling thread (`resume_unwind`), so
/// the caller sees the original panic payload. Failing to spawn the thread
/// panics (it is a host-resource failure, not a program error).
pub fn run_on_stack<R: Send + 'static>(
    stack_size: usize,
    f: impl FnOnce() -> R + Send + 'static,
) -> R {
    std::thread::Builder::new()
        .stack_size(stack_size)
        .spawn(f)
        .expect("spawn the stack-grown runner thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}
