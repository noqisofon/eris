//! End-to-end tests for `eris check --level type`, run against the real binary.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Outcome {
    ok: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run(subcommand: &[&str], source: &str) -> Outcome {
    let path = std::env::temp_dir().join(format!(
        "eris-check-test-{}-{}.eris",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    fs::write(&path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_eris"))
        .args(subcommand)
        .arg(&path)
        .output()
        .unwrap();
    let _ = fs::remove_file(&path);
    Outcome {
        ok: output.status.success(),
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn check_type(source: &str) -> Outcome {
    run(&["check", "--level", "type"], source)
}

#[test]
fn mismatch_in_the_result_is_reported() {
    let o = check_type("[ (1 :: string) ]");
    assert!(!o.ok);
    assert!(o.stderr.contains("Type mismatch"), "stderr: {}", o.stderr);
}

#[test]
fn well_typed_program_passes() {
    let o = check_type("let x = (1 :: int); in [ x (\"a\" :: string) ]");
    assert!(o.ok, "stderr: {}", o.stderr);
    assert!(o.stdout.starts_with("OK:"), "stdout: {}", o.stdout);
    assert!(o.stdout.contains("no type errors"));
}

#[test]
fn mismatch_in_an_unused_let_binding_is_reported() {
    let o = check_type("let x = (1 :: string); in 1");
    assert!(!o.ok);
    assert!(o.stderr.contains("Type mismatch"), "stderr: {}", o.stderr);
}

#[test]
fn mismatch_in_an_unused_function_argument_is_reported() {
    let o = check_type("let f = |x| -> 1; in f (1 :: string)");
    assert!(!o.ok);
}

#[test]
fn mismatch_in_an_unused_attrset_is_reported() {
    let o = check_type("let a = { x = (1 :: string); }; in 1");
    assert!(!o.ok);
}

#[test]
fn mismatch_in_an_unused_list_element_is_reported() {
    let o = check_type("let l = [ 1 (2 :: string) ]; in 1");
    assert!(!o.ok);
}

#[test]
fn unknown_type_name_in_an_unused_binding_is_reported() {
    let o = check_type("let x = (1 :: strng); in 1");
    assert!(!o.ok);
    assert!(o.stderr.contains("Unknown type"), "stderr: {}", o.stderr);
}

#[test]
fn other_errors_in_unused_code_are_not_type_errors() {
    // Division by zero in a binding nothing uses: `run` never notices, and
    // neither should the type check.
    let o = check_type("let x = 1 / 0; in 0");
    assert!(o.ok, "stderr: {}", o.stderr);
    let o = check_type("let x = nope; in 0");
    assert!(o.ok, "stderr: {}", o.stderr);
}

#[test]
fn errors_in_code_that_does_run_still_fail_the_check() {
    let o = check_type("1 / 0");
    assert!(!o.ok);
    assert!(o.stderr.contains("Division by zero"), "stderr: {}", o.stderr);
}

#[test]
fn code_that_is_never_reached_is_not_checked() {
    // Documented limitation: only values that get evaluated are checked.
    assert!(check_type("if false then (1 :: string) else 2").ok);
    assert!(check_type("let f = |n| -> (n :: string); in 1").ok);
}

#[test]
fn unbounded_unused_structure_terminates() {
    let o = check_type("let f = |n| -> [ n (f (n + 1)) ]; inf = f 0; in 1");
    assert!(o.ok, "stderr: {}", o.stderr);
    assert!(o.stderr.contains("stopped forcing"), "stderr: {}", o.stderr);
    // The success line must not claim a complete check.
    assert!(o.stdout.contains("incomplete"), "stdout: {}", o.stdout);
    assert!(!o.stdout.starts_with("OK:"), "stdout: {}", o.stdout);
}

#[test]
fn abort_in_an_unused_binding_does_not_end_the_check() {
    let o = check_type("{ builtin } -> let x = builtin.abort 3; in 1");
    assert!(o.ok, "stderr: {}", o.stderr);
    assert!(o.stdout.contains("no type errors"), "stdout: {}", o.stdout);
}

#[test]
fn abort_that_really_runs_still_exits_with_its_code() {
    let o = check_type("{ builtin } -> builtin.abort 3");
    assert!(!o.ok);
    assert_eq!(o.code, Some(3));
}

#[test]
fn run_does_not_execute_an_unused_abort() {
    let o = run(&["run"], "{ builtin } -> let x = builtin.abort 3; in 1");
    assert!(o.ok);
}

#[test]
fn run_stays_lazy_about_unused_annotations() {
    let o = run(&["run"], "let x = (1 :: string); in 1");
    assert!(o.ok, "stderr: {}", o.stderr);
}
