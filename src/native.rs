use crate::ast::Expr;
use crate::eval::evaluate;
use crate::value::{Env, Thunk, Value};
use chumsky::Parser;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub fn native_fn<F>(f: F) -> Value
where
    F: Fn(Value) -> Result<Value, String> + 'static,
{
    Value::NativeClosure(Rc::new(f))
}

fn serialize_value(val: Value) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    match val {
        Value::String(s) => {
            let bytes = s.as_bytes();
            out.extend_from_slice(format!("S:{}:", bytes.len()).as_bytes());
            out.extend_from_slice(bytes);
        }
        Value::Path(p) => {
            let bytes = p.as_bytes();
            out.extend_from_slice(format!("P:{}:", bytes.len()).as_bytes());
            out.extend_from_slice(bytes);
        }
        Value::Int(i) => {
            out.extend_from_slice(format!("I:{};", i).as_bytes());
        }
        Value::Float(f) => {
            out.extend_from_slice(format!("F:{};", f).as_bytes());
        }
        Value::Bool(b) => {
            out.extend_from_slice(if b { b"B:1;" } else { b"B:0;" });
        }
        Value::List(thunks) => {
            out.extend_from_slice(format!("L:{}:", thunks.len()).as_bytes());
            for t in thunks {
                let v = evaluate(t)?;
                out.extend_from_slice(&serialize_value(v)?);
            }
        }
        Value::AttrSet(map) => {
            out.extend_from_slice(format!("A:{}:", map.len()).as_bytes());
            let mut keys: Vec<&String> = map.keys().collect();
            // Sort by key to guarantee stable hash
            keys.sort();
            for k in keys {
                let k_bytes = k.as_bytes();
                out.extend_from_slice(format!("S:{}:", k_bytes.len()).as_bytes());
                out.extend_from_slice(k_bytes);
                let v = evaluate(map.get(k).unwrap().clone())?;
                out.extend_from_slice(&serialize_value(v)?);
            }
        }
        Value::Closure { .. } | Value::NativeClosure(_) => {
            return Err("unhashable type: Function".into());
        }
    }
    Ok(out)
}

