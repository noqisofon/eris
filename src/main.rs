pub mod ast;
pub mod eval;
pub mod native;
pub mod parser;
pub mod value;

use crate::eval::evaluate;
use crate::parser::parser;
use crate::value::{Env, Thunk, Value};
use chumsky::Parser;
use std::env;
use std::fs;

fn deep_force(val: Value) -> Result<Value, String> {
    match val {
        Value::List(thunks) => {
            let mut res = Vec::new();
            for t in thunks {
                res.push(Thunk::evaluated(deep_force(evaluate(t.clone())?)?));
            }
            Ok(Value::List(res))
        }
        Value::AttrSet(map) => {
            let mut res = std::collections::HashMap::new();
            for (k, t) in map {
                res.insert(k, Thunk::evaluated(deep_force(evaluate(t.clone())?)?));
            }
            Ok(Value::AttrSet(res))
        }
        other => Ok(other),
    }
}

enum Command {
    Run(String),
    Check(String),
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  eris <file.eris>          # same as `eris run <file.eris>`");
    eprintln!("  eris run <file.eris>      # run a script");
    eprintln!("  eris check <file.eris>    # syntax check only");
}

fn parse_args() -> Command {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("run") => match args.get(2) {
            Some(file) => Command::Run(file.clone()),
            None => {
                print_usage();
                std::process::exit(1);
            }
        },
        Some("check") => match args.get(2) {
            Some(file) => Command::Check(file.clone()),
            None => {
                print_usage();
                std::process::exit(1);
            }
        },
        Some(file) => Command::Run(file.to_string()),
        None => {
            print_usage();
            std::process::exit(1);
        }
    }
}

