//! Task 2 slice of the io-streams memory surface: `fromList`/`toList`.
//! (Task 3 extends this file with read/atEnd/unread/connect/make*.)

use rill_lang::builtin::Registry;
use rill_lang::compile_runner;
use rill_lang::program_runner::ProgramRunner;

fn runner(src: &str) -> ProgramRunner<f32, 256> {
    compile_runner::<f32, 256>(src, &Registry::new(), 44_100.0)
        .unwrap_or_else(|e| panic!("program must compile: {e}\nsource:\n{src}"))
}

fn render(src_body: &str) -> Vec<String> {
    let src = format!(
        r#"
main = do {{
    xs <- {src_body};
    ys <- toList xs;
    write (Just (fold (\acc -> \x -> concat_string acc (show_float x)) "" ys)) stdout;
    return ()
}};
"#
    );
    let mut r = runner(&src);
    r.run().expect("run must succeed");
    r.stdout().to_vec()
}

#[test]
fn from_list_to_list_preserves_order() {
    assert_eq!(render("fromList [1.0, 2.0, 3.0]"), vec!["1.02.03.0"]);
}

#[test]
fn to_list_of_empty_stream_is_empty() {
    assert_eq!(render("fromList list"), vec![""]);
}
