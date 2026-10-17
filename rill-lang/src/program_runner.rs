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

use rill_core::math::Transcendental;
use rill_core::traits::ProcessError;

use crate::program::RillProgram;

/// The executor of a compiled rill-lang program.
///
/// Bind launch arguments with [`set_args`](Self::set_args), then
/// [`run`](Self::run) the program's `main : IO ()` to completion. The captured
/// console is available through [`stdout`](Self::stdout) and
/// [`stderr`](Self::stderr).
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
    /// captured into the runner's console, and `stdin` is a finite,
    /// line-oriented input the caller supplies (rill has no `ByteString`, so
    /// each line is one `String` element). Injecting rewinds the stream, so the
    /// next [`run`](Self::run) reads the lines from the start and reports EOF
    /// after them.
    pub fn set_stdin(&mut self, lines: &[String]) {
        self.program.inject_stdin(lines);
    }

    /// Run `main : IO ()` to completion against the realized `World`.
    ///
    /// The captured console is refreshed even when the run fails, so a caller
    /// can inspect what the program wrote before the error. A program whose
    /// `main` is not an executable `IO ()` action, or whose evaluation exhausts
    /// the fixed arena, yields a [`ProcessError`] rather than panicking.
    pub fn run(&mut self) -> Result<(), ProcessError> {
        let result = crate::backend::interp::run_main_io(&mut self.program);
        let world = self.program.world.borrow();
        self.stdout.clone_from(&world.stdout);
        self.stderr.clone_from(&world.stderr);
        result
    }

    /// Lines written to `stdout` by the last [`run`](Self::run), in order.
    pub fn stdout(&self) -> &[String] {
        &self.stdout
    }

    /// Lines written to `stderr` by the last [`run`](Self::run), in order.
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
