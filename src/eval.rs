use crate::ast::*;
use crate::value::*;
use ariadne::{Color, Label, Report, ReportKind, Source};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static HAD_ERROR: AtomicBool = AtomicBool::new(false);

/// Whether any `report_error` call has fired since the last `reset_error_flag`.
/// Callers use this to decide the process exit code after a lazily-evaluated
/// program has finished running (errors are reported and evaluation limps on
/// with `Value::Poison` rather than aborting immediately).
pub fn had_error() -> bool {
    HAD_ERROR.load(Ordering::SeqCst)
}

pub fn reset_error_flag() {
    HAD_ERROR.store(false, Ordering::SeqCst);
}

static LENIENT: AtomicBool = AtomicBool::new(false);

/// In lenient mode only `::` type-annotation errors are reported (and counted);
/// every other error is swallowed. `eris check --level type` uses it while
/// forcing values the program never asked for, where, say, a division by zero in
/// dead code is none of the type checker's business.
pub fn set_lenient(lenient: bool) {
    LENIENT.store(lenient, Ordering::SeqCst);
}

pub fn is_lenient() -> bool {
    LENIENT.load(Ordering::SeqCst)
}

fn report_error(env: &Env, span: std::ops::Range<usize>, msg: &str, hint: Option<&str>, note: Option<&str>) {
    if LENIENT.load(Ordering::SeqCst) {
        return;
    }
    report_type_error(env, span, msg, hint, note);
}

/// Like `report_error`, but also reported in lenient mode.
fn report_type_error(env: &Env, span: std::ops::Range<usize>, msg: &str, hint: Option<&str>, note: Option<&str>) {
    HAD_ERROR.store(true, Ordering::SeqCst);
    let mut builder = Report::build(ReportKind::Error, (env.filename.to_string(), span.clone()))
        .with_message(msg)
        .with_label(
            Label::new((env.filename.to_string(), span.clone()))
                .with_message(msg)
                .with_color(Color::Red),
        );

    if let Some(hint_msg) = hint {
        builder = builder.with_label(
            Label::new((env.filename.to_string(), span))
                .with_message(format!("did you mean '{}'?", hint_msg))
                .with_color(Color::Yellow),
        );
    }

    if let Some(note_msg) = note {
        builder = builder.with_note(note_msg);
    }

    builder
        .finish()
        .eprint((env.filename.to_string(), Source::from(env.source.as_str())))
        .unwrap();
}

pub fn levenshtein_distance(a: &str, b: &str) -> usize {
    let len_a = a.chars().count();
    let len_b = b.chars().count();
    let mut matrix = vec![vec![0; len_b + 1]; len_a + 1];
    for i in 0..=len_a {
        matrix[i][0] = i;
    }
    for j in 0..=len_b {
        matrix[0][j] = j;
    }
    for (i, ca) in a.chars().enumerate() {
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            matrix[i + 1][j + 1] = (matrix[i][j + 1] + 1)
                .min(matrix[i + 1][j] + 1)
                .min(matrix[i][j] + cost);
        }
    }
    matrix[len_a][len_b]
}

pub fn did_you_mean<'a>(
    target: &str,
    candidates: impl Iterator<Item = &'a String>,
) -> Option<&'a String> {
    let mut best = None;
    let mut best_dist = usize::MAX;
    for cand in candidates {
        if cand == target {
            continue;
        }
        let dist = levenshtein_distance(target, cand);
        if dist <= 3 && dist < best_dist {
            best_dist = dist;
            best = Some(cand);
        }
    }
    best
}

fn value_type_name(val: &Value) -> &'static str {
    match val {
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Bool(_) => "bool",
        Value::String(_) => "string",
        Value::Path(_) => "path",
        Value::List(_) => "list",
        Value::AttrSet(_) => "attrset",
        Value::Closure { .. } | Value::NativeClosure(_) => "closure",
        Value::Poison => "poison",
    }
}

/// Maps a type name written after `::` to the canonical type it asserts.
/// Sized aliases like `int32`/`float64` all collapse onto the one runtime
/// representation eris actually has (`Value::Int`/`Value::Float`); the width
/// itself isn't checked.
fn resolve_type_name(name: &str) -> Option<&'static str> {
    match name {
        "int" | "int8" | "int16" | "int32" | "int64" => Some("int"),
        "float" | "float32" | "float64" | "double" => Some("float"),
        "bool" | "boolean" => Some("bool"),
        "string" | "str" => Some("string"),
        "path" => Some("path"),
        "list" => Some("list"),
        "attrset" | "set" => Some("attrset"),
        "closure" | "function" | "fn" => Some("closure"),
        _ => None,
    }
}

