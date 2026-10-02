//! End-to-end tests for function application, including the native `list`
//! functions (`map`, `filter`, `foldl`) that call back into eris closures.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Runs `body` with `fmt`, `list` and `string` imported and `p` = `fmt.println`.
fn run(body: &str) -> Outcome {
    let source = format!(
        "{{ builtin }} -> let fmt = builtin.import \"fmt\"; l = builtin.import \"list\"; \
         s = builtin.import \"string\"; p = fmt.println; in {}",
        body
    );
    let path = std::env::temp_dir().join(format!(
        "eris-apply-test-{}-{}.eris",
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
    Outcome {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn stdout_lines(body: &str) -> Vec<String> {
    let o = run(body);
    assert_eq!(o.code, Some(0), "stderr: {}", o.stderr);
    o.stdout.lines().map(str::to_string).collect()
}

#[test]
fn map_filter_foldl_call_closures() {
    let out = stdout_lines(
        "[ (p (l.map (|x| -> x * 2) [1 2 3])) (p (l.filter (|x| -> x > 1) [1 2 3])) \
         (p (l.foldl (|a, b| -> a + b) 0 [1 2 3])) (p (l.foldl (|a, b| -> a - b) 10 [1 2 3])) ]",
    );
    assert_eq!(out, ["[ 2 4 6 ]", "[ 2 3 ]", "6", "4"]);
}

#[test]
fn partially_applied_and_curried_functions_work() {
    let out = stdout_lines(
        "[ (p (l.map ((|a, b| -> a + b) 10) [1 2 3])) (p (l.foldl (|a| -> |b| -> a * b) 1 [2 3 4])) ]",
    );
    assert_eq!(out, ["[ 11 12 13 ]", "24"]);
}

#[test]
fn native_functions_can_be_passed_to_list_functions() {
    let out = stdout_lines(
        "[ (p (l.map s.trim [ \"  a \" \" b\" ])) (p (l.foldl s.concat \"\" [ \"x\" \"y\" \"z\" ])) ]",
    );
    assert_eq!(out, ["[ \"a\" \"b\" ]", "xyz"]);
}

#[test]
fn destructuring_closures_work_through_map() {
    let out = stdout_lines(
        "[ (p (l.map ({ a; } -> a) [ { a = 1; } { a = 2; } ])) \
         (p (l.map ({ a; ... } -> a) [ { a = 1; b = 2; } ])) ]",
    );
    assert_eq!(out, ["[ 1 2 ]", "[ 1 ]"]);
}

#[test]
fn applying_a_non_function_is_reported() {
    let o = run("[ (p (l.map 5 [1 2])) ]");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Not a function: 5"), "stderr: {}", o.stderr);
}

#[test]
fn destructuring_errors_are_reported_through_map() {
    let o = run("[ (p (l.map ({ a; } -> a) [ { b = 1; } ])) ]");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Missing required attribute 'a'"), "stderr: {}", o.stderr);

    let o = run("[ (p (l.map ({ a; } -> a) [ { a = 1; b = 2; } ])) ]");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Unexpected attributes in destructuring"), "stderr: {}", o.stderr);
}

#[test]
fn errors_inside_a_mapped_function_propagate_as_poison() {
    let o = run("[ (p (l.foldl (|a, b| -> nope) 0 [1 2])) ]");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Variable 'nope' not found"), "stderr: {}", o.stderr);
    // The poisoned value never reaches `println`, so nothing is printed.
    assert_eq!(o.stdout, "");
}

#[test]
fn list_map_evaluates_every_element_even_if_the_function_ignores_it() {
    // `list.map` evaluates each element before calling the function, so an
    // undefined element is still an error even if the function ignores it.
    let o = run("[ (p (l.map (|x| -> 1) [ nope ])) ]");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Variable 'nope' not found"), "stderr: {}", o.stderr);
}

#[test]
fn recursion_through_list_functions_hits_the_limit_instead_of_crashing() {
    let o = run(
        "let f = |n| -> if n == 0 then 0 else l.foldl (|a, x| -> f x) 0 [ (n - 1) ]; \
         in [ (p (f 100000)) ]",
    );
    assert_eq!(o.code, Some(1), "stderr: {}", o.stderr);
    assert!(o.stderr.contains("Recursion limit exceeded"), "stderr: {}", o.stderr);

    // A modest depth is fine.
    let out = stdout_lines(
        "let f = |n| -> if n == 0 then 0 else l.foldl (|a, x| -> f x) 0 [ (n - 1) ]; in [ (p (f 100)) ]",
    );
    assert_eq!(out, ["0"]);
}
