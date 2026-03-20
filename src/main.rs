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
