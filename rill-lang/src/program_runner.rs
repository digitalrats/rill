//! `ProgramRunner` — the executor of a compiled program.
//!
//! Where the old runner was a pure signal transform (`&[&[f32]]` →
//! `&mut [&mut [f32]]`), this one **executes the program**: it runs
//! `main : (String…) -> IO ()` to completion against the realized `World`,
//! performing whatever memory-stream and stdio effects the program requests.
//! The runner takes no input/output buffers or backend handles from outside —
//! streams are created inside the program.
//!
//! The executable is built with [`compile_runner`](crate::compile_runner), which
//! compiles in `Complete` (lazy) mode unless the source carries its own
//! `{-# MODE … #-}` pragma.
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
use rill_core::traits::ProcessError;

use crate::program::RillProgram;

/// The executor of a compiled rill-lang program.
///
/// Bind launch arguments with [`set_args`](Self::set_args), then
/// [`run`](Self::run) the program's `main : IO ()` to completion. The captured
/// console is available through [`stdout`](Self::stdout) and
/// [`stderr`](Self::stderr); it is captured (not echoed) and cumulative across
/// runs. Multi-statement programs need a grown stack — see [`run_on_stack`].
pub struct ProgramRunner<T: Transcendental, const BUF: usize> {
    program: RillProgram<T, BUF>,
    stdout: Vec<String>,
    stderr: Vec<String>,
}

impl<T: Transcendental, const BUF: usize> ProgramRunner<T, BUF> {
    /// Wrap an already-compiled program in a runner.
    pub fn new(program: RillProgram<T, BUF>) -> Self {
        Self {
            program,
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    /// Bind `main`'s String λ-parameters.
    ///
    /// The `n`-th argument binds `main`'s `n`-th λ-parameter. Parameters with
    /// no corresponding argument default to the empty string. Extra arguments
    /// are ignored (the source has no such parameter).
    pub fn set_args(&mut self, args: &[String]) {
        for cell in 0..self.program.num_main_args() {
            let value = args.get(cell).map_or("", String::as_str);
            self.program.set_main_arg(cell, value);
        }
    }

    /// Inject the console input lines that `read stdin` yields.
    ///
    /// The runner owns the program's realized stdio: `stdout`/`stderr` are
    /// captured into the runner's console (not echoed to the process console),
    /// and `stdin` is a finite, line-oriented input the caller supplies (rill
    /// has no `ByteString`, so each line is one `String` element). Injecting
    /// rewinds the stream, so the next [`run`](Self::run) reads the lines from
    /// the start and reports EOF after them.
    pub fn set_stdin(&mut self, lines: &[String]) {
        self.program.inject_stdin(lines);
    }

    /// Run `main : IO ()` to completion against the realized `World`.
    ///
    /// The captured console is refreshed even when the run fails, so a caller
    /// can inspect what the program wrote before the error. The refresh is a
    /// snapshot of the console built up by **all runs since construction** (the
    /// world console is cumulative, not per-run). A program whose `main` is not
    /// an executable `IO ()` action (including an action whose final result is
    /// not `()`), or whose evaluation exhausts the fixed arena, yields a
    /// [`ProcessError`] rather than panicking.
    ///
    /// This method runs on the calling thread's stack. The lazy `IO`
    /// interpreter costs roughly 300 KiB of stack per `do` statement in a debug
    /// build, so run multi-statement programs via [`run_on_stack`] (or a
    /// `std::thread::Builder::stack_size` thread of your own).
    pub fn run(&mut self) -> Result<(), ProcessError> {
        let result = crate::backend::interp::run_main_io(&mut self.program);
        let world = self.program.world.borrow();
        self.stdout.clone_from(&world.stdout);
        self.stderr.clone_from(&world.stderr);
        result
    }

    /// Lines written to `stdout` by **all runs since construction**, in order.
    ///
    /// The console is cumulative: it is not cleared between runs, and it is
    /// captured rather than echoed to the process console.
    pub fn stdout(&self) -> &[String] {
        &self.stdout
    }

    /// Lines written to `stderr` by **all runs since construction**, in order.
    ///
    /// The console is cumulative: it is not cleared between runs, and it is
    /// captured rather than echoed to the process console.
    pub fn stderr(&self) -> &[String] {
        &self.stderr
    }

    /// The underlying compiled program, for inspection (parameters, IR).
    pub fn program(&self) -> &RillProgram<T, BUF> {
        &self.program
    }

    /// Mutable access to the underlying compiled program (parameter control).
    pub fn program_mut(&mut self) -> &mut RillProgram<T, BUF> {
        &mut self.program
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
/// owns `Rc<RefCell<…>>` state — so a built runner cannot be moved into the
/// thread; the whole compile-then-run sequence must happen **inside** `f`:
///
/// ```
/// use rill_lang::{compile_runner, run_on_stack, Registry};
///
/// let (stdout, ()) = run_on_stack(16 << 20, || {
///     let mut r = compile_runner::<f32, 256>(
///         r#"main = do { write (Just "hi") stdout; return () };"#,
///         &Registry::new(),
///         44_100.0,
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