/// Parses `source`, printing any syntax errors under `filename`. Returns `None`
/// (after printing diagnostics) if parsing failed.
fn parse_source(filename: &str, source: &str) -> Option<crate::ast::Expr> {
    let (opt_ast, errs) = parser()
        .then_ignore(chumsky::prelude::end())
        .parse(source)
        .into_output_errors();

    if !errs.is_empty() {
        use ariadne::{Color, Label, Report, ReportKind, Source};
        for err in errs {
            Report::build(
                ReportKind::Error,
                (
                    filename.to_string(),
                    err.span().into_range().start..err.span().into_range().start,
                ),
            )
            .with_message(err.to_string())
            .with_label(
                Label::new((filename.to_string(), err.span().into_range()))
                    .with_message(err.reason().to_string())
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((filename.to_string(), Source::from(source)))
            .unwrap();
        }
    }

    opt_ast
}

fn run_check(filename: &str) {
    let source = fs::read_to_string(filename).unwrap_or_else(|err| {
        eprintln!("Error reading file {}: {}", filename, err);
        std::process::exit(1);
    });

    match parse_source(filename, &source) {
        Some(_) => {
            println!("OK: {} has no syntax errors", filename);
        }
        None => {
            std::process::exit(1);
        }
    }
}

fn run_script(filename: &str) {
    let source = fs::read_to_string(filename).unwrap_or_else(|err| {
        eprintln!("Error reading file {}: {}", filename, err);
        std::process::exit(1);
    });

    let expr = match parse_source(filename, &source) {
        Some(ast) => ast,
        None => {
            std::process::exit(1);
        }
    };

    let env = Env::new(
        std::rc::Rc::new(source.clone()),
        std::rc::Rc::new(filename.to_string()),
    );
    let thunk = Thunk::new(expr, env);
    let mut result_val = evaluate(thunk).unwrap_or_else(|e| {
        eprintln!("Runtime error: {}", e);
        std::process::exit(1);
    });

    if let Value::Closure {
        args,
        body,
        env: closure_env,
    } = result_val
    {
        // Produce the __native AttrSet from native.rs
        let native_val = crate::native::build_native_env();

        // Evaluate builtin.eris script using include_str!
        let builtin_source = include_str!("builtin.eris");
        let (builtin_opt, builtin_errs) = parser()
            .then_ignore(chumsky::prelude::end())
            .parse(builtin_source)
            .into_output_errors();

        if !builtin_errs.is_empty() {
            use ariadne::{Color, Label, Report, ReportKind, Source};
            for err in builtin_errs {
                Report::build(
                    ReportKind::Error,
                    (
                        "builtin.eris",
                        err.span().into_range().start..err.span().into_range().start,
                    ),
                )
                .with_message(err.to_string())
                .with_label(
                    Label::new(("builtin.eris", err.span().into_range()))
                        .with_message(err.reason().to_string())
                        .with_color(Color::Red),
                )
                .finish()
                .eprint(("builtin.eris", Source::from(builtin_source)))
                .unwrap();
            }
        }

        let builtin_parse = builtin_opt.unwrap_or_else(|| {
            std::process::exit(1);
        });

        let builtin_env = Env::new(
            std::rc::Rc::new(builtin_source.to_string()),
            std::rc::Rc::new("builtin.eris".to_string()),
        );
        let builtin_thunk = Thunk::new(builtin_parse, builtin_env);
        let builtin_closure = evaluate(builtin_thunk).unwrap_or_else(|e| {
            eprintln!("Runtime error evaluating builtin.eris: {}", e);
            std::process::exit(1);
        });

        // Pass { __native } to builtin_closure to get the `builtin` module
        let builtin_val = if let Value::Closure {
            args: b_args,
            body: b_body,
            env: b_env,
        } = builtin_closure
        {
            let call_env = b_env.extend();
            if let crate::ast::Args::Destructure { names, .. } = b_args {
                for name in names {
                    if name == "__native" {
                        call_env.define(name, Thunk::evaluated(native_val.clone()));
                    }
                }
            }
            crate::eval::eval_expr(&b_body, &call_env).unwrap_or_else(|e| {
                eprintln!("Runtime error applying __native to builtin.eris: {}", e);
                std::process::exit(1);
            })
        } else {
            eprintln!("builtin.eris did not return a closure");
            std::process::exit(1);
        };

        let call_env = closure_env.extend();
        if let crate::ast::Args::Destructure { names, .. } = args {
            for name in names {
                if name == "builtin" {
                    call_env.define(name, Thunk::evaluated(builtin_val.clone()));
                }
            }
        }
        result_val = crate::eval::eval_expr(&body, &call_env).unwrap_or_else(|e| {
            eprintln!("Runtime error: {}", e);
            std::process::exit(1);
        });
    }

    match deep_force(result_val) {
        Ok(_val) => {
            // Evaluated successfully
        }
        Err(e) => eprintln!("Runtime error: {}", e),
    }
}

fn main() {
    let command = parse_args();

    // Chumsky 0.12 in debug mode uses massive stack space that overflows the default 1MB Windows stack.
    // Instead of forcing users to build in release mode, we spawn the interpreter in an 8MB stack thread.
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || match command {
            Command::Run(filename) => run_script(&filename),
            Command::Check(filename) => run_check(&filename),
        })
        .unwrap()
        .join()
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval_code(source: &str) -> String {
        let (opt_ast, errs) = parser()
            .then_ignore(chumsky::prelude::end())
            .parse(source)
            .into_output_errors();
        if !errs.is_empty() {
            use ariadne::{Color, Label, Report, ReportKind, Source};
            for err in errs {
                Report::build(
                    ReportKind::Error,
                    (
                        "test",
                        err.span().into_range().start..err.span().into_range().start,
                    ),
                )
                .with_message(err.to_string())
                .with_label(
                    Label::new(("test", err.span().into_range()))
                        .with_message(err.reason().to_string())
                        .with_color(Color::Red),
                )
                .finish()
                .eprint(("test", Source::from(source)))
                .unwrap();
            }
        }
        let ast = opt_ast.unwrap();
        let env = Env::new(
            std::rc::Rc::new(source.to_string()),
            std::rc::Rc::new("test".to_string()),
        );
        let thunk = Thunk::new(ast, env);
        let val = deep_force(evaluate(thunk).unwrap()).unwrap();
        format!("{:?}", val)
    }

    #[test]
    fn test_math() {
        assert_eq!(eval_code("1 + 2 * 3"), "7");
        assert_eq!(eval_code("(1 + 2) * 3"), "9");
        assert_eq!(eval_code("10 / 2 - 1"), "4");
        assert_eq!(eval_code("1 + 4.5"), "5.5");
        assert_eq!(eval_code("10 - 5"), "5");
    }

    #[test]
    fn test_kebab_case() {
        assert_eq!(eval_code("let write-file = 42; in write-file"), "42");
        assert_eq!(eval_code("let my-var = { a-b = 10; }; in my-var.a-b"), "10");
        assert_eq!(eval_code("let f = |x-y| -> x-y * 2; in f 5"), "10"); // using positional lambda
    }

    #[test]
    fn test_string_interpolation() {
        assert_eq!(
            eval_code("let name = \"Eris\"; in \"Hello, ${name}!\""),
            "\"Hello, Eris!\""
        );
    }

    #[test]
    fn test_attr_set() {
        assert_eq!(eval_code("let obj = { a = 1; b = 2; }; in obj.a"), "1");
        assert_eq!(
            eval_code("let obj = { inner = { a = 42; }; }; in obj.inner.a"),
            "42"
        );
    }

    #[test]
    fn test_rec_attr_set() {
        assert_eq!(eval_code("let r = rec { a = b; b = 99; }; in r.a"), "99");
        // Deeper nested recursion referencing variables in scope
        assert_eq!(
            eval_code("let x = 5; r = rec { a = b + x; b = 10; }; in r.a"),
            "15"
        );
    }

    #[test]
    fn test_lambda_currying() {
        assert_eq!(eval_code("let add = |a, b| -> a + b; in add 10 20"), "30");
        assert_eq!(eval_code("let succ = |n| -> n + 1; in succ 5"), "6");
        // Test partial application
        assert_eq!(
            eval_code("let add = |a, b| -> a + b; add5 = add 5; in add5 10"),
            "15"
        );
    }

    #[test]
    fn test_destructuring_lambda() {
        assert_eq!(
            eval_code("let f = {x; y} -> x + y; in f { x = 10; y = 20; }"),
            "30"
        );
        assert_eq!(
            eval_code("let f = {x; ...} -> x; in f { x = 10; y = 20; z = 30; }"),
            "10"
        );
    }

    #[test]
    fn test_lists() {
        // Using format strings tests output exactly
        assert_eq!(eval_code("let a = 1; in [ a 2 3 ]"), "[ 1 2 3 ]");
        assert_eq!(eval_code("[ \"foo\" \"bar\" ]"), "[ \"foo\" \"bar\" ]");
    }

    #[test]
    fn test_path() {
        assert_eq!(eval_code("p'./my/file.txt'"), "p'\"./my/file.txt\"'");
    }

    #[test]
    fn test_with() {
        assert_eq!(eval_code("let obj = { a = 1; }; in with obj .a"), "1");
        assert_eq!(eval_code("with { a = { b = 42; }; } .a.b"), "42");
    }

    #[test]
    fn test_bool_and_comparisons() {
        assert_eq!(eval_code("true"), "true");
        assert_eq!(eval_code("false"), "false");
        assert_eq!(eval_code("10 > 5"), "true");
        assert_eq!(eval_code("10 < 5"), "false");
        assert_eq!(eval_code("10 >= 10"), "true");
        assert_eq!(eval_code("10 <= 9"), "false");
        assert_eq!(eval_code("42 == 42"), "true");
        assert_eq!(eval_code("42 != 42"), "false");
        assert_eq!(eval_code("\"abc\" == \"abc\""), "true");
        assert_eq!(eval_code("\"abc\" != \"def\""), "true");
    }

    #[test]
    fn test_logical_ops() {
        assert_eq!(eval_code("true && false"), "false");
        assert_eq!(eval_code("true || false"), "true");
        assert_eq!(eval_code("false && true"), "false");
        assert_eq!(eval_code("false || true"), "true");
    }

    #[test]
    fn test_if_else() {
        assert_eq!(eval_code("if true then 1 else 2"), "1");
        assert_eq!(eval_code("if false then 1 else 2"), "2");
        assert_eq!(
            eval_code("if 10 > 5 && true then \"yes\" else \"no\""),
            "\"yes\""
        );
    }
}