pub fn build_native_env() -> Value {
    let mut map = HashMap::new();
    let native_ref = Rc::new(RefCell::new(None));

    // -- abort --
    map.insert(
        "abort".to_string(),
        Thunk::evaluated(native_fn(|v| {
            let code = match v {
                Value::Int(i) => i as i32,
                _ => 1,
            };
            std::process::exit(code);
        })),
    );

    // -- import --
    let native_ref_clone = native_ref.clone();

    // Cache for standard library modules to prevent parsing deep in the stack
    let mut stdlib_cache = HashMap::new();

    let preload_module = |name: &str, source: &str| -> Result<Value, String> {
        let (opt_ast, errs) = crate::parser::parser()
            .then_ignore(chumsky::prelude::end())
            .parse(source)
            .into_output_errors();

        if !errs.is_empty() {
            use ariadne::{Color, Label, Report, ReportKind, Source};
            for err in errs {
                Report::build(
                    ReportKind::Error,
                    (
                        name.to_string(),
                        err.span().into_range().start..err.span().into_range().start,
                    ),
                )
                .with_message(err.to_string())
                .with_label(
                    Label::new((name.to_string(), err.span().into_range()))
                        .with_message(err.reason().to_string())
                        .with_color(Color::Red),
                )
                .finish()
                .eprint((name.to_string(), Source::from(source)))
                .unwrap();
            }
        }

        let expr = opt_ast.ok_or_else(|| format!("Parse error in module {}", name))?;
        let env = Env::new(
            std::rc::Rc::new(source.to_string()),
            std::rc::Rc::new(name.to_string()),
        );
        let thunk = Thunk::new(expr, env);
        Ok(evaluate(thunk)?)
    };

    stdlib_cache.insert(
        "fmt".to_string(),
        preload_module("fmt", include_str!("fmt.eris")).unwrap(),
    );
    stdlib_cache.insert(
        "list".to_string(),
        preload_module("list", include_str!("list.eris")).unwrap(),
    );
    stdlib_cache.insert(
        "string".to_string(),
        preload_module("string", include_str!("string.eris")).unwrap(),
    );
    stdlib_cache.insert(
        "attr".to_string(),
        preload_module("attr", include_str!("attr.eris")).unwrap(),
    );
    stdlib_cache.insert(
        "fs".to_string(),
        preload_module("fs", include_str!("fs.eris")).unwrap(),
    );
    stdlib_cache.insert(
        "json".to_string(),
        preload_module("json", include_str!("json.eris")).unwrap(),
    );
    stdlib_cache.insert(
        "path".to_string(),
        preload_module("path", include_str!("path.eris")).unwrap(),
    );
    stdlib_cache.insert(
        "child_process".to_string(),
        preload_module("child_process", include_str!("child_process.eris")).unwrap(),
    );
    stdlib_cache.insert(
        "hash".to_string(),
        preload_module("hash", include_str!("hash.eris")).unwrap(),
    );

    map.insert(
        "import".to_string(),
        Thunk::evaluated(native_fn(move |v| {
            let module_name = match v {
                Value::String(s) => s,
                _ => return Err(format!("import expects a string, got {:?}", v)),
            };

            let result_val = if let Some(cached_val) = stdlib_cache.get(&module_name) {
                cached_val.clone()
            } else {
                let source = std::fs::read_to_string(&module_name)
                    .map_err(|e| format!("Error loading module {}: {}", module_name, e))?;

                let (opt_ast, errs) = crate::parser::parser()
                    .then_ignore(chumsky::prelude::end())
                    .parse(source.as_str())
                    .into_output_errors();

                if !errs.is_empty() {
                    use ariadne::{Color, Label, Report, ReportKind, Source};
                    for err in errs {
                        Report::build(
                            ReportKind::Error,
                            (
                                module_name.clone(),
                                err.span().into_range().start..err.span().into_range().start,
                            ),
                        )
                        .with_message(err.to_string())
                        .with_label(
                            Label::new((module_name.clone(), err.span().into_range()))
                                .with_message(err.reason().to_string())
                                .with_color(Color::Red),
                        )
                        .finish()
                        .eprint((module_name.clone(), Source::from(&source)))
                        .unwrap();
                    }
                }

                let expr =
                    opt_ast.ok_or_else(|| format!("Parse error in module {}", module_name))?;

                let env = Env::new(
                    std::rc::Rc::new(source.to_string()),
                    std::rc::Rc::new(module_name.to_string()),
                );
                let thunk = Thunk::new(expr, env);
                evaluate(thunk)?
            };

            if let Value::Closure {
                args,
                body,
                env: closure_env,
            } = result_val
            {
                let call_env = closure_env.extend();
                if let crate::ast::Args::Destructure { names, .. } = args {
                    for name in names {
                        if name == "__native" {
                            let nv = native_ref_clone.borrow().clone().unwrap();
                            call_env.define(name, Thunk::evaluated(nv));
                        }
                    }
                }
                crate::eval::eval_expr(&body, &call_env)
            } else {
                Ok(result_val)
            }
        })),
    );

    // -- fmt --
    map.insert(
        "fmt_println".to_string(),
        Thunk::evaluated(native_fn(|v| {
            match v {
                Value::String(s) => println!("{}", s),
                v => println!("{:?}", v),
            }
            Ok(Value::Int(0))
        })),
    );
    map.insert(
        "fmt_eprintln".to_string(),
        Thunk::evaluated(native_fn(|v| {
            match v {
                Value::String(s) => eprintln!("{}", s),
                v => eprintln!("{:?}", v),
            }
            Ok(Value::Int(0))
        })),
    );
    map.insert(
        "fmt_print".to_string(),
        Thunk::evaluated(native_fn(|v| {
            use std::io::Write;
            match v {
                Value::String(s) => print!("{}", s),
                v => print!("{:?}", v),
            }
            let _ = std::io::stdout().flush();
            Ok(Value::Int(0))
        })),
    );
    map.insert(
        "fmt_eprint".to_string(),
        Thunk::evaluated(native_fn(|v| {
            use std::io::Write;
            match v {
                Value::String(s) => eprint!("{}", s),
                v => eprint!("{:?}", v),
            }
            let _ = std::io::stderr().flush();
            Ok(Value::Int(0))
        })),
    );
    map.insert(
        "fmt_printf".to_string(),
        Thunk::evaluated(native_fn(|v| {
            // Simple mock for now, printf would need parsing. We just print.
            match v {
                Value::String(s) => print!("{}", s),
                v => print!("{:?}", v),
            }
            Ok(Value::Int(0))
        })),
    );
    map.insert(
        "fmt_eprintf".to_string(),
        Thunk::evaluated(native_fn(|v| {
            match v {
                Value::String(s) => eprint!("{}", s),
                v => eprint!("{:?}", v),
            }
            Ok(Value::Int(0))
        })),
    );

    // -- list --
    map.insert(
        "list_map".to_string(),
        Thunk::evaluated(native_fn(|func| {
            Ok(native_fn(move |list| {
                if let Value::List(thunks) = list {
                    let mut res = Vec::new();
                    for t in thunks {
                        let arg = evaluate(t)?;
                        let apply_env = Env::new();
                        // To apply `func` (which is a Value) to `arg` (which is a Value),
                        // we can construct an App expr or manually evaluate if it's a closure.
                        // Doing App expr is easiest:
                        let app_expr = Expr::App(
                            Box::new(Expr::Ident("f".to_string())),
                            Box::new(Expr::Ident("x".to_string())),
                        );
                        apply_env.define("f".to_string(), Thunk::evaluated(func.clone()));
                        apply_env.define("x".to_string(), Thunk::evaluated(arg));
                        let mapped = crate::eval::eval_expr(&app_expr, &apply_env)?;
                        res.push(Thunk::evaluated(mapped));
                    }
                    Ok(Value::List(res))
                } else {
                    Err("list.map expects a list".into())
                }
            }))
        })),
    );
    map.insert(
        "list_filter".to_string(),
        Thunk::evaluated(native_fn(|func| {
            Ok(native_fn(move |list| {
                if let Value::List(thunks) = list {
                    let mut res = Vec::new();
                    for t in thunks {
                        let arg = evaluate(t.clone())?;
                        let apply_env = Env::new();
                        let app_expr = Expr::App(
                            Box::new(Expr::Ident("f".to_string())),
                            Box::new(Expr::Ident("x".to_string())),
                        );
                        apply_env.define("f".to_string(), Thunk::evaluated(func.clone()));
                        apply_env.define("x".to_string(), Thunk::evaluated(arg));
                        let is_match = crate::eval::eval_expr(&app_expr, &apply_env)?;
                        if let Value::Bool(true) = is_match {
                            res.push(t);
                        }
                    }
                    Ok(Value::List(res))
                } else {
                    Err("list.filter expects a list".into())
                }
            }))
        })),
    );
    map.insert(
        "list_foldl".to_string(),
        Thunk::evaluated(native_fn(|func| {
            Ok(native_fn(move |init| {
                let func_clone = func.clone();
                Ok(native_fn(move |list| {
                    if let Value::List(thunks) = list {
                        let mut acc = init.clone();
                        for t in thunks {
                            let arg = evaluate(t)?;
                            let apply_env = Env::new();
                            let app1 = Expr::App(
                                Box::new(Expr::Ident("f".to_string())),
                                Box::new(Expr::Ident("acc".to_string())),
                            );
                            let app2 =
                                Expr::App(Box::new(app1), Box::new(Expr::Ident("x".to_string())));
                            apply_env.define("f".to_string(), Thunk::evaluated(func_clone.clone()));
                            apply_env.define("acc".to_string(), Thunk::evaluated(acc));
                            apply_env.define("x".to_string(), Thunk::evaluated(arg));
                            acc = crate::eval::eval_expr(&app2, &apply_env)?;
                        }
                        Ok(acc)
                    } else {
                        Err("list.foldl expects a list".into())
                    }
                }))
            }))
        })),
    );
    map.insert(
        "list_head".to_string(),
        Thunk::evaluated(native_fn(|v| {
            if let Value::List(thunks) = v {
                if thunks.is_empty() {
                    Err("head on empty list".into())
                } else {
                    evaluate(thunks[0].clone())
                }
            } else {
                Err("list.head expects a list".into())
            }
        })),
    );
    map.insert(
        "list_tail".to_string(),
        Thunk::evaluated(native_fn(|v| {
            if let Value::List(thunks) = v {
                if thunks.is_empty() {
                    Err("tail on empty list".into())
                } else {
                    Ok(Value::List(thunks[1..].to_vec()))
                }
            } else {
                Err("list.tail expects a list".into())
            }
        })),
    );

    // -- string --
    map.insert(
        "string_concat".to_string(),
        Thunk::evaluated(native_fn(|s1| {
            Ok(native_fn(move |s2| match (&s1, &s2) {
                (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
                _ => Err("string.concat expects two strings".into()),
            }))
        })),
    );
    map.insert(
        "string_split".to_string(),
        Thunk::evaluated(native_fn(|sep| {
            Ok(native_fn(move |s| match (&sep, &s) {
                (Value::String(a), Value::String(b)) => {
                    let parts: Vec<Thunk> = b
                        .split(a)
                        .map(|part| Thunk::evaluated(Value::String(part.to_string())))
                        .collect();
                    Ok(Value::List(parts))
                }
                _ => Err("string.split expects two strings".into()),
            }))
        })),
    );
    map.insert(
        "string_trim".to_string(),
        Thunk::evaluated(native_fn(|s| match s {
            Value::String(a) => Ok(Value::String(a.trim().to_string())),
            _ => Err("string.trim expects a string".into()),
        })),
    );
    map.insert(
        "string_interpolate".to_string(),
        Thunk::evaluated(native_fn(|s| {
            // interpolation is already a language feature, this might just pass through or we can format.
            Ok(s)
        })),
    );

    // -- attr --
    map.insert(
        "attr_names".to_string(),
        Thunk::evaluated(native_fn(|v| {
            if let Value::AttrSet(map) = v {
                let mut keys: Vec<String> = map.keys().cloned().collect();
                keys.sort();
                let thunks = keys
                    .into_iter()
                    .map(|k| Thunk::evaluated(Value::String(k)))
                    .collect();
                Ok(Value::List(thunks))
            } else {
                Err("attr.names expects an attrset".into())
            }
        })),
    );
    map.insert(
        "attr_values".to_string(),
        Thunk::evaluated(native_fn(|v| {
            if let Value::AttrSet(map) = v {
                let mut keys: Vec<String> = map.keys().cloned().collect();
                keys.sort();
                let thunks = keys
                    .into_iter()
                    .map(|k| map.get(&k).unwrap().clone())
                    .collect();
                Ok(Value::List(thunks))
            } else {
                Err("attr.values expects an attrset".into())
            }
        })),
    );
    map.insert(
        "attr_has".to_string(),
        Thunk::evaluated(native_fn(|name| {
            Ok(native_fn(move |v| {
                if let (Value::String(k), Value::AttrSet(map)) = (&name, &v) {
                    Ok(Value::Bool(map.contains_key(k)))
                } else {
                    Err("attr.has expects a string and an attrset".into())
                }
            }))
        })),
    );
    map.insert(
        "attr_get".to_string(),
        Thunk::evaluated(native_fn(|name| {
            Ok(native_fn(move |v| {
                if let (Value::String(k), Value::AttrSet(map)) = (&name, &v) {
                    if let Some(thunk) = map.get(k) {
                        evaluate(thunk.clone())
                    } else {
                        Err(format!("attribute {} not found", k))
                    }
                } else {
                    Err("attr.get expects a string and an attrset".into())
                }
            }))
        })),
    );
    map.insert(
        "attr_merge".to_string(),
        Thunk::evaluated(native_fn(|a| {
            Ok(native_fn(move |b| {
                if let (Value::AttrSet(map1), Value::AttrSet(map2)) = (&a, &b) {
                    let mut map = map1.clone();
                    for (k, v) in map2 {
                        map.insert(k.clone(), v.clone());
                    }
                    Ok(Value::AttrSet(map))
                } else {
                    Err("attr.merge expects two attrsets".into())
                }
            }))
        })),
    );

    // -- fs --
    map.insert(
        "fs_read_file".to_string(),
        Thunk::evaluated(native_fn(|v| {
            let path = match v {
                Value::String(s) => s,
                Value::Path(p) => p,
                _ => return Err("fs.read_file expects a string or path".into()),
            };
            match std::fs::read_to_string(&path) {
                Ok(s) => Ok(Value::String(s)),
                Err(e) => Err(format!("failed to read file {}: {}", path, e)),
            }
        })),
    );
    map.insert(
        "fs_read_dir".to_string(),
        Thunk::evaluated(native_fn(|v| {
            let path = match v {
                Value::String(s) => s,
                Value::Path(p) => p,
                _ => return Err("fs.read_dir expects a string or path".into()),
            };
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    let mut thunks = Vec::new();
                    for entry in entries {
                        if let Ok(e) = entry {
                            if let Ok(name) = e.file_name().into_string() {
                                thunks.push(Thunk::evaluated(Value::String(name)));
                            }
                        }
                    }
                    Ok(Value::List(thunks))
                }
                Err(e) => Err(format!("failed to read directory {}: {}", path, e)),
            }
        })),
    );
    map.insert(
        "fs_write_file".to_string(),
        Thunk::evaluated(native_fn(|path_val| {
            Ok(native_fn(move |content_val| {
                let path = match &path_val {
                    Value::String(s) => s,
                    Value::Path(p) => p,
                    _ => return Err("fs.write_file expects a string or path".into()),
                };
                let content = match &content_val {
                    Value::String(s) => s,
                    _ => return Err("fs.write_file expects a string content".into()),
                };
                match std::fs::write(path, content) {
                    Ok(_) => Ok(Value::Int(0)),
                    Err(e) => Err(format!("failed to write file {}: {}", path, e)),
                }
            }))
        })),
    );

    // -- json --
    map.insert(
        "json_to".to_string(),
        Thunk::evaluated(native_fn(|v| {
            fn val_to_json(val: Value) -> Result<serde_json::Value, String> {
                match val {
                    Value::Int(i) => Ok(serde_json::Value::Number(i.into())),
                    Value::Float(f) => {
                        if let Some(n) = serde_json::Number::from_f64(f) {
                            Ok(serde_json::Value::Number(n))
                        } else {
                            Err("invalid float for JSON".into())
                        }
                    }
                    Value::Bool(b) => Ok(serde_json::Value::Bool(b)),
                    Value::String(s) => Ok(serde_json::Value::String(s)),
                    Value::Path(s) => Ok(serde_json::Value::String(s)),
                    Value::List(thunks) => {
                        let mut arr = Vec::new();
                        for t in thunks {
                            arr.push(val_to_json(evaluate(t)?)?);
                        }
                        Ok(serde_json::Value::Array(arr))
                    }
                    Value::AttrSet(map) => {
                        let mut obj = serde_json::Map::new();
                        for (k, thunk) in map {
                            obj.insert(k, val_to_json(evaluate(thunk)?)?);
                        }
                        Ok(serde_json::Value::Object(obj))
                    }
                    _ => Err("cannot convert closure/builtin to json".into()),
                }
            }
            let j = val_to_json(v)?;
            Ok(Value::String(j.to_string()))
        })),
    );
    map.insert(
        "json_from".to_string(),
        Thunk::evaluated(native_fn(|v| {
            let s = match v {
                Value::String(st) => st,
                _ => return Err("json.from expects a string".into()),
            };
            let j: serde_json::Value = serde_json::from_str(&s).map_err(|e| e.to_string())?;

            fn json_to_val(j: serde_json::Value) -> Result<Value, String> {
                match j {
                    serde_json::Value::Null => Ok(Value::AttrSet(HashMap::new())), // Eris has no null, fallback to {}
                    serde_json::Value::Bool(b) => Ok(Value::Bool(b)),
                    serde_json::Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            Ok(Value::Int(i))
                        } else if let Some(f) = n.as_f64() {
                            Ok(Value::Float(f))
                        } else {
                            Ok(Value::Int(0))
                        }
                    }
                    serde_json::Value::String(s) => Ok(Value::String(s)),
                    serde_json::Value::Array(arr) => {
                        let mut thunks = Vec::new();
                        for item in arr {
                            thunks.push(Thunk::evaluated(json_to_val(item)?));
                        }
                        Ok(Value::List(thunks))
                    }
                    serde_json::Value::Object(obj) => {
                        let mut map = HashMap::new();
                        for (k, v) in obj {
                            map.insert(k, Thunk::evaluated(json_to_val(v)?));
                        }
                        Ok(Value::AttrSet(map))
                    }
                }
            }
            json_to_val(j)
        })),
    );

    // -- path --
    map.insert(
        "path_join".to_string(),
        Thunk::evaluated(native_fn(|p1_val| {
            Ok(native_fn(move |p2_val| {
                let p1 = match &p1_val {
                    Value::String(s) => s.clone(),
                    Value::Path(p) => p.clone(),
                    _ => return Err("path.join expects strings/paths".into()),
                };
                let p2 = match &p2_val {
                    Value::String(s) => s.clone(),
                    Value::Path(p) => p.clone(),
                    _ => return Err("path.join expects strings/paths".into()),
                };
                let mut path = std::path::PathBuf::from(p1);
                path.push(p2);
                Ok(Value::String(path.to_string_lossy().into_owned()))
            }))
        })),
    );
    map.insert(
        "path_dirname".to_string(),
        Thunk::evaluated(native_fn(|p_val| {
            let p = match p_val {
                Value::String(s) => s,
                Value::Path(p) => p,
                _ => return Err("path.dirname expects a string/path".into()),
            };
            let path = std::path::Path::new(&p);
            if let Some(parent) = path.parent() {
                Ok(Value::String(parent.to_string_lossy().into_owned()))
            } else {
                Ok(Value::String("".to_string()))
            }
        })),
    );
    map.insert(
        "path_basename".to_string(),
        Thunk::evaluated(native_fn(|p_val| {
            let p = match p_val {
                Value::String(s) => s,
                Value::Path(p) => p,
                _ => return Err("path.basename expects a string/path".into()),
            };
            let path = std::path::Path::new(&p);
            if let Some(file_name) = path.file_name() {
                Ok(Value::String(file_name.to_string_lossy().into_owned()))
            } else {
                Ok(Value::String("".to_string()))
            }
        })),
    );
    map.insert(
        "path_extname".to_string(),
        Thunk::evaluated(native_fn(|p_val| {
            let p = match p_val {
                Value::String(s) => s,
                Value::Path(p) => p,
                _ => return Err("path.extname expects a string/path".into()),
            };
            let path = std::path::Path::new(&p);
            if let Some(ext) = path.extension() {
                Ok(Value::String(format!(".{}", ext.to_string_lossy())))
            } else {
                Ok(Value::String("".to_string()))
            }
        })),
    );

    // -- child_process --
    map.insert(
        "child_process_exec".to_string(),
        Thunk::evaluated(native_fn(|cmd_val| {
            Ok(native_fn(move |args_val| {
                let cmd = match &cmd_val {
                    Value::String(s) => s.clone(),
                    _ => return Err("child_process.exec expects a string command".into()),
                };
                let args = match &args_val {
                    Value::List(l) => {
                        let mut a = Vec::new();
                        for t in l {
                            match evaluate(t.clone())? {
                                Value::String(s) => a.push(s),
                                _ => return Err("child_process.exec args must be strings".into()),
                            }
                        }
                        a
                    }
                    _ => return Err("child_process.exec expects a list of arguments".into()),
                };

                let output = std::process::Command::new(cmd)
                    .args(args)
                    .output()
                    .map_err(|e| format!("Failed to execute command: {}", e))?;

                let mut res_map = HashMap::new();
                res_map.insert(
                    "status".to_string(),
                    Thunk::evaluated(Value::Int(output.status.code().unwrap_or(-1) as i64)),
                );
                let stdout_str = String::from_utf8_lossy(&output.stdout).into_owned();
                let stderr_str = String::from_utf8_lossy(&output.stderr).into_owned();
                res_map.insert(
                    "stdout".to_string(),
                    Thunk::evaluated(Value::String(stdout_str)),
                );
                res_map.insert(
                    "stderr".to_string(),
                    Thunk::evaluated(Value::String(stderr_str)),
                );

                Ok(Value::AttrSet(res_map))
            }))
        })),
    );

    // -- hash --
    map.insert(
        "hash_sha256".to_string(),
        Thunk::evaluated(native_fn(|v| {
            use base64::Engine;
            use sha2::{Digest, Sha256};
            let bytes = serialize_value(v)?;
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let result = hasher.finalize();
            Ok(Value::String(
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(result),
            ))
        })),
    );
    map.insert(
        "hash_sha512".to_string(),
        Thunk::evaluated(native_fn(|v| {
            use base64::Engine;
            use sha2::{Digest, Sha512};
            let bytes = serialize_value(v)?;
            let mut hasher = Sha512::new();
            hasher.update(&bytes);
            let result = hasher.finalize();
            Ok(Value::String(
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(result),
            ))
        })),
    );
    map.insert(
        "hash_blake3".to_string(),
        Thunk::evaluated(native_fn(|v| {
            use base64::Engine;
            let bytes = serialize_value(v)?;
            let hash = blake3::hash(&bytes);
            Ok(Value::String(
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash.as_bytes()),
            ))
        })),
    );

    let native_val = Value::AttrSet(map);
    *native_ref.borrow_mut() = Some(native_val.clone());

    native_val
}