pub fn evaluate(thunk: Thunk) -> Result<Value, String> {
    {
        let b = thunk.0.borrow();
        match &*b {
            ThunkState::Evaluated(val) => return Ok(val.clone()),
            ThunkState::Evaluating => return Err("Infinite recursion detected".to_string()),
            ThunkState::Unevaluated { .. } => {}
        }
    }

    let (expr, env) = match thunk.0.replace(ThunkState::Evaluating) {
        ThunkState::Unevaluated { expr, env } => (expr, env),
        _ => unreachable!(),
    };

    let val = eval_expr(&expr, &env)?;
    *thunk.0.borrow_mut() = ThunkState::Evaluated(val.clone());
    Ok(val)
}

/// Default maximum nesting of `eval_expr` calls before evaluation is aborted
/// with an error. Eris has no tail-call elimination, so deep (or infinite)
/// recursion would otherwise overflow the native stack and abort the process.
pub const DEFAULT_MAX_EVAL_DEPTH: usize = 10_000;

/// Stack bytes budgeted per `eval_expr` nesting level, with ~2x headroom over
/// what was measured (about 30KB per level unoptimised, 2.6KB optimised).
pub const STACK_BYTES_PER_DEPTH: usize = if cfg!(debug_assertions) { 48 * 1024 } else { 6 * 1024 };

static MAX_EVAL_DEPTH: AtomicUsize = AtomicUsize::new(DEFAULT_MAX_EVAL_DEPTH);

/// Sizes the depth limit to the stack of the thread that will evaluate, so a
/// smaller-than-intended stack yields a clean error instead of an overflow.
pub fn set_max_depth_for_stack(stack_bytes: usize) {
    let depth = (stack_bytes / STACK_BYTES_PER_DEPTH).clamp(1, DEFAULT_MAX_EVAL_DEPTH);
    MAX_EVAL_DEPTH.store(depth, Ordering::SeqCst);
}

/// The current nesting limit, shared by `eval_expr` and by every function that
/// recurses over the *shape* of a value (forcing, hashing, JSON, printing).
pub fn max_depth() -> usize {
    MAX_EVAL_DEPTH.load(Ordering::Relaxed)
}

/// Guards recursion over deeply nested data. Values are built lazily, so a
/// list nested a million levels deep costs nothing to create but would overflow
/// the native stack the moment something walks it recursively.
pub fn check_data_depth(depth: usize) -> Result<(), String> {
    if depth > max_depth() {
        Err("Recursion limit exceeded: value is nested too deeply".to_string())
    } else {
        Ok(())
    }
}

