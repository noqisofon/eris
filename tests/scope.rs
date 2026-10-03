//! `eris check` and unbound variables: found statically, without running anything.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn check_with(args: &[&str], source: &str) -> Outcome {
    let path = std::env::temp_dir().join(format!(
        "eris-scope-test-{}-{}.eris",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    fs::write(&path, source).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_eris"))
        .arg("check")
        .args(args)
        .arg(&path)
        .output()
        .unwrap();
    let _ = fs::remove_file(&path);
    // Strip ANSI colour codes from the diagnostics.
    let raw = String::from_utf8_lossy(&o.stderr);
    let mut stderr = String::new();
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

fn check(source: &str) -> Outcome {
    check_with(&[], source)
}

fn assert_clean(source: &str) {
    let o = check(source);
    assert_eq!(o.code, Some(0), "`{}` should be clean, stderr: {}", source, o.stderr);
    assert!(o.stdout.contains("no syntax or scope errors"), "stdout: {}", o.stdout);
}

fn assert_unbound(source: &str, name: &str) {
    let o = check(source);
    assert_eq!(o.code, Some(1), "`{}` should fail", source);
    assert!(
        o.stderr.contains(&format!("Variable '{}' not found", name)),
        "`{}`: stderr was {}",
        source,
        o.stderr
    );
}

// ---- detected without running ---------------------------------------------

#[test]
fn an_unbound_variable_is_reported() {
    assert_unbound("nope", "nope");
    assert_unbound("1 + nope", "nope");
}

#[test]
fn unbound_variables_are_found_in_code_that_never_runs() {
    // A branch not taken.
    assert_unbound("if false then nope else 1", "nope");
    // The body of a function nobody calls.
    assert_unbound("let f = |x| -> nope; in 1", "nope");
    // A binding nothing uses.
    assert_unbound("let unused = nope; in 1", "nope");
    // Inside a string interpolation.
    assert_unbound("\"v ${nope}\"", "nope");
    assert_unbound("\"v ${1 + nope}\"", "nope");
}

#[test]
fn unbound_variables_are_found_in_every_construct() {
    assert_unbound("[ 1 nope ]", "nope");
    assert_unbound("{ a = nope; }", "nope");
    assert_unbound("rec { a = nope; }", "nope");
    assert_unbound("nope.field", "nope");
    assert_unbound("f nope", "f");
    assert_unbound("(|x| -> x) nope", "nope");
    assert_unbound("-nope", "nope");
    assert_unbound("(nope :: int)", "nope");
    assert_unbound("with nope; .a", "nope");
}

#[test]
fn a_bare_name_in_a_with_body_is_not_bound_by_the_with_object() {
    // `with` only provides `.name`; plain `a` is an ordinary variable.
    assert_unbound("with { a = 1; }; a", "a");
}

#[test]
fn a_plain_attrset_does_not_let_values_see_their_siblings() {
    assert_unbound("{ a = 1; b = a; }", "a");
}

#[test]
fn every_unbound_use_is_reported_not_just_the_first() {
    let o = check("[ one two ]");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Variable 'one' not found"), "stderr: {}", o.stderr);
    assert!(o.stderr.contains("Variable 'two' not found"), "stderr: {}", o.stderr);
}

#[test]
fn hints_come_from_every_enclosing_scope() {
    let o = check("let apple = 1; in let x = 2; in appel");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("did you mean 'apple'?"), "stderr: {}", o.stderr);
}

#[test]
fn nothing_close_means_no_hint() {
    let o = check("let apple = 1; in zzzzzzzzzz");
    assert_eq!(o.code, Some(1));
    assert!(!o.stderr.contains("did you mean"), "stderr: {}", o.stderr);
}

// ---- no false positives ----------------------------------------------------

#[test]
fn bound_variables_are_accepted() {
    assert_clean("let x = 1; in x");
    assert_clean("(|x| -> x) 1");
    assert_clean("(|a, b| -> a + b) 1 2");
    assert_clean("({ a; b; } -> a + b) { a = 1; b = 2; }");
    assert_clean("rec { a = 1; b = a; }");
}

#[test]
fn let_is_recursive_so_bindings_can_refer_to_each_other_and_themselves() {
    assert_clean("let a = b + 1; b = 2; in a");
    assert_clean("let ev = |n| -> od n; od = |n| -> ev n; in ev 1");
    // Whether it terminates is a run-time matter; scope-wise `x` is bound.
    assert_clean("let x = x; in 1");
}

#[test]
fn rec_attrsets_allow_forward_references() {
    assert_clean("rec { a = b; b = 2; }");
}

#[test]
fn inner_bindings_shadow_outer_ones() {
    assert_clean("let x = 1; in let x = 2; in x");
    assert_clean("let f = |x| -> (|x| -> x); in 1");
}

#[test]
fn with_dot_names_are_not_variables() {
    assert_clean("with { a = 1; }; .a");
    assert_clean("with { a = 1; } .a");
    assert_clean("let o = { a = 1; }; in with o; .a + 1");
}

#[test]
fn type_annotation_names_are_not_variables() {
    assert_clean("(1 :: int)");
    assert_clean("let x = (\"a\" :: string); in x");
}

#[test]
fn hyphenated_names_are_single_identifiers() {
    assert_clean("let write-file = 1; in write-file");
}

#[test]
fn interpolation_can_use_bound_variables_and_expressions() {
    assert_clean("let x = 1; o = { n = 2; }; in \"${x} ${o.n} ${x + 1}\"");
}

// ---- the script's outermost lambda -----------------------------------------

#[test]
fn the_host_provides_builtin_and_native_to_the_outermost_lambda() {
    assert_clean("{ builtin } -> builtin");
    assert_clean("{ builtin; ... } -> builtin.import \"fmt\"");
    assert_clean("{ __native } -> __native");
}

#[test]
fn other_outermost_parameters_are_not_provided_so_using_one_is_an_error() {
    // Never supplied by the host, so using `foo` fails at run time.
    assert_unbound("{ foo } -> foo", "foo");
    assert_unbound("|x| -> x", "x");
    // Not using it is fine, as it is at run time.
    assert_clean("{ foo } -> 1");
}

#[test]
fn only_the_outermost_lambda_is_special() {
    // Lambdas that are called by the program bind all their parameters.
    assert_clean("{ builtin } -> (|x| -> x) 1");
    assert_clean("{ builtin } -> (({ foo } -> foo) { foo = 1; })");
    // ...and one that is not the script's own outermost expression is not
    // second-guessed either.
    assert_clean("let f = { foo } -> foo; in 1");
}

#[test]
fn a_missing_builtin_argument_gets_a_hint_about_how_to_receive_it() {
    let o = check("{} -> builtins.import \"fmt\"");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Variable 'builtins' not found"), "stderr: {}", o.stderr);
    assert!(o.stderr.contains("`{ builtin } ->`"), "stderr: {}", o.stderr);
}

// ---- levels and the bundled sources -----------------------------------------

#[test]
fn syntax_errors_are_still_reported_first() {
    let o = check("let x = ; in x");
    assert_eq!(o.code, Some(1));
    assert!(!o.stderr.contains("scope errors"), "stderr: {}", o.stderr);
}

#[test]
fn type_level_does_not_run_a_script_with_an_unbound_variable() {
    let marker = std::env::temp_dir().join(format!("eris-scope-marker-{}", std::process::id()));
    let _ = fs::remove_file(&marker);
    let source = format!(
        "{{ builtin }} -> let fs = builtin.import \"fs\"; \
         written = fs.write_file \"{}\" \"ran\"; \
         dead = if false then nope else 1; in written",
        marker.display().to_string().replace('\\', "\\\\")
    );
    let o = check_with(&["--level", "type"], &source);
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Variable 'nope' not found"), "stderr: {}", o.stderr);
    assert!(!marker.exists(), "the script ran even though it has an unbound variable");
}

#[test]
fn type_level_still_runs_a_clean_script() {
    let o = check_with(&["--level", "type"], "let x = (1 :: int); in x");
    assert_eq!(o.code, Some(0), "stderr: {}", o.stderr);
    assert!(o.stdout.contains("no type errors"), "stdout: {}", o.stdout);
}

#[test]
fn every_bundled_module_is_clean() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0;
    for entry in fs::read_dir(src).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("eris") {
            continue;
        }
        let o = check(&fs::read_to_string(&path).unwrap());
        assert_eq!(o.code, Some(0), "{} should be clean: {}", path.display(), o.stderr);
        checked += 1;
    }
    assert!(checked >= 10, "expected to find the bundled modules, found {}", checked);
}
