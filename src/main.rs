pub mod ast;
pub mod eval;
pub mod native;
pub mod parser;
pub mod value;

use crate::eval::evaluate;
use crate::parser::parse_source;
use crate::value::{Env, Thunk, Value};
use clap::{CommandFactory, Parser, Subcommand};
use std::fs;

fn deep_force(val: Value) -> Result<Value, String> {
    deep_force_at(val, 0)
}

fn deep_force_at(val: Value, depth: usize) -> Result<Value, String> {
    crate::eval::check_data_depth(depth)?;
    match val {
        Value::List(thunks) => {
            let mut res = Vec::new();
            for t in thunks {
                res.push(Thunk::evaluated(deep_force_at(
                    evaluate(t.clone())?,
                    depth + 1,
                )?));
            }
            Ok(Value::List(res))
        }
        Value::AttrSet(map) => {
            let mut res = std::collections::HashMap::new();
            for (k, t) in map {
                res.insert(
                    k,
                    Thunk::evaluated(deep_force_at(evaluate(t.clone())?, depth + 1)?),
                );
            }
            Ok(Value::AttrSet(res))
        }
        other => Ok(other),
    }
}

#[derive(Parser)]
#[command(name = "eris", version, about = "Eris language interpreter")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Script file to run (same as `eris run <file>`)
    file: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a script
    Run { file: String },
    /// Check a script without running it
    Check {
        file: String,
        /// `syntax` only parses the file; `type` also runs it and checks
        /// every `expr :: type` annotation as it gets forced.
        #[arg(long, value_enum, default_value_t = CheckLevel::Syntax)]
        level: CheckLevel,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum CheckLevel {
    Syntax,
    Type,
}

#[derive(Clone)]
enum Command {
    Run(String),
    Check(String, CheckLevel),
}

fn parse_args() -> Command {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Run { file }) => Command::Run(file),
        Some(Commands::Check { file, level }) => Command::Check(file, level),
        None => match cli.file {
            Some(file) => Command::Run(file),
            None => {
                Cli::command().print_help().ok();
                println!();
                std::process::exit(1);
            }
        },
    }
}

