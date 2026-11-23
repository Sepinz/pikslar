//! Operators. Integer arithmetic is checked and fails on overflow; float results must be
//! finite. Work on strings and lists burns fuel in proportion to their size.

use crate::ast::BinOp;
use crate::error::{ErrorKind, Fault};
use crate::fuel::Fuel;
use crate::limits::{byte_fuel, item_fuel};
use crate::meter::Meter;
use crate::value::{List, Str, Value};
use std::cmp::Ordering;
use std::rc::Rc;

pub(crate) fn binary(
    op: BinOp,
    l: Value,
    r: Value,
    fuel: &mut Fuel<'_>,
    meter: &Rc<Meter>,
) -> Result<Value, Fault> {
    if let (Value::Int(a), Value::Int(b)) = (&l, &r) {
        return int_binary(op, *a, *b);
    }
    match op {
        BinOp::Eq => equal(&l, &r, fuel).map(Value::Bool),
        BinOp::Ne => equal(&l, &r, fuel).map(|e| Value::Bool(!e)),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            let ord = order(&l, &r, fuel)?;
            Ok(Value::Bool(match op {
                BinOp::Lt => ord.is_lt(),
                BinOp::Le => ord.is_le(),
                BinOp::Gt => ord.is_gt(),
                _ => ord.is_ge(),
            }))
        }
        BinOp::Add => match (&l, &r) {
            (Value::Str(a), Value::Str(b)) => concat(a, b, fuel, meter),
            (Value::Str(_), _) | (_, Value::Str(_)) => Err(Fault::type_error(
                "'+' joins two strings; use str() to turn a value into a string",
            )),
            _ => arith(op, &l, &r),
        },
        _ => arith(op, &l, &r),
    }
}

/// The common case, integers with integers, without touching fuel or memory.
#[inline]
fn int_binary(op: BinOp, a: i64, b: i64) -> Result<Value, Fault> {
    Ok(Value::Bool(match op {
        BinOp::Eq => a == b,
        BinOp::Ne => a != b,
        BinOp::Lt => a < b,
        BinOp::Le => a <= b,
        BinOp::Gt => a > b,
        BinOp::Ge => a >= b,
        _ => return int_arith(op, a, b).map(Value::Int),
    }))
}

fn concat(a: &Str, b: &Str, fuel: &mut Fuel<'_>, meter: &Rc<Meter>) -> Result<Value, Fault> {
    let len = a.as_str().len().saturating_add(b.as_str().len());
    fuel.burn(byte_fuel(len))?;
    Str::concat(meter, &[a.as_str(), b.as_str()]).map(Value::Str)
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::Float(x) => Some(*x),
        _ => None,
    }
}

fn arith(op: BinOp, l: &Value, r: &Value) -> Result<Value, Fault> {
    if let (Value::Int(a), Value::Int(b)) = (l, r) {
        return int_arith(op, *a, *b).map(Value::Int);
    }
    let (Some(a), Some(b)) = (number(l), number(r)) else {
        let msg = format!(
            "arithmetic needs numbers, not {} and {}",
            l.type_name(),
            r.type_name()
        );
        return Err(Fault::type_error(msg));
    };
    let x = match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => a / b,
        _ => a.rem_euclid(b),
    };
    finite(x)
}

pub(crate) fn finite(x: f64) -> Result<Value, Fault> {
    if x.is_finite() {
        Ok(Value::Float(x))
    } else {
        Err(Fault::new(
            ErrorKind::Arithmetic,
            "float result is not finite",
        ))
    }
}

/// Division and remainder are Euclidean: the remainder is never negative, so `x % w` wraps
/// a coordinate into `0..w` even when `x` is negative.
fn int_arith(op: BinOp, a: i64, b: i64) -> Result<i64, Fault> {
    if b == 0 && matches!(op, BinOp::Div | BinOp::Rem) {
        return Err(Fault::new(ErrorKind::Arithmetic, "division by zero"));
    }
    match op {
        BinOp::Add => a.checked_add(b),
        BinOp::Sub => a.checked_sub(b),
        BinOp::Mul => a.checked_mul(b),
        BinOp::Div => a.checked_div_euclid(b),
        _ => a.checked_rem_euclid(b),
    }
    .ok_or_else(Fault::overflow)
}

fn str_cost(a: &Str, b: &Str) -> u64 {
    byte_fuel(a.as_str().len().min(b.as_str().len()))
}

