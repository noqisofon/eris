//! Small pieces of surface syntax: unary minus and string escapes.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Runs `source` as a script file.
fn run_script(source: &str) -> Outcome {
    let path = std::env::temp_dir().join(format!(
        "eris-syntax-test-{}-{}.eris",
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
    Outcome {
        code: o.status.code(),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

/// Evaluates `expr` (with `f = |n| -> n + 1` and `x = 4` in scope) and returns
/// what `fmt.println` printed for it.
fn eval(expr: &str) -> Outcome {
    run_script(&format!(
        "{{ builtin }} -> let p = (builtin.import \"fmt\").println; f = |n| -> n + 1; x = 4; in [ (p ({})) ]",
        expr
    ))
}

fn printed(expr: &str) -> String {
    let o = eval(expr);
    assert_eq!(o.code, Some(0), "`{}` failed: {}", expr, o.stderr);
    o.stdout
}

#[test]
fn unary_minus_negates_ints_and_floats() {
    assert_eq!(printed("-5"), "-5\n");
    assert_eq!(printed("- 5"), "-5\n");
    assert_eq!(printed("-x"), "-4\n");
    assert_eq!(printed("-1.5"), "-1.5\n");
    assert_eq!(printed("-(1 + 2)"), "-3\n");
}

#[test]
fn unary_minus_nests() {
    assert_eq!(printed("--4"), "4\n");
    assert_eq!(printed("3 - -2"), "5\n");
}

#[test]
fn unary_minus_binds_tighter_than_multiplication_but_looser_than_application() {
    assert_eq!(printed("-2 * 3"), "-6\n");
    assert_eq!(printed("2 * -3"), "-6\n");
    assert_eq!(printed("1 + -x * 2"), "-7\n");
    // `-f 2` is `-(f 2)`, i.e. -3, not `(-f) 2`.
    assert_eq!(printed("-f 2"), "-3\n");
    assert_eq!(printed("- f 2 * 2"), "-6\n");
}

#[test]
fn binary_minus_is_unchanged() {
    assert_eq!(printed("3 - 2"), "1\n");
    assert_eq!(printed("3 -2"), "1\n");
    assert_eq!(printed("x - 1"), "3\n");
}

#[test]
fn hyphenated_identifiers_still_win_over_subtraction() {
    // `x-1` has always been one kebab-case identifier.
    let o = eval("-x-1");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Variable 'x-1' not found"), "stderr: {}", o.stderr);
}

#[test]
fn unary_minus_on_a_non_number_is_a_type_error() {
    let o = eval("-true");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Invalid type for unary minus"), "stderr: {}", o.stderr);
    let o = eval("-\"a\"");
    assert_eq!(o.code, Some(1));
}

#[test]
fn negating_the_smallest_integer_is_an_overflow_error() {
    let o = eval("-(0 - 9223372036854775807 - 1)");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Integer overflow"), "stderr: {}", o.stderr);
}

#[test]
fn list_elements_need_parentheses_for_a_leading_minus() {
    // `[ 1 -2 ]` is not a list of two numbers; the parenthesised form is.
    assert_eq!(run_script("[ 1 -2 ]").code, Some(1));
    // (`println` shows list elements unevaluated, so check them by summing.)
    assert_eq!(
        printed("(builtin.import \"list\").foldl (|a, e| -> a + e) 10 [ 1 (-2) ]"),
        "9\n"
    );
}

#[test]
fn arrow_and_lambda_syntax_still_parse() {
    assert_eq!(printed("(|a, b| -> a - b) 5 2"), "3\n");
    assert_eq!(printed("({ a; b; } -> a - -b) { a = 1; b = 2; }"), "3\n");
}

// ---- string escapes -------------------------------------------------------

/// Evaluates a script whose body is `expr`, returning stdout of `println expr`.
fn printed_string(expr: &str) -> String {
    printed(expr)
}

#[test]
fn escapes_in_double_quoted_strings() {
    assert_eq!(printed_string(r#""a\nb""#), "a\nb\n");
    assert_eq!(printed_string(r#""tab\there""#), "tab\there\n");
    assert_eq!(printed_string(r#""back\\slash""#), "back\\slash\n");
    assert_eq!(printed_string(r#""say \"hi\"""#), "say \"hi\"\n");
    assert_eq!(printed_string(r#""C:\\Users\\me""#), "C:\\Users\\me\n");
    assert_eq!(printed_string(r#""cr\rlf""#), "cr\rlf\n");
}

#[test]
fn escaped_dollar_prevents_interpolation() {
    assert_eq!(printed_string(r#""cost \$5""#), "cost $5\n");
    assert_eq!(printed_string(r#""literal \${x}""#), "literal ${x}\n");
    // ...while an unescaped one still interpolates, and a lone `$` is untouched.
    assert_eq!(printed_string(r#""value ${x}""#), "value 4\n");
    assert_eq!(printed_string(r#""plain $ dollar""#), "plain $ dollar\n");
}

#[test]
fn strings_without_backslashes_are_unchanged() {
    assert_eq!(printed_string(r#""hello, world""#), "hello, world\n");
    assert_eq!(printed_string("\"multi\nline\""), "multi\nline\n");
}

#[test]
fn single_quoted_strings_stay_raw() {
    assert_eq!(printed_string(r"'raw \n stays'"), "raw \\n stays\n");
    assert_eq!(printed_string(r"'C:\Users'"), "C:\\Users\n");
}

#[test]
fn unknown_escape_is_a_syntax_error() {
    let o = run_script(r#""bad \q escape""#);
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("unknown escape sequence '\\q'"), "stderr: {}", o.stderr);
    // The message points at the supported ones and at raw strings.
    assert!(o.stderr.contains("single-quoted"), "stderr: {}", o.stderr);
}

#[test]
fn unterminated_string_after_an_escaped_quote_is_a_syntax_error() {
    // `\"` is an escaped quote, so this string never closes.
    let o = run_script(r#""abc\""#);
    assert_eq!(o.code, Some(1));
}

#[test]
fn escapes_work_in_the_string_part_of_an_interpolation_too() {
    assert_eq!(printed_string(r#""[${x}]\n""#), "[4]\n\n");
}

// ---- interpolation of arbitrary expressions -------------------------------

/// Like `printed`, but with a record `o` and nested attribute `o.a.b` in scope.
fn interpolated(template: &str) -> String {
    let o = run_script(&format!(
        "{{ builtin }} -> let p = (builtin.import \"fmt\").println; x = 4; \
         o = {{ a = {{ b = \"deep\"; }}; n = 7; }}; f = |n| -> n + 1; in [ (p {}) ]",
        template
    ));
    assert_eq!(o.code, Some(0), "`{}` failed: {}", template, o.stderr);
    o.stdout
}

#[test]
fn interpolation_still_takes_a_plain_variable() {
    assert_eq!(interpolated(r#""plain ${x}""#), "plain 4\n");
    assert_eq!(interpolated(r#""${x}${x}""#), "44\n");
}

#[test]
fn interpolation_takes_field_access() {
    assert_eq!(interpolated(r#""n=${o.n}""#), "n=7\n");
    assert_eq!(interpolated(r#""${o.a.b}""#), "deep\n");
}

#[test]
fn interpolation_takes_arithmetic_and_calls() {
    assert_eq!(interpolated(r#""${x + 1} ${x * 2 - 1} ${-x}""#), "5 7 -4\n");
    assert_eq!(interpolated(r#""${f 2} ${f (f 2)}""#), "3 4\n");
    assert_eq!(interpolated(r#""${1.5 + 1}""#), "2.5\n");
}

#[test]
fn interpolation_takes_if_and_nested_strings_and_braces() {
    assert_eq!(interpolated(r#""${ if x > 3 then "big" else "small" }""#), "big\n");
    assert_eq!(interpolated(r#""nested ${ "inner ${x}" } done""#), "nested inner 4 done\n");
    // A `}` inside the expression does not end the interpolation early.
    assert_eq!(interpolated(r#""${ { k = 1; }.k }""#), "1\n");
}

#[test]
fn interpolation_allows_spaces_inside_the_braces() {
    assert_eq!(interpolated(r#""${ x } ${  o.n  }""#), "4 7\n");
}

#[test]
fn escaped_dollar_still_stops_interpolation_of_an_expression() {
    assert_eq!(interpolated(r#""\${o.n} stays""#), "${o.n} stays\n");
    assert_eq!(interpolated(r#""$ ${x} $""#), "$ 4 $\n");
}

#[test]
fn interpolating_a_missing_field_reports_the_field_with_a_hint() {
    let o = run_script(
        "{ builtin } -> let o = { name = 1; }; in \"v ${o.nmae}\"",
    );
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Field 'nmae' not found"), "stderr: {}", o.stderr);
    assert!(o.stderr.contains("did you mean 'name'?"), "stderr: {}", o.stderr);
}

#[test]
fn interpolating_something_that_is_not_text_or_a_number_is_an_error() {
    let o = run_script("{ builtin } -> let l = [1]; in \"v ${l}\"");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Cannot interpolate"), "stderr: {}", o.stderr);
    let o = run_script("\"v ${true}\"");
    assert_eq!(o.code, Some(1));
}

#[test]
fn errors_inside_an_interpolated_expression_are_reported() {
    let o = run_script("\"v ${1 / 0}\"");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Division by zero"), "stderr: {}", o.stderr);
}

#[test]
fn malformed_interpolations_are_syntax_errors() {
    for src in ["\"v ${}\"", "\"v ${x +}\"", "\"v ${x\""] {
        let o = run_script(src);
        assert_eq!(o.code, Some(1), "{} should fail", src);
        assert!(o.stderr.contains("Error"), "stderr: {}", o.stderr);
    }
}
