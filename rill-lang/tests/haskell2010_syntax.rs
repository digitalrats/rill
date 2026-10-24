//! Spec smoke: a program written in the full Haskell 2010 surface syntax
//! (backslash lambdas, `--` line comments, layout/offside `do`, curried
//! application, nested `map`s) compiles and runs through the runner.

use rill_lang::compile;
use rill_lang::ffi::ForeignRegistry;

#[test]
fn haskell2010_syntax_end_to_end() {
    // The whole program is written in the new surface: `--` comments, `\x ->`
    // lambdas, top-level defs separated by lines (no braces/semicolons), and a
    // layout `do` block whose statements live at the same indentation column.
    let src = r#"
        -- gain and offset composition
        gain = \x -> x * 0.5
        offset = \x -> x + 1.0
        main = do
            xs <- fromList [1.0, 3.0]
            ws <- toList xs
            ys <- fromList (map offset (map gain ws))
            zs <- toList ys
            write (Just (fold (\acc -> \x -> concat_string acc (show_float x)) "" zs)) stdout
            return ()
    "#;
    let mut runner = compile::<f32, 256>(src, ForeignRegistry::new())
        .expect("the Haskell 2010 program must compile");
    runner.run().expect("the compiled program must run");
    assert_eq!(runner.stdout(), ["1.52.5"]);
}
