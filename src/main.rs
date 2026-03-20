pub mod ast;
pub mod eval;
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

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: eris <file.eris>");
        std::process::exit(1);
    }

    let filename = &args[1];
    let source = fs::read_to_string(filename).unwrap_or_else(|err| {
        eprintln!("Error reading file {}: {}", filename, err);
        std::process::exit(1);
    });

    let expr = match parser().parse(&source).into_result() {
        Ok(ast) => ast,
        Err(errs) => {
            for err in errs {
                eprintln!("Parse error: {:?}", err);
            }
            std::process::exit(1);
        }
    };

    let env = Env::new();
    let thunk = Thunk::new(expr, env);
    match evaluate(thunk).and_then(deep_force) {
        Ok(val) => println!("{:#?}", val),
        Err(e) => eprintln!("Runtime error: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval_code(source: &str) -> String {
        let ast = parser().parse(source).into_result().unwrap();
        let env = Env::new();
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
    }

    #[test]
    fn test_string_interpolation() {
        assert_eq!(eval_code("let name = \"Eris\"; in \"Hello, ${name}!\""), "\"Hello, Eris!\"");
    }

    #[test]
    fn test_attr_set() {
        assert_eq!(eval_code("let obj = { a = 1; b = 2; }; in obj.a"), "1");
        assert_eq!(eval_code("let obj = { inner = { a = 42; }; }; in obj.inner.a"), "42");
    }

    #[test]
    fn test_rec_attr_set() {
        assert_eq!(eval_code("let r = rec { a = b; b = 99; }; in r.a"), "99");
        // Deeper nested recursion referencing variables in scope
        assert_eq!(eval_code("let x = 5; r = rec { a = b + x; b = 10; }; in r.a"), "15");
    }

    #[test]
    fn test_lambda_currying() {
        assert_eq!(eval_code("let add = |a, b| -> a + b; in add 10 20"), "30");
        assert_eq!(eval_code("let succ = |n| -> n + 1; in succ 5"), "6");
        // Test partial application
        assert_eq!(eval_code("let add = |a, b| -> a + b; add5 = add 5; in add5 10"), "15");
    }

    #[test]
    fn test_destructuring_lambda() {
        assert_eq!(eval_code("let f = {x; y} -> x + y; in f { x = 10; y = 20; }"), "30");
        assert_eq!(eval_code("let f = {x; ...} -> x; in f { x = 10; y = 20; z = 30; }"), "10");
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
}
