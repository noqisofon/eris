//! `builtin.abort`: an exit code (int) or a message (string).

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run_with(args: &[&str], source: &str) -> Outcome {
    let path = std::env::temp_dir().join(format!(
        "eris-abort-test-{}-{}.eris",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    fs::write(&path, source).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_eris"))
        .args(args)
        .arg(&path)
        .output()
        .unwrap();
    let _ = fs::remove_file(&path);
    let mut stderr = String::new();
    // Strip ANSI colour codes from diagnostics.
    let raw = String::from_utf8_lossy(&o.stderr);
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        } else {
            stderr.push(c);
        }
    }
    Outcome {
        code: o.status.code(),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr,
    }
}

/// Runs a script that prints "before" and then evaluates `abort_expr`.
fn run_abort(abort_expr: &str) -> Outcome {
    run_with(
        &["run"],
        &format!(
            "{{ builtin }} -> let p = (builtin.import \"fmt\").println; in [ (p \"before\") ({}) ]",
            abort_expr
        ),
    )
}

#[test]
fn an_int_is_the_exit_code_and_nothing_is_printed() {
    let o = run_abort("builtin.abort 3");
    assert_eq!(o.code, Some(3));
    assert_eq!(o.stderr, "");
    // What ran before the abort still got printed.
    assert_eq!(o.stdout, "before\n");
}

#[test]
fn zero_is_a_successful_exit() {
    assert_eq!(run_abort("builtin.abort 0").code, Some(0));
}

#[test]
fn a_string_is_reported_on_stderr_with_exit_code_1() {
    let o = run_abort("builtin.abort \"boom\"");
    assert_eq!(o.code, Some(1));
    assert_eq!(o.stderr, "abort: boom\n");
    assert_eq!(o.stdout, "before\n");
}

#[test]
fn a_computed_or_multiline_message_is_reported_in_full() {
    let o = run_abort("builtin.abort \"x ${1 + 2}\"");
    assert_eq!(o.code, Some(1));
    assert_eq!(o.stderr, "abort: x 3\n");

    let o = run_abort("builtin.abort \"line1\\nline2\"");
    assert_eq!(o.code, Some(1));
    assert_eq!(o.stderr, "abort: line1\nline2\n");
}

#[test]
fn any_other_type_is_a_type_error_instead_of_a_silent_exit() {
    for arg in ["true", "[ 1 ]", "1.5", "{ a = 1; }"] {
        let o = run_abort(&format!("builtin.abort {}", arg));
        assert_eq!(o.code, Some(1), "abort {}", arg);
        assert!(
            o.stderr.contains("abort expects an exit code (int) or a message (string)"),
            "abort {}: stderr was {:?}",
            arg,
            o.stderr
        );
    }
}

#[test]
fn an_abort_nobody_evaluates_does_nothing() {
    let o = run_with(
        &["run"],
        "{ builtin } -> let x = builtin.abort \"boom\"; in 1",
    );
    assert_eq!(o.code, Some(0), "stderr: {}", o.stderr);
    assert_eq!(o.stderr, "");
}

#[test]
fn type_check_does_not_run_or_report_an_abort_in_an_unused_binding() {
    let o = run_with(
        &["check", "--level", "type"],
        "{ builtin } -> let a = builtin.abort \"boom\"; b = builtin.abort 3; c = builtin.abort true; in 1",
    );
    assert_eq!(o.code, Some(0), "stderr: {}", o.stderr);
    assert!(o.stdout.contains("no type errors"), "stdout: {}", o.stdout);
    assert!(!o.stderr.contains("boom"), "stderr: {}", o.stderr);
}

#[test]
fn an_abort_that_really_runs_still_ends_the_type_check() {
    let o = run_with(&["check", "--level", "type"], "{ builtin } -> builtin.abort \"stop\"");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("abort: stop"), "stderr: {}", o.stderr);
}