fn run_check(filename: &str, level: CheckLevel) {
    if level == CheckLevel::Type {
        run_check_type(filename);
        return;
    }

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

/// Parses and fully evaluates `filename`, forcing every value it produces so
/// that any `expr :: type` annotation along the way gets checked. (`check
/// --level type` then goes on to force the values nothing needed; see
/// `force_unused_thunks`.) Errors are
/// reported via `crate::eval`'s ariadne diagnostics; check `eval::had_error()`
/// afterwards to see whether anything went wrong.
fn evaluate_file(filename: &str) -> Value {
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
        let builtin_parse = parse_source("builtin.eris", builtin_source).unwrap_or_else(|| {
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

    deep_force(result_val).unwrap_or_else(|e| {
        eprintln!("Runtime error: {}", e);
        std::process::exit(1);
    })
}

fn run_script(filename: &str) {
    evaluate_file(filename);
    if crate::eval::had_error() {
        std::process::exit(1);
    }
}

/// Upper bound on how many otherwise-unused values `check --level type` will
/// force. Lazily built infinite structures would otherwise never finish.
const MAX_EXTRA_FORCED_THUNKS: usize = 1_000_000;

/// Forces every thunk the program created but never needed (unused `let`
/// bindings, unused attributes and list elements, unused function arguments), so
/// that `::` annotations inside them get checked too. Errors other than type
/// mismatches are ignored here (see `eval::set_lenient`). Code that is never
/// reached by evaluation at all (an untaken `if` branch, a function nobody calls)
/// is still not checked. Returns `false` if it had to stop early.
fn force_unused_thunks() -> bool {
    crate::eval::set_lenient(true);
    let mut complete = true;
    let mut forced = 0;
    let mut index = 0;
    while let Some(thunk) = crate::value::tracked_thunk(index) {
        index += 1;
        if !thunk.is_unevaluated() {
            continue;
        }
        if forced >= MAX_EXTRA_FORCED_THUNKS {
            eprintln!(
                "note: stopped forcing unused values after {}; the rest were not type-checked",
                MAX_EXTRA_FORCED_THUNKS
            );
            complete = false;
            break;
        }
        forced += 1;
        // A runtime failure in code the program never ran is not a type error.
        let _ = evaluate(thunk);
    }
    crate::eval::set_lenient(false);
    crate::value::stop_tracking_thunks();
    complete
}

fn run_check_type(filename: &str) {
    crate::eval::reset_error_flag();
    crate::value::start_tracking_thunks();
    evaluate_file(filename);
    let complete = force_unused_thunks();
    if crate::eval::had_error() {
        eprintln!("FAILED: {} has type errors", filename);
        std::process::exit(1);
    } else if complete {
        println!("OK: {} has no type errors", filename);
    } else {
        // The exit code stays 0 so existing CI setups keep working, but the line
        // says plainly that the check did not cover everything.
        println!(
            "OK (incomplete: stopped after {} values): {} has no type errors in what was checked",
            MAX_EXTRA_FORCED_THUNKS, filename
        );
    }
}

/// Preferred stack size of the interpreter thread. The evaluator is recursive,
/// and an unoptimised build burns tens of KB of stack per eris-level call, so
/// debug builds need far more than release builds to reach
/// `eval::DEFAULT_MAX_EVAL_DEPTH`. The memory is only reserved, not committed,
/// until it is actually used.
const INTERPRETER_STACK_SIZE: usize = if cfg!(debug_assertions) {
    512 * 1024 * 1024
} else {
    64 * 1024 * 1024
};

/// Smallest stack worth running on; below this we give up rather than crash later.
const MIN_INTERPRETER_STACK_SIZE: usize = 8 * 1024 * 1024;

/// The process's address-space limit (`ulimit -v`), if one is set and we can
/// find out about it. Only Linux exposes this without extra dependencies.
fn address_space_limit() -> Option<usize> {
    let limits = fs::read_to_string("/proc/self/limits").ok()?;
    let line = limits.lines().find(|l| l.starts_with("Max address space"))?;
    line.split_whitespace().find_map(|w| w.parse::<usize>().ok())
}

fn main() {
    let command = parse_args();

    // Chumsky 0.12 in debug mode uses massive stack space that overflows the default 1MB Windows stack,
    // and the recursive evaluator needs far more than the main thread offers.
    // Instead of forcing users to build in release mode, we spawn the interpreter in a big-stack thread.
    //
    // Reserving that much address space can fail (e.g. under `ulimit -v` or in a small
    // container), so retry with progressively smaller stacks, scaling the recursion
    // limit down with whatever we actually got.
    //
    // A successful reservation is not enough under an address-space limit: a stack that eats
    // most of it leaves nothing for the heap. So never ask for more than an eighth of the limit (the allocator also reserves arena space for the thread).
    let mut stack_size = match address_space_limit() {
        Some(limit) => INTERPRETER_STACK_SIZE
            .min(limit / 8)
            .max(MIN_INTERPRETER_STACK_SIZE),
        None => INTERPRETER_STACK_SIZE,
    };
    let handle = loop {
        crate::eval::set_max_depth_for_stack(stack_size);
        let command = command.clone();
        match std::thread::Builder::new()
            .stack_size(stack_size)
            .spawn(move || match command {
                Command::Run(filename) => run_script(&filename),
                Command::Check(filename, level) => run_check(&filename, level),
            }) {
            Ok(handle) => break handle,
            Err(_) if stack_size / 2 >= MIN_INTERPRETER_STACK_SIZE => stack_size /= 2,
            Err(err) => {
                eprintln!("Failed to start the interpreter thread: {}", err);
                std::process::exit(1);
            }
        }
    };
    handle.join().unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval_code(source: &str) -> String {
        let ast = parse_source("test", source).unwrap();
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
    fn test_int_overflow_is_poison() {
        assert_eq!(eval_code("9223372036854775807 + 1"), "<poison>");
        assert_eq!(eval_code("(0 - 9223372036854775807 - 1) / (0 - 1)"), "<poison>");
    }

    #[test]
    fn test_interpolation_missing_var_is_error() {
        assert_eq!(eval_code("\"hi ${nope}\""), "<poison>");
    }

    #[test]
    fn test_huge_int_literal_is_syntax_error() {
        let (_, errs) = crate::parser::parse("99999999999999999999");
        assert!(
            errs.iter().any(|e| e.to_string().contains("out of range")),
            "expected an out-of-range error, got: {:?}",
            errs.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_unbounded_recursion_hits_limit() {
        // Test threads get a small stack, so run on one sized like the real
        // interpreter thread.
        let out = std::thread::Builder::new()
            .stack_size(INTERPRETER_STACK_SIZE)
            .spawn(|| eval_code("let f = |n| -> f (n + 1); in f 0"))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(out, "<poison>");
    }

    #[test]
    fn test_moderate_recursion_still_works() {
        let out = std::thread::Builder::new()
            .stack_size(INTERPRETER_STACK_SIZE)
            .spawn(|| {
                eval_code("let f = |n| -> if n == 0 then 0 else f (n - 1); in f 1000")
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(out, "0");
    }

    /// A list nested `depth` levels deep, built bottom-up without recursion.
    fn nested_list(depth: usize) -> Thunk {
        let mut t = Thunk::evaluated(Value::List(vec![]));
        for _ in 0..depth {
            t = Thunk::evaluated(Value::List(vec![t]));
        }
        t
    }

    #[test]
    fn test_dropping_deeply_nested_value_does_not_overflow() {
        // Runs on the default (small) test-thread stack on purpose.
        drop(nested_list(1_000_000));

        let mut t = Thunk::evaluated(Value::List(vec![]));
        for i in 0..200_000 {
            let mut m = std::collections::HashMap::new();
            m.insert(format!("k{}", i), t);
            t = Thunk::evaluated(Value::AttrSet(m));
        }
        drop(t);
    }

    #[test]
    fn test_debug_of_deeply_nested_value_is_truncated() {
        let out = std::thread::Builder::new()
            .stack_size(INTERPRETER_STACK_SIZE)
            .spawn(|| format!("{:?}", nested_list(1_000_000)))
            .unwrap()
            .join()
            .unwrap();
        assert!(out.contains("..."));
    }

    #[test]
    fn test_deep_force_rejects_deeply_nested_value() {
        let err = std::thread::Builder::new()
            .stack_size(INTERPRETER_STACK_SIZE)
            .spawn(|| {
                let t = nested_list(1_000_000);
                let v = evaluate(t).unwrap();
                deep_force(v).err()
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(err.unwrap().contains("nested too deeply"));
    }

    #[test]
    fn test_deep_force_accepts_moderately_nested_value() {
        let out = std::thread::Builder::new()
            .stack_size(INTERPRETER_STACK_SIZE)
            .spawn(|| deep_force(evaluate(nested_list(1_000)).unwrap()).is_ok())
            .unwrap()
            .join()
            .unwrap();
        assert!(out);
    }

    #[test]
    fn test_visible_names_walks_enclosing_scopes_without_duplicates() {
        let outer = Env::new(std::rc::Rc::new(String::new()), std::rc::Rc::new(String::new()));
        outer.define("a".to_string(), Thunk::evaluated(Value::Int(1)));
        outer.define("shadowed".to_string(), Thunk::evaluated(Value::Int(1)));
        let inner = outer.extend();
        inner.define("b".to_string(), Thunk::evaluated(Value::Int(2)));
        inner.define("shadowed".to_string(), Thunk::evaluated(Value::Int(2)));

        // Innermost scope first, each scope's names sorted, shadowed names once.
        assert_eq!(inner.visible_names(), ["b", "shadowed", "a"]);
        assert_eq!(outer.visible_names(), ["a", "shadowed"]);
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