thread_local! {
    static EVAL_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Decrements the depth counter when an `eval_expr` frame exits, including on
/// early `?` returns.
struct DepthGuard;

impl Drop for DepthGuard {
    fn drop(&mut self) {
        EVAL_DEPTH.with(|d| d.set(d.get() - 1));
    }
}

pub fn eval_expr(expr: &Expr, env: &Env) -> Result<Value, String> {
    let depth = EVAL_DEPTH.with(|d| {
        d.set(d.get() + 1);
        d.get()
    });
    let _guard = DepthGuard;
    if depth > MAX_EVAL_DEPTH.load(Ordering::Relaxed) {
        report_error(
            env,
            expr.span.clone(),
            "Recursion limit exceeded",
            None,
            Some("evaluation nested too deeply; check for unbounded recursion (eris has no tail-call optimisation)"),
        );
        return Ok(Value::Poison);
    }
    eval_expr_inner(expr, env)
}

/// Applies `func` to the (possibly still lazy) argument `arg`.
///
/// `env` and `span` are only used to attribute diagnostics (a non-function being
/// called, a failed destructuring) to the call site.
pub fn apply_value(func: Value, arg: Thunk, env: &Env, span: &Span) -> Result<Value, String> {
    if let Value::Poison = func {
        return Ok(Value::Poison);
    }
    match func {
        Value::NativeClosure(f) => {
            let arg_val = evaluate(arg)?;
            if let Value::Poison = arg_val {
                return Ok(Value::Poison);
            }
            f(arg_val)
        }
        Value::Closure {
            args,
            body,
            env: closure_env,
        } => {
            let call_env = closure_env.extend();
            match args {
                Args::Positional(names) => {
                    call_env.define(names[0].clone(), arg);

                    if names.len() == 1 {
                        eval_expr(&body, &call_env)
                    } else {
                        let remaining_args = Args::Positional(names[1..].to_vec());
                        Ok(Value::Closure {
                            args: remaining_args,
                            body,
                            env: call_env,
                        })
                    }
                }
                Args::Destructure { names, ignore_rest } => {
                    let arg_val = evaluate(arg)?;
                    if let Value::Poison = arg_val {
                        return Ok(Value::Poison);
                    }
                    match arg_val {
                        Value::AttrSet(mut map) => {
                            for name in names {
                                let val_thunk = map.remove(&name).unwrap_or_else(|| {
                                    report_error(env, span.clone(), &format!("Missing required attribute '{}' in destructuring", name), None, None);
                                    Thunk::evaluated(Value::Poison)
                                });
                                call_env.define(name, val_thunk);
                            }
                            if !ignore_rest && !map.is_empty() {
                                report_error(
                                    env,
                                    span.clone(),
                                    &format!(
                                        "Unexpected attributes in destructuring: {:?}",
                                        map.keys()
                                    ),
                                    None,
                                    None,
                                );
                                return Ok(Value::Poison);
                            }
                        }
                        _ => {
                            report_error(
                                env,
                                span.clone(),
                                "Expected an attribute set for destructuring",
                                None,
                                None,
                            );
                            return Ok(Value::Poison);
                        }
                    }
                    eval_expr(&body, &call_env)
                }
            }
        }
        _ => {
            report_error(
                env,
                span.clone(),
                &format!("Not a function: {:?}", func),
                None,
                None,
            );
            Ok(Value::Poison)
        }
    }
}

/// Applies `func` to an already-evaluated `arg` from native code, where there is
/// no call site in any source file to blame diagnostics on.
pub fn apply(func: &Value, arg: Value) -> Result<Value, String> {
    let env = Env::new(
        std::rc::Rc::new(String::new()),
        std::rc::Rc::new(String::new()),
    );
    apply_value(func.clone(), Thunk::evaluated(arg), &env, &(0..0))
}

fn eval_expr_inner(expr: &Expr, env: &Env) -> Result<Value, String> {
    match &expr.kind {
        ExprKind::Bool(b) => Ok(Value::Bool(*b)),
        ExprKind::IfElse(cond, true_branch, false_branch) => {
            let cond_val = eval_expr(cond, env)?;
            if let Value::Poison = cond_val {
                return Ok(Value::Poison);
            }
            match cond_val {
                Value::Bool(true) => eval_expr(true_branch, env),
                Value::Bool(false) => eval_expr(false_branch, env),
                _ => {
                    report_error(
                        env,
                        cond.span.clone(),
                        &format!(
                            "Condition in if expression must be a boolean, got {:?}",
                            cond_val
                        ),
                        None,
                        None,
                    );
                    Ok(Value::Poison)
                }
            }
        }
        ExprKind::TypeAnnotation(inner, type_name) => {
            let val = eval_expr(inner, env)?;
            if let Value::Poison = val {
                return Ok(Value::Poison);
            }
            match resolve_type_name(type_name) {
                None => {
                    let known = [
                        "int", "float", "bool", "string", "path", "list", "attrset", "closure",
                    ];
                    let known_strings: Vec<String> = known.iter().map(|s| s.to_string()).collect();
                    let hint = did_you_mean(type_name, known_strings.iter());
                    report_type_error(
                        env,
                        expr.span.clone(),
                        &format!("Unknown type '{}'", type_name),
                        hint.map(|s| s.as_str()),
                        None,
                    );
                    Ok(Value::Poison)
                }
                Some(expected) => {
                    let actual = value_type_name(&val);
                    if actual == expected {
                        Ok(val)
                    } else {
                        report_type_error(
                            env,
                            expr.span.clone(),
                            &format!(
                                "Type mismatch: expected `{}`, got {:?} (a `{}`)",
                                type_name, val, actual
                            ),
                            None,
                            None,
                        );
                        Ok(Value::Poison)
                    }
                }
            }
        }
        ExprKind::Int(i) => Ok(Value::Int(*i)),
        ExprKind::Float(f) => Ok(Value::Float(*f)),
        ExprKind::String(parts) => {
            let mut result = String::new();
            for part in parts {
                match part {
                    StringPart::Literal(s) => result.push_str(s),
                    StringPart::Interpolation(ident) => {
                        let Some(thunk) = env.get(ident) else {
                            let candidates = env.visible_names();
                            let hint = did_you_mean(ident, candidates.iter());
                            report_error(
                                env,
                                expr.span.clone(),
                                &format!("Variable '{}' not found", ident),
                                hint.map(|s| s.as_str()),
                                None,
                            );
                            return Ok(Value::Poison);
                        };

                        let val = evaluate(thunk)?;
                        if let Value::Poison = val {
                            // If it's poison, interpolation fails, just return Poison
                            return Ok(Value::Poison);
                        }
                        match val {
                            Value::Int(i) => result.push_str(&i.to_string()),
                            Value::Float(f) => result.push_str(&f.to_string()),
                            Value::String(s) => result.push_str(&s),
                            _ => {
                                report_error(
                                    env,
                                    expr.span.clone(),
                                    &format!("Cannot interpolate {:?}", val),
                                    None,
                                    None,
                                );
                                return Ok(Value::Poison);
                            }
                        }
                    }
                }
            }
            Ok(Value::String(result))
        }
        ExprKind::Path(p) => Ok(Value::Path(p.clone())),
        ExprKind::List(exprs) => {
            let thunks = exprs
                .iter()
                .map(|e| Thunk::new(e.clone(), env.clone()))
                .collect();
            Ok(Value::List(thunks))
        }
        ExprKind::AttrSet { is_rec, attrs } => {
            let mut map = HashMap::new();
            let new_env = if *is_rec { env.extend() } else { env.clone() };

            for (k, v) in attrs {
                let thunk = Thunk::new(v.clone(), new_env.clone());
                map.insert(k.clone(), thunk.clone());
                if *is_rec {
                    new_env.define(k.clone(), thunk);
                }
            }
            Ok(Value::AttrSet(map))
        }
        ExprKind::Ident(name) => {
            if let Some(thunk) = env.get(name) {
                evaluate(thunk)
            } else {
                let candidates = env.visible_names();
                let hint = did_you_mean(name, candidates.iter());
                let note = if name == "builtins" {
                    Some("consider receiving 'builtins' as a function argument, e.g., `{ builtins } ->`")
                } else {
                    None
                };
                report_error(
                    env,
                    expr.span.clone(),
                    &format!("Variable '{}' not found", name),
                    hint.map(|s| s.as_str()),
                    note,
                );
                Ok(Value::Poison)
            }
        }
        ExprKind::FieldAccess(lhs, fields) => {
            let mut current = eval_expr(lhs, env)?;
            for field in fields {
                if let Value::Poison = current {
                    return Ok(Value::Poison);
                }
                match current {
                    Value::AttrSet(mut map) => {
                        let thunk = if let Some(t) = map.remove(field) {
                            t
                        } else {
                            // HashMap order differs from run to run; sort so equally
                            // close candidates always give the same suggestion.
                            let mut keys: Vec<String> = map.keys().cloned().collect();
                            keys.sort();
                            let hint = did_you_mean(field, keys.iter());
                            report_error(
                                env,
                                expr.span.clone(),
                                &format!("Field '{}' not found in attribute set", field),
                                hint.map(|s| s.as_str()),
                                None,
                            );
                            Thunk::evaluated(Value::Poison)
                        };
                        current = evaluate(thunk)?;
                    }
                    _ => {
                        report_error(
                            env,
                            expr.span.clone(),
                            &format!(
                                "Cannot access field '{}' on non-attribute set {:?}",
                                field, current
                            ),
                            None,
                            None,
                        );
                        return Ok(Value::Poison);
                    }
                }
            }
            Ok(current)
        }
        ExprKind::Lambda(args, body) => Ok(Value::Closure {
            args: args.clone(),
            body: *body.clone(),
            env: env.clone(),
        }),
        ExprKind::With(obj, body) => {
            let obj_thunk = Thunk::new(*obj.clone(), env.clone());
            let new_env = env.with_context(obj_thunk);
            eval_expr(body, &new_env)
        }
        ExprKind::ImplicitAccess(fields) => {
            let mut current_env = Some(env.clone());
            let mut found_with = None;
            while let Some(e) = current_env {
                if let Some(ctx) = &e.with_context {
                    found_with = Some(ctx.clone());
                    break;
                }
                current_env = e.parent.as_deref().cloned();
            }
            let ctx_thunk = found_with.unwrap_or_else(|| {
                report_error(
                    env,
                    expr.span.clone(),
                    "Implicit field access outside of 'with' block",
                    None,
                    None,
                );
                Thunk::evaluated(Value::Poison)
            });
            let mut current = evaluate(ctx_thunk)?;
            for field in fields {
                if let Value::Poison = current {
                    return Ok(Value::Poison);
                }
                match current {
                    Value::AttrSet(mut map) => {
                        let thunk = if let Some(t) = map.remove(field) {
                            t
                        } else {
                            // HashMap order differs from run to run; sort so equally
                            // close candidates always give the same suggestion.
                            let mut keys: Vec<String> = map.keys().cloned().collect();
                            keys.sort();
                            let hint = did_you_mean(field, keys.iter());
                            report_error(
                                env,
                                expr.span.clone(),
                                &format!("Field '{}' not found in attribute set", field),
                                hint.map(|s| s.as_str()),
                                None,
                            );
                            Thunk::evaluated(Value::Poison)
                        };
                        current = evaluate(thunk)?;
                    }
                    _ => {
                        report_error(
                            env,
                            expr.span.clone(),
                            &format!(
                                "Cannot access field '{}' on non-attribute set {:?}",
                                field, current
                            ),
                            None,
                            None,
                        );
                        return Ok(Value::Poison);
                    }
                }
            }
            Ok(current)
        }
        ExprKind::App(f, arg) => {
            let func_val = eval_expr(f, env)?;
            if let Value::Poison = func_val {
                return Ok(Value::Poison);
            }
            apply_value(
                func_val,
                Thunk::new(*arg.clone(), env.clone()),
                env,
                &expr.span,
            )
        }
        ExprKind::LetIn(bindings, body) => {
            let new_env = env.extend();
            for (k, v) in bindings {
                let thunk = Thunk::new(v.clone(), new_env.clone());
                new_env.define(k.clone(), thunk);
            }
            eval_expr(body, &new_env)
        }
        ExprKind::BinOp(lhs, op, rhs) => {
            if *op == Op::And {
                let left = eval_expr(lhs, env)?;
                if let Value::Poison = left {
                    return Ok(Value::Poison);
                }
                if let Value::Bool(false) = left {
                    return Ok(Value::Bool(false));
                }
                let right = eval_expr(rhs, env)?;
                if let Value::Poison = right {
                    return Ok(Value::Poison);
                }
                return match (left, right) {
                    (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a && b)),
                    _ => {
                        report_error(env, expr.span.clone(), "Invalid types for &&", None, None);
                        Ok(Value::Poison)
                    }
                };
            }
            if *op == Op::Or {
                let left = eval_expr(lhs, env)?;
                if let Value::Poison = left {
                    return Ok(Value::Poison);
                }
                if let Value::Bool(true) = left {
                    return Ok(Value::Bool(true));
                }
                let right = eval_expr(rhs, env)?;
                if let Value::Poison = right {
                    return Ok(Value::Poison);
                }
                return match (left, right) {
                    (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a || b)),
                    _ => {
                        report_error(env, expr.span.clone(), "Invalid types for ||", None, None);
                        Ok(Value::Poison)
                    }
                };
            }

            let left = eval_expr(lhs, env)?;
            if let Value::Poison = left {
                return Ok(Value::Poison);
            }
            let right = eval_expr(rhs, env)?;
            if let Value::Poison = right {
                return Ok(Value::Poison);
            }
            match (left, op, right) {
                (Value::Int(a), Op::Add, Value::Int(b)) => match a.checked_add(b) {
                    Some(n) => Ok(Value::Int(n)),
                    None => {
                        report_error(env, expr.span.clone(), "Integer overflow", None, None);
                        Ok(Value::Poison)
                    }
                },
                (Value::Int(a), Op::Sub, Value::Int(b)) => match a.checked_sub(b) {
                    Some(n) => Ok(Value::Int(n)),
                    None => {
                        report_error(env, expr.span.clone(), "Integer overflow", None, None);
                        Ok(Value::Poison)
                    }
                },
                (Value::Int(a), Op::Mul, Value::Int(b)) => match a.checked_mul(b) {
                    Some(n) => Ok(Value::Int(n)),
                    None => {
                        report_error(env, expr.span.clone(), "Integer overflow", None, None);
                        Ok(Value::Poison)
                    }
                },
                (Value::Int(a), Op::Div, Value::Int(b)) => {
                    if b == 0 {
                        report_error(env, expr.span.clone(), "Division by zero", None, None);
                        Ok(Value::Poison)
                    } else {
                        match a.checked_div(b) {
                            Some(n) => Ok(Value::Int(n)),
                            None => {
                                report_error(env, expr.span.clone(), "Integer overflow", None, None);
                                Ok(Value::Poison)
                            }
                        }
                    }
                }
                (Value::Float(a), Op::Add, Value::Float(b)) => Ok(Value::Float(a + b)),
                (Value::Float(a), Op::Sub, Value::Float(b)) => Ok(Value::Float(a - b)),
                (Value::Float(a), Op::Mul, Value::Float(b)) => Ok(Value::Float(a * b)),
                (Value::Float(a), Op::Div, Value::Float(b)) => Ok(Value::Float(a / b)),
                (Value::Int(a), Op::Add, Value::Float(b)) => Ok(Value::Float(a as f64 + b)),
                (Value::Float(a), Op::Add, Value::Int(b)) => Ok(Value::Float(a + b as f64)),
                (Value::Int(a), Op::Sub, Value::Float(b)) => Ok(Value::Float(a as f64 - b)),
                (Value::Float(a), Op::Sub, Value::Int(b)) => Ok(Value::Float(a - b as f64)),
                (Value::Int(a), Op::Mul, Value::Float(b)) => Ok(Value::Float(a as f64 * b)),
                (Value::Float(a), Op::Mul, Value::Int(b)) => Ok(Value::Float(a * b as f64)),
                (Value::Int(a), Op::Div, Value::Float(b)) => Ok(Value::Float(a as f64 / b)),
                (Value::Float(a), Op::Div, Value::Int(b)) => Ok(Value::Float(a / b as f64)),

                (Value::Int(a), Op::Eq, Value::Int(b)) => Ok(Value::Bool(a == b)),
                (Value::Int(a), Op::Neq, Value::Int(b)) => Ok(Value::Bool(a != b)),
                (Value::Int(a), Op::Lt, Value::Int(b)) => Ok(Value::Bool(a < b)),
                (Value::Int(a), Op::Lte, Value::Int(b)) => Ok(Value::Bool(a <= b)),
                (Value::Int(a), Op::Gt, Value::Int(b)) => Ok(Value::Bool(a > b)),
                (Value::Int(a), Op::Gte, Value::Int(b)) => Ok(Value::Bool(a >= b)),

                (Value::Float(a), Op::Eq, Value::Float(b)) => Ok(Value::Bool(a == b)),
                (Value::Float(a), Op::Neq, Value::Float(b)) => Ok(Value::Bool(a != b)),
                (Value::Float(a), Op::Lt, Value::Float(b)) => Ok(Value::Bool(a < b)),
                (Value::Float(a), Op::Lte, Value::Float(b)) => Ok(Value::Bool(a <= b)),
                (Value::Float(a), Op::Gt, Value::Float(b)) => Ok(Value::Bool(a > b)),
                (Value::Float(a), Op::Gte, Value::Float(b)) => Ok(Value::Bool(a >= b)),

                (Value::String(a), Op::Eq, Value::String(b)) => Ok(Value::Bool(a == b)),
                (Value::String(a), Op::Neq, Value::String(b)) => Ok(Value::Bool(a != b)),

                (Value::Bool(a), Op::Eq, Value::Bool(b)) => Ok(Value::Bool(a == b)),
                (Value::Bool(a), Op::Neq, Value::Bool(b)) => Ok(Value::Bool(a != b)),

                _ => {
                    report_error(
                        env,
                        expr.span.clone(),
                        "Invalid types for binary operation",
                        None,
                        None,
                    );
                    Ok(Value::Poison)
                }
            }
        }
    }
}
