//! `==` `!=` `<` `<=` `>` `>=` across the value types.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run_script(source: &str) -> Outcome {
    let path = std::env::temp_dir().join(format!(
        "eris-compare-test-{}-{}.eris",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    fs::write(&path, source).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_eris"))
        .arg("run")
        .arg(&path)
        .output()
        .unwrap();
    let _ = fs::remove_file(&path);
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

/// Wraps `expr` in a script with a few helpers in scope (`f` a function, `nan`).
fn script(body: &str) -> String {
    format!(
        "{{ builtin }} -> let p = (builtin.import \"fmt\").println; f = |n| -> n; \
         nan = 0.0 / 0.0; nest = |n| -> if n == 0 then [] else [ (nest (n - 1)) ]; in {}",
        body
    )
}

fn eval(expr: &str) -> Outcome {
    run_script(&script(&format!("[ (p ({})) ]", expr)))
}

/// Evaluates every expression in one script and returns what each printed.
fn eval_all(exprs: &[&str]) -> Vec<String> {
    let items: String = exprs.iter().map(|e| format!("(p ({})) ", e)).collect();
    let o = run_script(&script(&format!("[ {} ]", items)));
    assert_eq!(o.code, Some(0), "stderr: {}", o.stderr);
    o.stdout.lines().map(str::to_string).collect()
}

fn assert_all(cases: &[(&str, &str)]) {
    let exprs: Vec<&str> = cases.iter().map(|(e, _)| *e).collect();
    let got = eval_all(&exprs);
    assert_eq!(got.len(), cases.len(), "got: {:?}", got);
    for ((expr, want), got) in cases.iter().zip(&got) {
        assert_eq!(got, want, "`{}`", expr);
    }
}

fn assert_error(expr: &str, message: &str) {
    let o = eval(expr);
    assert_eq!(o.code, Some(1), "`{}` should fail, stdout: {}", expr, o.stdout);
    assert!(o.stderr.contains(message), "`{}`: stderr was {}", expr, o.stderr);
}

#[test]
fn ints_and_floats_compare_by_numeric_value() {
    assert_all(&[
        ("1 == 1.0", "true"),
        ("1.0 == 1", "true"),
        ("1 != 1.0", "false"),
        ("1 == 2.0", "false"),
        ("1 != 2.0", "true"),
        ("0 == 0.0 - 0.0", "true"),
    ]);
}

#[test]
fn ordering_works_across_ints_and_floats() {
    assert_all(&[
        ("1 < 1.5", "true"),
        ("1.5 > 1", "true"),
        ("2 >= 1.5", "true"),
        ("1 <= 1.0", "true"),
        ("1 < 1.0", "false"),
        ("2 < 1.5", "false"),
        ("0 - 1 < 0 - 1.5", "false"),
        ("0 - 2 < 0 - 1.5", "true"),
    ]);
}

#[test]
fn int_float_comparison_is_exact_beyond_two_to_the_53() {
    assert_all(&[
        // A float conversion would round these together.
        ("9007199254740993 == 9007199254740992.0", "false"),
        ("9007199254740993 > 9007199254740992.0", "true"),
        ("9007199254740992 == 9007199254740992.0", "true"),
        ("9007199254740991 < 9007199254740992.0", "true"),
        ("9223372036854775807 < 9223372036854775808.0", "true"),
    ]);
}

#[test]
fn strings_compare_and_order_bytewise() {
    assert_all(&[
        (r#""a" == "a""#, "true"),
        (r#""a" == "b""#, "false"),
        (r#""a" < "b""#, "true"),
        (r#""b" <= "a""#, "false"),
        (r#""abc" < "abd""#, "true"),
        (r#""ab" < "abc""#, "true"),
        (r#""" < "a""#, "true"),
        // Uppercase sorts before lowercase: this is code point order, not locale order.
        (r#""Z" < "a""#, "true"),
        (r#""a" >= "a""#, "true"),
    ]);
}

#[test]
fn paths_compare_as_strings() {
    assert_all(&[
        ("p'./a' == p'./a'", "true"),
        ("p'./a' == p'./b'", "false"),
        // No normalisation: these are different strings.
        ("p'./a' == p'a'", "false"),
        ("p'./a/../b' == p'./b'", "false"),
        ("p'a' != p'b'", "true"),
    ]);
}

#[test]
fn bools_compare_for_equality() {
    assert_all(&[("true == true", "true"), ("true == false", "false"), ("true != false", "true")]);
}

#[test]
fn lists_compare_structurally() {
    assert_all(&[
        ("[ 1 ] == [ 1 ]", "true"),
        ("[] == []", "true"),
        ("[ 1 2 3 ] == [ 1 2 3 ]", "true"),
        ("[ 1 2 3 ] == [ 1 2 4 ]", "false"),
        ("[ 1 ] == [ 1 2 ]", "false"),
        ("[ 1 2 ] != [ 1 2 ]", "false"),
        ("[ [ 1 ] [ 2 ] ] == [ [ 1 ] [ 2 ] ]", "true"),
        ("[ [ 1 ] [ 2 ] ] == [ [ 1 ] [ 3 ] ]", "false"),
        // Elements follow the same rules as top-level values.
        ("[ 1 2 ] == [ 1 2.0 ]", "true"),
        (r#"[ "a" p'b' true ] == [ "a" p'b' true ]"#, "true"),
        // Order matters in a list.
        ("[ 1 2 ] == [ 2 1 ]", "false"),
    ]);
}

#[test]
fn attrsets_compare_structurally_regardless_of_key_order() {
    assert_all(&[
        ("{ a = 1; b = 2; } == { b = 2; a = 1; }", "true"),
        ("{} == {}", "true"),
        ("{ a = 1; } == { a = 2; }", "false"),
        ("{ a = 1; } == { b = 1; }", "false"),
        ("{ a = 1; } == { a = 1; b = 2; }", "false"),
        ("{ a = { b = [ 1 ]; }; } == { a = { b = [ 1.0 ]; }; }", "true"),
        ("{ a = 1; } != { a = 1; }", "false"),
    ]);
}

#[test]
fn values_of_different_types_are_not_equal() {
    assert_all(&[
        (r#"1 == "a""#, "false"),
        ("true == 1", "false"),
        ("[ 1 ] == { a = 1; }", "false"),
        (r#"p'a' == "a""#, "false"),
        ("[] == {}", "false"),
        (r#"1 != "a""#, "true"),
        ("[ 1 ] != 1", "true"),
    ]);
}

#[test]
fn nan_is_not_equal_to_anything_including_itself() {
    assert_all(&[
        ("nan == nan", "false"),
        ("nan != nan", "true"),
        ("nan == 1", "false"),
        ("1 == nan", "false"),
        ("[ nan ] == [ nan ]", "false"),
        // Unordered: every ordering operator is false.
        ("nan < 1", "false"),
        ("nan <= 1", "false"),
        ("nan > 1", "false"),
        ("nan >= 1", "false"),
        ("1 < nan", "false"),
        ("nan < nan", "false"),
    ]);
}

#[test]
fn functions_cannot_be_compared() {
    assert_error("f == f", "Cannot compare functions");
    assert_error("f != f", "Cannot compare functions");
    assert_error("f == 1", "Cannot compare functions");
    assert_error("1 == f", "Cannot compare functions");
    assert_error("[ f ] == [ f ]", "Cannot compare functions");
    assert_error("{ a = f; } == { a = f; }", "Cannot compare functions");
}

#[test]
fn only_numbers_and_strings_can_be_ordered() {
    assert_error("true < false", "Cannot order two bool values");
    assert_error("[ 1 ] < [ 2 ]", "Cannot order two list values");
    assert_error("{ a = 1; } < { a = 2; }", "Cannot order two attrset values");
    assert_error("p'a' < p'b'", "Cannot order two path values");
    assert_error(r#""a" < 1"#, "Cannot order a string and an int");
    assert_error(r#"1 >= "a""#, "Cannot order an int and a string");
    assert_error("f < f", "Cannot order");
}

// ---- laziness and errors ----------------------------------------------------

#[test]
fn elements_are_only_evaluated_as_far_as_needed() {
    // The first elements already differ, so the broken ones are never touched.
    assert_all(&[
        ("[ 1 nope ] == [ 2 nope ]", "false"),
        // Different lengths / key sets are decided without looking at any element.
        ("[ nope ] == [ 1 2 ]", "false"),
        ("{ a = nope; } == { b = nope; }", "false"),
        ("{ a = nope; } == { a = nope; b = 1; }", "false"),
    ]);
}

#[test]
fn an_element_that_fails_to_evaluate_poisons_the_comparison() {
    for expr in ["[ nope ] == [ nope ]", "{ a = nope; } == { a = 1; }", "[ 1 nope ] == [ 1 2 ]"] {
        let o = eval(expr);
        assert_eq!(o.code, Some(1), "`{}`", expr);
        assert!(o.stderr.contains("Variable 'nope' not found"), "`{}`: {}", expr, o.stderr);
        // Nothing was printed for the poisoned comparison.
        assert_eq!(o.stdout, "", "`{}`", expr);
    }
}

#[test]
fn an_element_that_fails_natively_is_a_runtime_error() {
    let o = run_script(&script(
        "let l = builtin.import \"list\"; in [ (p ([ (l.head []) ] == [ 1 ])) ]",
    ));
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("head on empty list"), "stderr: {}", o.stderr);
}

// ---- deep and self-referential data -----------------------------------------

#[test]
fn moderately_nested_lists_compare_fine() {
    assert_all(&[
        ("nest 1000 == nest 1000", "true"),
        ("nest 5000 == nest 5000", "true"),
        ("nest 1000 == nest 999", "false"),
    ]);
}

#[test]
fn absurdly_nested_lists_end_in_the_nesting_limit_error_not_a_crash() {
    assert_error("nest 100000 == nest 100000", "nested too deeply");
}

#[test]
fn a_value_containing_itself_can_be_compared_with_itself() {
    // Each pair of elements is compared once, so a cycle ends instead of hanging.
    assert_all(&[
        ("let s = [ 1 s ]; in s == s", "true"),
        ("let s = { x = s; }; in s == s", "true"),
        ("let s = [ 1 s ]; in s != s", "false"),
    ]);
}

#[test]
fn cyclic_values_of_the_same_shape_are_equal_and_of_different_shapes_are_not() {
    assert_all(&[
        ("let a = [ 1 a ]; b = [ 1 b ]; in a == b", "true"),
        ("let a = { x = a; }; b = { x = b; }; in a == b", "true"),
        ("let a = [ 1 a ]; b = [ 2 b ]; in a == b", "false"),
        ("let a = [ 1 a ]; b = [ 1 [ 1 b ] ]; in a == b", "true"),
        ("let a = [ 1 a ]; b = [ 1 [ 2 b ] ]; in a == b", "false"),
    ]);
}

#[test]
fn a_cyclic_value_differing_early_is_decided_before_the_cycle_is_reached() {
    assert_all(&[("let s = [ 1 s ]; t = [ 2 t ]; in s == t", "false")]);
}

/// `let l0 = [ 1 ]; m0 = [ 1 ]; l1 = [ l0 l0 ]; m1 = [ m0 m0 ]; ... in <body>`.
fn shared_levels(levels: usize, body: &str) -> String {
    let mut bindings = String::from("l0 = [ 1 ]; m0 = [ 1 ]; ");
    for i in 1..=levels {
        bindings.push_str(&format!(
            "l{i} = [ l{p} l{p} ]; m{i} = [ m{p} m{p} ]; ",
            i = i,
            p = i - 1
        ));
    }
    format!("let {} in {}", bindings, body)
}

#[test]
fn heavily_shared_structures_compare_in_linear_time() {
    // Each level refers to the level below it twice, so walked as a tree this is
    // 2^40 steps. Compared pair by pair it is 40.
    assert_all(&[
        (&shared_levels(40, "l40 == l40"), "true"),
        (&shared_levels(40, "l40 != l40"), "false"),
        // Two separately built structures with the same sharing are just as fast.
        (&shared_levels(40, "l40 == m40"), "true"),
        (&shared_levels(40, "l40 == m39"), "false"),
    ]);
}

#[test]
fn a_shared_element_is_still_compared_normally_the_first_time() {
    // Sharing never changes what a comparison means: a NaN is not equal to
    // itself, a function cannot be compared, and a broken element fails.
    assert_all(&[
        ("let l = [ nan ]; in l == l", "false"),
        ("let l = [ nan 1 ]; in [ l l ] == [ l l ]", "false"),
        ("[ nan ] == [ nan ]", "false"),
        ("nan == nan", "false"),
    ]);
    assert_error("let g = [ f ]; in g == g", "Cannot compare functions");
    assert_error("let g = [ f ]; h = [ f ]; in g == h", "Cannot compare functions");
    assert_error("let g = [ nope ]; in g == g", "Variable 'nope' not found");
    assert_error("let g = [ (1 / 0) ]; in [ g ] == [ g ]", "Division by zero");
}

#[test]
fn distinct_elements_that_fail_still_poison_the_comparison() {
    let o = eval("[ nope ] == [ nope ]");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Variable 'nope' not found"), "stderr: {}", o.stderr);
}

#[test]
fn long_flat_lists_compare_without_trouble() {
    let items = "1 ".repeat(100_000);
    let o = run_script(&script(&format!("[ (p ([ {} ] == [ {} ])) ]", items, items)));
    assert_eq!(o.code, Some(0), "stderr: {}", o.stderr);
    assert_eq!(o.stdout, "true\n");
}

// ---- comparison results are ordinary values ----------------------------------

#[test]
fn comparisons_compose_with_if_and_logic() {
    assert_all(&[
        (r#"if [ 1 2 ] == [ 1 2 ] then "same" else "different""#, "same"),
        ("1 == 1.0 && \"a\" < \"b\"", "true"),
        ("[ 1 ] == [ 2 ] || { a = 1; } == { a = 1; }", "true"),
    ]);
}
