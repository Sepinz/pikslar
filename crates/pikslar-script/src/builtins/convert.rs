//! Conversions and number functions. All are deterministic: no locale, no platform maths
//! beyond IEEE operations that are exactly rounded (`sqrt`, `floor`, `round`).

use super::Cx;
use crate::error::{ErrorKind, Fault};
use crate::limits::{byte_fuel, item_fuel};
use crate::ops::finite;
use crate::value::{Str, Value};
use std::fmt::Write;

/// The text `str` gives a value: the same as it prints.
pub(super) fn to_str(v: &Value, cx: &mut Cx<'_, '_>) -> Result<Value, Fault> {
    match v {
        Value::Str(_) => return Ok(v.clone()),
        Value::List(l) => cx.fuel.burn(item_fuel(l.len()))?,
        _ => {}
    }
    let len = v.render_len();
    cx.fuel.burn(byte_fuel(len))?;
    Str::build(cx.meter, len, |s| {
        let _ = write!(s, "{v}");
    })
    .map(Value::Str)
}

/// Largest float magnitude that converts to an integer: 2^63 is just out of range.
const INT_LIMIT: f64 = 9_223_372_036_854_775_808.0;

fn float_to_int(x: f64) -> Result<i64, Fault> {
    let t = x.trunc();
    if (-INT_LIMIT..INT_LIMIT).contains(&t) {
        Ok(t as i64)
    } else {
        Err(Fault::new(
            ErrorKind::Arithmetic,
            "number out of the integer range",
        ))
    }
}

pub(super) fn to_int(v: &Value, cx: &mut Cx<'_, '_>) -> Result<Value, Fault> {
    match v {
        Value::Int(_) => Ok(v.clone()),
        Value::Float(x) => float_to_int(*x).map(Value::Int),
        Value::Str(s) => {
            cx.fuel.burn(byte_fuel(s.as_str().len()))?;
            s.as_str()
                .parse()
                .map(Value::Int)
                .map_err(|_| Fault::new(ErrorKind::Value, "the string is not an integer"))
        }
        other => Err(Fault::type_error(format!(
            "cannot make an integer from {}",
            other.type_name()
        ))),
    }
}

pub(super) fn to_float(v: &Value, cx: &mut Cx<'_, '_>) -> Result<Value, Fault> {
    match v {
        Value::Int(n) => Ok(Value::Float(*n as f64)),
        Value::Float(_) => Ok(v.clone()),
        Value::Str(s) => {
            cx.fuel.burn(byte_fuel(s.as_str().len()))?;
            let x: f64 = s
                .as_str()
                .parse()
                .map_err(|_| Fault::new(ErrorKind::Value, "the string is not a number"))?;
            finite(x).map_err(|_| Fault::new(ErrorKind::Value, "the string is not a finite number"))
        }
        other => Err(Fault::type_error(format!(
            "cannot make a float from {}",
            other.type_name()
        ))),
    }
}

pub(super) fn abs(v: &Value) -> Result<Value, Fault> {
    match v {
        Value::Int(n) => n.checked_abs().map(Value::Int).ok_or_else(Fault::overflow),
        Value::Float(x) => Ok(Value::Float(x.abs())),
        other => Err(Fault::type_error(format!("abs of {}", other.type_name()))),
    }
}

/// `floor` and `round` give integers.
pub(super) fn rounded(v: &Value, f: fn(f64) -> f64) -> Result<Value, Fault> {
    match v {
        Value::Int(_) => Ok(v.clone()),
        Value::Float(x) => float_to_int(f(*x)).map(Value::Int),
        other => Err(Fault::type_error(format!(
            "cannot round {}",
            other.type_name()
        ))),
    }
}

pub(super) fn sqrt(v: &Value) -> Result<Value, Fault> {
    let x = match v {
        Value::Int(n) => *n as f64,
        Value::Float(x) => *x,
        other => return Err(Fault::type_error(format!("sqrt of {}", other.type_name()))),
    };
    if x < 0.0 {
        return Err(Fault::new(
            ErrorKind::Arithmetic,
            "sqrt of a negative number",
        ));
    }
    finite(x.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_to_int_checks_range() {
        assert_eq!(float_to_int(-2.7).ok(), Some(-2));
        assert_eq!(float_to_int(-INT_LIMIT).ok(), Some(i64::MIN));
        assert!(float_to_int(INT_LIMIT).is_err());
        assert!(float_to_int(1e30).is_err());
    }

    #[test]
    fn rounding_and_roots() {
        assert!(matches!(
            rounded(&Value::Float(2.5), f64::round),
            Ok(Value::Int(3))
        ));
        assert!(matches!(
            rounded(&Value::Float(-0.5), f64::floor),
            Ok(Value::Int(-1))
        ));
        assert!(matches!(sqrt(&Value::Int(9)), Ok(Value::Float(x)) if x == 3.0));
        assert!(sqrt(&Value::Int(-1)).is_err());
        assert!(abs(&Value::Int(i64::MIN)).is_err());
    }
}
