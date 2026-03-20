use crate::ast::*;
use crate::value::*;
use std::collections::HashMap;

pub fn evaluate(thunk: Thunk) -> Result<Value, String> {
    let state = {
        let b = thunk.0.borrow();
        match &*b {
            ThunkState::Evaluated(val) => return Ok(val.clone()),
            ThunkState::Evaluating => return Err("Infinite recursion detected".to_string()),
            ThunkState::Unevaluated { .. } => {}
        }
    };

    let (expr, env) = match thunk.0.replace(ThunkState::Evaluating) {
        ThunkState::Unevaluated { expr, env } => (expr, env),
        _ => unreachable!(),
    };

    let val = eval_expr(&expr, &env)?;
    *thunk.0.borrow_mut() = ThunkState::Evaluated(val.clone());
    Ok(val)
}

pub fn eval_expr(expr: &Expr, env: &Env) -> Result<Value, String> {
    match expr {
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::IfElse(cond, true_branch, false_branch) => {
            let cond_val = eval_expr(cond, env)?;
            match cond_val {
                Value::Bool(true) => eval_expr(true_branch, env),
                Value::Bool(false) => eval_expr(false_branch, env),
                _ => Err(format!(
                    "Condition in if expression must be a boolean, got {:?}",
                    cond_val
                )),
            }
        }
        Expr::Int(i) => Ok(Value::Int(*i)),
        Expr::Float(f) => Ok(Value::Float(*f)),
        Expr::String(parts) => {
            let mut result = String::new();
            for part in parts {
                match part {
                    StringPart::Literal(s) => result.push_str(s),
                    StringPart::Interpolation(ident) => {
                        let thunk = env
                            .get(ident)
                            .ok_or_else(|| format!("Variable '{}' not found", ident))?;
                        let val = evaluate(thunk)?;
                        match val {
                            Value::Int(i) => result.push_str(&i.to_string()),
                            Value::Float(f) => result.push_str(&f.to_string()),
                            Value::String(s) => result.push_str(&s),
                            _ => return Err(format!("Cannot interpolate {:?}", val)),
                        }
                    }
                }
            }
            Ok(Value::String(result))
        }
        Expr::Path(p) => Ok(Value::Path(p.clone())),
        Expr::List(exprs) => {
            let thunks = exprs
                .iter()
                .map(|e| Thunk::new(e.clone(), env.clone()))
                .collect();
            Ok(Value::List(thunks))
        }
        Expr::AttrSet { is_rec, attrs } => {
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
        Expr::Ident(name) => {
            let thunk = env
                .get(name)
                .ok_or_else(|| format!("Variable '{}' not found", name))?;
            evaluate(thunk)
        }
        Expr::FieldAccess(lhs, fields) => {
            let mut current = eval_expr(lhs, env)?;
            for field in fields {
                match current {
                    Value::AttrSet(mut map) => {
                        let thunk = map.remove(field).ok_or_else(|| {
                            format!("Field '{}' not found in attribute set", field)
                        })?;
                        current = evaluate(thunk)?;
                    }
                    _ => {
                        return Err(format!(
                            "Cannot access field '{}' on non-attribute set {:?}",
                            field, current
                        ));
                    }
                }
            }
            Ok(current)
        }
        Expr::Lambda(args, body) => Ok(Value::Closure {
            args: args.clone(),
            body: *body.clone(),
            env: env.clone(),
        }),
        Expr::With(obj, body) => {
            let obj_thunk = Thunk::new(*obj.clone(), env.clone());
            let new_env = env.with_context(obj_thunk);
            eval_expr(body, &new_env)
        }
        Expr::ImplicitAccess(fields) => {
            let mut current_env = Some(env.clone());
            let mut found_with = None;
            while let Some(e) = current_env {
                if let Some(ctx) = &e.with_context {
                    found_with = Some(ctx.clone());
                    break;
                }
                current_env = e.parent.as_deref().cloned();
            }
            let ctx_thunk = found_with
                .ok_or_else(|| "Implicit field access outside of 'with' block".to_string())?;
            let mut current = evaluate(ctx_thunk)?;
            for field in fields {
                match current {
                    Value::AttrSet(mut map) => {
                        let thunk = map.remove(field).ok_or_else(|| {
                            format!("Field '{}' not found in attribute set", field)
                        })?;
                        current = evaluate(thunk)?;
                    }
                    _ => {
                        return Err(format!(
                            "Cannot access field '{}' on non-attribute set {:?}",
                            field, current
                        ));
                    }
                }
            }
            Ok(current)
        }
        Expr::App(f, arg) => {
            let func_val = eval_expr(f, env)?;
            let arg_thunk = Thunk::new(*arg.clone(), env.clone());

            match func_val {
                Value::NativeClosure(f) => {
                    let arg_val = evaluate(arg_thunk)?;
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
                            call_env.define(names[0].clone(), arg_thunk);

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
                            let arg_val = evaluate(arg_thunk)?;
                            match arg_val {
                                Value::AttrSet(mut map) => {
                                    for name in names {
                                        let val_thunk = map.remove(&name).ok_or_else(|| {
                                            format!(
                                                "Missing required attribute '{}' in destructuring",
                                                name
                                            )
                                        })?;
                                        call_env.define(name, val_thunk);
                                    }
                                    if !ignore_rest && !map.is_empty() {
                                        return Err(format!(
                                            "Unexpected attributes in destructuring: {:?}",
                                            map.keys()
                                        ));
                                    }
                                }
                                _ => {
                                    return Err(
                                        "Expected an attribute set for destructuring".into()
                                    );
                                }
                            }
                            eval_expr(&body, &call_env)
                        }
                    }
                }
                _ => return Err(format!("Not a function: {:?}", func_val)),
            }
        }
        Expr::LetIn(bindings, body) => {
            let new_env = env.extend();
            for (k, v) in bindings {
                let thunk = Thunk::new(v.clone(), new_env.clone());
                new_env.define(k.clone(), thunk);
            }
            eval_expr(body, &new_env)
        }
        Expr::BinOp(lhs, op, rhs) => {
            if *op == Op::And {
                let left = eval_expr(lhs, env)?;
                if let Value::Bool(false) = left {
                    return Ok(Value::Bool(false));
                }
                let right = eval_expr(rhs, env)?;
                return match (left, right) {
                    (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a && b)),
                    _ => Err("Invalid types for &&".into()),
                };
            }
            if *op == Op::Or {
                let left = eval_expr(lhs, env)?;
                if let Value::Bool(true) = left {
                    return Ok(Value::Bool(true));
                }
                let right = eval_expr(rhs, env)?;
                return match (left, right) {
                    (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a || b)),
                    _ => Err("Invalid types for ||".into()),
                };
            }

            let left = eval_expr(lhs, env)?;
            let right = eval_expr(rhs, env)?;
            match (left, op, right) {
                (Value::Int(a), Op::Add, Value::Int(b)) => Ok(Value::Int(a + b)),
                (Value::Int(a), Op::Sub, Value::Int(b)) => Ok(Value::Int(a - b)),
                (Value::Int(a), Op::Mul, Value::Int(b)) => Ok(Value::Int(a * b)),
                (Value::Int(a), Op::Div, Value::Int(b)) => {
                    if b == 0 {
                        Err("Division by zero".into())
                    } else {
                        Ok(Value::Int(a / b))
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

                _ => Err("Invalid types for binary operation".into()),
            }
        }
    }
}