/// Equality between scalars and strings. Values of different types are unequal, except that
/// integers and floats compare by value.
fn scalar_equal(a: &Value, b: &Value, fuel: &mut Fuel<'_>) -> Result<bool, Fault> {
    Ok(match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => {
            if Str::ptr_eq(x, y) {
                return Ok(true);
            }
            if x.as_str().len() != y.as_str().len() {
                return Ok(false);
            }
            fuel.burn(str_cost(x, y))?;
            x.as_str() == y.as_str()
        }
        (Value::List(x), Value::List(y)) => List::ptr_eq(x, y),
        _ => match (number(a), number(b)) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
    })
}

/// Equality. Lists are equal when they hold equal items in the same order.
pub(crate) fn equal(a: &Value, b: &Value, fuel: &mut Fuel<'_>) -> Result<bool, Fault> {
    let (Value::List(x), Value::List(y)) = (a, b) else {
        return scalar_equal(a, b, fuel);
    };
    if List::ptr_eq(x, y) {
        return Ok(true);
    }
    let (x, y) = (x.items(), y.items());
    if x.len() != y.len() {
        return Ok(false);
    }
    fuel.burn(item_fuel(x.len()))?;
    for (p, q) in x.iter().zip(y.iter()) {
        if !scalar_equal(p, q, fuel)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Ordering, for numbers with numbers and strings with strings (by bytes).
pub(crate) fn order(a: &Value, b: &Value, fuel: &mut Fuel<'_>) -> Result<Ordering, Fault> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::Str(x), Value::Str(y)) => {
            fuel.burn(str_cost(x, y))?;
            Ok(x.as_str().cmp(y.as_str()))
        }
        _ => match (number(a), number(b)) {
            // Floats are always finite, so this always has an answer.
            (Some(x), Some(y)) => Ok(x.partial_cmp(&y).unwrap_or(Ordering::Equal)),
            _ => Err(Fault::type_error(format!(
                "cannot order {} and {}",
                a.type_name(),
                b.type_name()
            ))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn run(op: BinOp, l: Value, r: Value) -> Result<Value, Fault> {
        let flag = AtomicBool::new(false);
        let mut fuel = Fuel::new(1_000_000, &flag);
        binary(op, l, r, &mut fuel, &Meter::new(1 << 20))
    }

    fn int(op: BinOp, a: i64, b: i64) -> Result<i64, ErrorKind> {
        match run(op, Value::Int(a), Value::Int(b)) {
            Ok(Value::Int(n)) => Ok(n),
            Ok(_) => Err(ErrorKind::Type),
            Err(e) => Err(e.kind),
        }
    }

    #[test]
    fn integer_overflow_is_an_error() {
        assert_eq!(int(BinOp::Add, i64::MAX, 1), Err(ErrorKind::Arithmetic));
        assert_eq!(int(BinOp::Sub, i64::MIN, 1), Err(ErrorKind::Arithmetic));
        assert_eq!(int(BinOp::Mul, i64::MAX, 2), Err(ErrorKind::Arithmetic));
        assert_eq!(int(BinOp::Div, i64::MIN, -1), Err(ErrorKind::Arithmetic));
        assert_eq!(int(BinOp::Rem, 5, 0), Err(ErrorKind::Arithmetic));
        assert_eq!(int(BinOp::Div, 5, 0), Err(ErrorKind::Arithmetic));
    }

    #[test]
    fn division_is_euclidean() {
        assert_eq!(int(BinOp::Div, 7, 2), Ok(3));
        assert_eq!(int(BinOp::Div, -7, 2), Ok(-4));
        assert_eq!(int(BinOp::Rem, -7, 2), Ok(1));
        assert_eq!(int(BinOp::Rem, -1, 256), Ok(255));
    }

    #[test]
    fn floats_must_stay_finite() {
        let e = run(BinOp::Div, Value::Float(1.0), Value::Int(0)).expect_err("inf");
        assert_eq!(e.kind, ErrorKind::Arithmetic);
        let e = run(BinOp::Mul, Value::Float(1e300), Value::Float(1e300)).expect_err("inf");
        assert_eq!(e.kind, ErrorKind::Arithmetic);
        assert!(
            matches!(run(BinOp::Add, Value::Int(1), Value::Float(0.5)), Ok(Value::Float(x)) if x == 1.5)
        );
    }

    #[test]
    fn equality_and_order() {
        assert!(matches!(
            run(BinOp::Eq, Value::Int(1), Value::Float(1.0)),
            Ok(Value::Bool(true))
        ));
        assert!(matches!(
            run(BinOp::Eq, Value::Int(1), Value::Nil),
            Ok(Value::Bool(false))
        ));
        assert!(run(BinOp::Lt, Value::Int(1), Value::Nil).is_err());
        assert!(run(BinOp::Add, Value::Int(1), Value::Bool(true)).is_err());
    }
}
