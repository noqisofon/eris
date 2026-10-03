//! "did you mean" suggestions for names that cannot be found.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Runs `source` and returns (exit code, stderr with ANSI colours stripped).
fn run(source: &str) -> (Option<i32>, String) {
    let path = std::env::temp_dir().join(format!(
        "eris-hints-test-{}-{}.eris",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    fs::write(&path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_eris"))
        .arg("run")
        .arg(&path)
        .output()
        .unwrap();
    let _ = fs::remove_file(&path);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut plain = String::new();
    let mut chars = stderr.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        } else {
            plain.push(c);
        }
    }
    (output.status.code(), plain)
}

#[test]
fn hint_comes_from_the_same_scope() {
    let (code, err) = run("let apple = 1; in appel");
    assert_eq!(code, Some(1));
    assert!(err.contains("did you mean 'apple'?"), "stderr: {}", err);
}

#[test]
fn hint_comes_from_an_enclosing_scope() {
    let (code, err) = run("let apple = 1; in let x = 2; in appel");
    assert_eq!(code, Some(1));
    assert!(err.contains("did you mean 'apple'?"), "stderr: {}", err);
}

#[test]
fn hint_comes_from_a_function_argument_scope() {
    let (_, err) = run("let apple = 1; f = |banana| -> bananna; in f 2");
    assert!(err.contains("did you mean 'banana'?"), "stderr: {}", err);
}

#[test]
fn interpolation_hints_look_through_enclosing_scopes_too() {
    let (code, err) = run("let apple = 1; in let x = 2; in \"v ${appel}\"");
    assert_eq!(code, Some(1));
    assert!(err.contains("did you mean 'apple'?"), "stderr: {}", err);
}

#[test]
fn nothing_close_means_no_hint() {
    let (code, err) = run("let apple = 1; in let x = 2; in zzzzzzzzzz");
    assert_eq!(code, Some(1));
    assert!(err.contains("Variable 'zzzzzzzzzz' not found"), "stderr: {}", err);
    assert!(!err.contains("did you mean"), "stderr: {}", err);
}

#[test]
fn the_innermost_of_equally_close_names_wins() {
    // `aple` and `apple` are both within reach of `appel`; the inner one is chosen.
    let (_, err) = run("let apple = 1; in let aple = 3; in appel");
    assert!(err.contains("did you mean 'aple'?"), "stderr: {}", err);
}

/// `HashMap` iteration order changes between processes, so a single run can be
/// lucky; run the same script several times and require one stable answer.
fn hints_over_runs(source: &str, runs: usize) -> std::collections::BTreeSet<String> {
    (0..runs)
        .map(|_| {
            let (_, err) = run(source);
            let start = err.find("did you mean").expect(&err);
            err[start..].lines().next().unwrap().to_string()
        })
        .collect()
}

#[test]
fn field_hint_among_equally_close_names_is_deterministic() {
    // `print`, `printf` and `println` are all one edit away from `printn`.
    let hints = hints_over_runs(
        "{ builtin } -> let fmt = builtin.import \"fmt\"; in fmt.printn \"x\"",
        12,
    );
    assert_eq!(hints.len(), 1, "got different hints: {:?}", hints);
    assert!(hints.iter().next().unwrap().contains("'print'"), "{:?}", hints);
}

#[test]
fn implicit_access_hint_among_equally_close_names_is_deterministic() {
    let hints = hints_over_runs("with { print = 1; printf = 2; println = 3; } .printn", 12);
    assert_eq!(hints.len(), 1, "got different hints: {:?}", hints);
    assert!(hints.iter().next().unwrap().contains("'print'"), "{:?}", hints);
}

#[test]
fn the_note_for_a_missing_builtin_argument_names_the_argument_that_works() {
    // The name that is actually injected is `builtin`, not `builtins`.
    let (code, err) = run("{} -> builtins.import \"fmt\"");
    assert_eq!(code, Some(1));
    assert!(err.contains("`{ builtin } ->`"), "stderr: {}", err);
    assert!(!err.contains("`{ builtins } ->`"), "stderr: {}", err);
}
