//! Comparison of values: `==` / `!=` for every data type, and ordering
//! (`<` `<=` `>` `>=`) for numbers and strings.

use crate::eval::{check_data_depth, evaluate};
use crate::value::{Thunk, Value};
use std::cmp::Ordering;

/// The result of `==`.
pub enum Equality {
    Equal,
    NotEqual,
    /// An element failed to evaluate (the error was already reported).
    Poison,
    /// A function was reached; functions cannot be compared.
    Function,
}

/// Compares an `i64` with an `f64` exactly, without converting the integer to a
/// float first (which would round integers beyond 2^53 and make, say,
/// `9007199254740993 == 9007199254740992.0` true). `None` if `f` is NaN.
pub fn cmp_int_float(i: i64, f: f64) -> Option<Ordering> {
    if f.is_nan() {
        return None;
    }
    // 2^63 is exactly representable; every float at or above it exceeds any i64,
    // and every one below -2^63 is smaller than any i64.
    if f >= 9223372036854775808.0 {
        return Some(Ordering::Less);
    }
    if f < -9223372036854775808.0 {
        return Some(Ordering::Greater);
    }
    let truncated = f.trunc();
    // In range now, so this cast is exact.
    match i.cmp(&(truncated as i64)) {
        Ordering::Equal => {
            let fraction = f - truncated;
            if fraction > 0.0 {
                Some(Ordering::Less)
            } else if fraction < 0.0 {
                Some(Ordering::Greater)
            } else {
                Some(Ordering::Equal)
            }
        }
        other => Some(other),
    }
}

/// Orders two values for `<` `<=` `>` `>=`. Numbers (int and float may be mixed)
/// and strings (by bytes, i.e. by code point) can be ordered. `Ok(None)` means
/// "unordered" (a NaN is involved), which makes every ordering operator false;
/// `Err(())` means the types cannot be ordered at all.
pub fn compare_order(a: &Value, b: &Value) -> Result<Option<Ordering>, ()> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Some(x.cmp(y))),
        (Value::Float(x), Value::Float(y)) => Ok(x.partial_cmp(y)),
        (Value::Int(x), Value::Float(y)) => Ok(cmp_int_float(*x, *y)),
        (Value::Float(x), Value::Int(y)) => Ok(cmp_int_float(*y, *x).map(Ordering::reverse)),
        (Value::String(x), Value::String(y)) => Ok(Some(x.as_str().cmp(y.as_str()))),
        _ => Err(()),
    }
}

/// `a == b`. Ints and floats compare by numeric value; lists and attribute sets
/// compare structurally (element by element, forcing elements only as far as
/// needed); paths compare as strings; values of different types are simply not
/// equal. A function anywhere in the comparison is an error.
///
/// The walk uses its own work stack, so a deeply nested value cannot overflow the
/// native stack, and a value that contains itself ends in the same "nested too
/// deeply" error as everything else that walks a value's shape instead of looping
/// forever.
pub fn values_equal(a: &Value, b: &Value) -> Result<Equality, String> {
    // Pairs of still-unevaluated values, with how deep in the data they are.
    let mut stack: Vec<(Thunk, Thunk, usize)> = vec![(
        Thunk::evaluated(a.clone()),
        Thunk::evaluated(b.clone()),
        0,
    )];
    while let Some((tx, ty, depth)) = stack.pop() {
        check_data_depth(depth)?;
        let x = evaluate(tx)?;
        let y = evaluate(ty)?;
        match (&x, &y) {
            (Value::Poison, _) | (_, Value::Poison) => return Ok(Equality::Poison),
            (Value::Closure { .. } | Value::NativeClosure(_), _)
            | (_, Value::Closure { .. } | Value::NativeClosure(_)) => {
                return Ok(Equality::Function);
            }
            (Value::Int(p), Value::Int(q)) => {
                if p != q {
                    return Ok(Equality::NotEqual);
                }
            }
            // IEEE: a NaN is not equal to anything, itself included.
            (Value::Float(p), Value::Float(q)) => {
                if p != q {
                    return Ok(Equality::NotEqual);
                }
            }
            (Value::Int(p), Value::Float(q)) | (Value::Float(q), Value::Int(p)) => {
                if cmp_int_float(*p, *q) != Some(Ordering::Equal) {
                    return Ok(Equality::NotEqual);
                }
            }
            (Value::Bool(p), Value::Bool(q)) => {
                if p != q {
                    return Ok(Equality::NotEqual);
                }
            }
            (Value::String(p), Value::String(q)) | (Value::Path(p), Value::Path(q)) => {
                if p != q {
                    return Ok(Equality::NotEqual);
                }
            }
            (Value::List(xs), Value::List(ys)) => {
                if xs.len() != ys.len() {
                    return Ok(Equality::NotEqual);
                }
                // Reversed, so the first elements come off the stack first.
                for (ex, ey) in xs.iter().zip(ys.iter()).rev() {
                    stack.push((ex.clone(), ey.clone(), depth + 1));
                }
            }
            (Value::AttrSet(xs), Value::AttrSet(ys)) => {
                if xs.len() != ys.len() || !xs.keys().all(|k| ys.contains_key(k)) {
                    return Ok(Equality::NotEqual);
                }
                // Sorted, so which value is forced (and fails) first is stable.
                let mut keys: Vec<&String> = xs.keys().collect();
                keys.sort();
                for key in keys.into_iter().rev() {
                    stack.push((xs[key].clone(), ys[key].clone(), depth + 1));
                }
            }
            // Different types.
            _ => return Ok(Equality::NotEqual),
        }
    }
    Ok(Equality::Equal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_float_comparison_is_exact() {
        // Converting the int to a float would call these equal.
        assert_eq!(cmp_int_float(9007199254740993, 9007199254740992.0), Some(Ordering::Greater));
        assert_eq!(cmp_int_float(9007199254740992, 9007199254740992.0), Some(Ordering::Equal));
        assert_eq!(cmp_int_float(9007199254740991, 9007199254740992.0), Some(Ordering::Less));
    }

    #[test]
    fn int_float_comparison_around_the_i64_limits() {
        assert_eq!(cmp_int_float(i64::MAX, 9223372036854775808.0), Some(Ordering::Less));
        assert_eq!(cmp_int_float(i64::MIN, -9223372036854775808.0), Some(Ordering::Equal));
        assert_eq!(cmp_int_float(i64::MIN, -9223372036854777856.0), Some(Ordering::Greater));
        assert_eq!(cmp_int_float(i64::MAX, f64::INFINITY), Some(Ordering::Less));
        assert_eq!(cmp_int_float(i64::MIN, f64::NEG_INFINITY), Some(Ordering::Greater));
    }

    #[test]
    fn int_float_comparison_with_fractions_and_signs() {
        assert_eq!(cmp_int_float(1, 1.0), Some(Ordering::Equal));
        assert_eq!(cmp_int_float(1, 1.5), Some(Ordering::Less));
        assert_eq!(cmp_int_float(2, 1.5), Some(Ordering::Greater));
        assert_eq!(cmp_int_float(-1, -1.5), Some(Ordering::Greater));
        assert_eq!(cmp_int_float(-2, -1.5), Some(Ordering::Less));
        assert_eq!(cmp_int_float(0, -0.0), Some(Ordering::Equal));
        assert_eq!(cmp_int_float(0, 0.5), Some(Ordering::Less));
        assert_eq!(cmp_int_float(0, -0.5), Some(Ordering::Greater));
    }

    #[test]
    fn int_float_comparison_with_nan_is_unordered() {
        assert_eq!(cmp_int_float(1, f64::NAN), None);
    }
}
