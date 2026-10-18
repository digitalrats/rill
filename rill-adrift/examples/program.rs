//! Run a rill-lang program to completion through the IO runner.
//!
//! Usage:
//!   cargo run --example program [program.rill]
//!
//! With no `.rill` path, a small self-contained demo program runs. A program's
//! `main` is a pure `IO ()` action — there are no String launch arguments. The
//! runner takes no I/O handles from outside — the program creates its own
//! streams — and the console it captures (`stdout`/`stderr`) is printed here
//! after the run.

use rill_lang::compile_runner;

const DEMO: &str = r#"
main = do {
    write (Just "Hello, world!") stdout;
    xs <- fromList [1.0, 2.0, 3.0];
    ys <- toList xs;
    write (Just (fold (\acc -> \x -> concat_string acc (show_float x)) "" ys)) stdout;
    return ()
};
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let source = match args.first() {
        Some(path) if path.ends_with(".rill") => std::fs::read_to_string(path)?,
        _ => DEMO.to_string(),
    };

    #[cfg(feature = "lofi")]
    let registry = rill_adrift::lang_builtins::full_registry_f32();
    #[cfg(not(feature = "lofi"))]
    let registry = rill_adrift::lang_builtins::full_registry::<f32>();

    let mut runner = compile_runner::<f32, 256>(&source, &registry, 44_100.0)?;
    runner.run()?;

    // The runner never echoes to the process console: drain its captured
    // console so the program's output is visible.
    for line in runner.stdout() {
        println!("{line}");
    }
    for line in runner.stderr() {
        eprintln!("{line}");
    }
    Ok(())
}
