//! Builtin functions. Each burns fuel for the work it does before doing it, and charges any
//! memory it allocates before allocating it. None of them reaches outside the run.

mod convert;

use crate::error::{ErrorKind, Fault};
use crate::fuel::Fuel;
use crate::limits::{byte_fuel, item_fuel};
use crate::meter::Meter;
use crate::ops;
use crate::value::{index_fault, List, Str, Value};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    Len,
    Push,
    Pop,
    Insert,
    Remove,
    Slice,
    Find,
    Repeat,
    List,
    Str,
    Int,
    Float,
    Type,
    Abs,
    Min,
    Max,
    Floor,
    Round,
    Sqrt,
}

const TABLE: [(&str, Builtin, usize); 19] = [
    ("len", Builtin::Len, 1),
    ("push", Builtin::Push, 2),
    ("pop", Builtin::Pop, 1),
    ("insert", Builtin::Insert, 3),
    ("remove", Builtin::Remove, 2),
    ("slice", Builtin::Slice, 3),
    ("find", Builtin::Find, 2),
    ("repeat", Builtin::Repeat, 2),
    ("list", Builtin::List, 2),
    ("str", Builtin::Str, 1),
    ("int", Builtin::Int, 1),
    ("float", Builtin::Float, 1),
    ("type", Builtin::Type, 1),
    ("abs", Builtin::Abs, 1),
    ("min", Builtin::Min, 2),
    ("max", Builtin::Max, 2),
    ("floor", Builtin::Floor, 1),
    ("round", Builtin::Round, 1),
    ("sqrt", Builtin::Sqrt, 1),
];

/// The builtin called `name`, and how many arguments it takes.
pub(crate) fn lookup(name: &str) -> Option<(Builtin, usize)> {
    TABLE
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, b, a)| (*b, *a))
}

/// What a builtin may use: the run's fuel and memory.
pub(crate) struct Cx<'a, 'c> {
    pub fuel: &'a mut Fuel<'c>,
    pub meter: &'a Rc<Meter>,
}

/// Runs builtin `b`. The parser has already checked the number of arguments.
pub(crate) fn call(b: Builtin, a: &[Value], cx: &mut Cx<'_, '_>) -> Result<Value, Fault> {
    cx.fuel.burn(1)?;
    match b {
        Builtin::Len => len(&a[0]),
        Builtin::Push => push(&a[0], &a[1], cx),
        Builtin::Pop => want_list(&a[0])?
            .pop()
            .ok_or_else(|| Fault::new(ErrorKind::Value, "pop from an empty list")),
        Builtin::Insert => insert(a, cx),
        Builtin::Remove => remove(a, cx),
        Builtin::Slice => slice(a, cx),
        Builtin::Find => find(&a[0], &a[1], cx),
        Builtin::Repeat => repeat(&a[0], &a[1], cx),
        Builtin::List => filled(&a[0], &a[1], cx),
        Builtin::Str => convert::to_str(&a[0], cx),
        Builtin::Int => convert::to_int(&a[0], cx),
        Builtin::Float => convert::to_float(&a[0], cx),
        Builtin::Type => Str::concat(cx.meter, &[a[0].type_name()]).map(Value::Str),
        Builtin::Abs => convert::abs(&a[0]),
        Builtin::Min | Builtin::Max => min_max(b, &a[0], &a[1], cx),
        Builtin::Floor => convert::rounded(&a[0], f64::floor),
        Builtin::Round => convert::rounded(&a[0], f64::round),
        Builtin::Sqrt => convert::sqrt(&a[0]),
    }
}

pub(crate) fn want_list(v: &Value) -> Result<&List, Fault> {
    match v {
        Value::List(l) => Ok(l),
        other => Err(Fault::type_error(format!(
            "expected a list, not {}",
            other.type_name()
        ))),
    }
}

fn want_str(v: &Value) -> Result<&Str, Fault> {
    match v {
        Value::Str(s) => Ok(s),
        other => Err(Fault::type_error(format!(
            "expected a string, not {}",
            other.type_name()
        ))),
    }
}

pub(crate) fn want_int(v: &Value) -> Result<i64, Fault> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(Fault::type_error(format!(
            "expected an integer, not {}",
            other.type_name()
        ))),
    }
}

/// An index from 0 to `max` inclusive.
fn want_place(v: &Value, max: usize) -> Result<usize, Fault> {
    usize::try_from(want_int(v)?)
        .ok()
        .filter(|&i| i <= max)
        .ok_or_else(index_fault)
}

fn len(v: &Value) -> Result<Value, Fault> {
    let n = match v {
        Value::Str(s) => s.as_str().len(),
        Value::List(l) => l.len(),
        other => {
            return Err(Fault::type_error(format!(
                "{} has no length",
                other.type_name()
            )))
        }
    };
    Ok(Value::Int(n as i64))
}

fn push(list: &Value, v: &Value, cx: &mut Cx<'_, '_>) -> Result<Value, Fault> {
    let list = want_list(list)?;
    let (len, cap) = {
        let items = list.items();
        (items.len(), items.capacity())
    };
    if len == cap {
        // Growing copies every item.
        cx.fuel.burn(item_fuel(len))?;
    }
    list.push(v.clone())?;
    Ok(Value::Nil)
}

fn insert(a: &[Value], cx: &mut Cx<'_, '_>) -> Result<Value, Fault> {
    let list = want_list(&a[0])?;
    let at = want_place(&a[1], list.len())?;
    cx.fuel.burn(item_fuel(list.len()))?;
    list.insert(at, a[2].clone())?;
    Ok(Value::Nil)
}

fn remove(a: &[Value], cx: &mut Cx<'_, '_>) -> Result<Value, Fault> {
    let list = want_list(&a[0])?;
    let at = want_place(&a[1], list.len())?;
    cx.fuel.burn(item_fuel(list.len()))?;
    list.remove(at)
}

fn slice(a: &[Value], cx: &mut Cx<'_, '_>) -> Result<Value, Fault> {
    let size = match &a[0] {
        Value::Str(s) => s.as_str().len(),
        other => want_list(other)?.len(),
    };
    let from = want_place(&a[1], size)?;
    let to = want_place(&a[2], size)?;
    if to < from {
        return Err(index_fault());
    }
    match &a[0] {
        Value::Str(s) => {
            let part = s.as_str().get(from..to).ok_or_else(|| {
                Fault::new(ErrorKind::Index, "slice is not on character boundaries")
            })?;
            cx.fuel.burn(byte_fuel(part.len()))?;
            Str::concat(cx.meter, &[part]).map(Value::Str)
        }
        other => {
            let src = want_list(other)?;
            cx.fuel.burn(item_fuel(to - from))?;
            let out = List::with_capacity(cx.meter, to - from)?;
            for v in &src.items()[from..to] {
                out.push(v.clone())?;
            }
            Ok(Value::List(out))
        }
    }
}

/// The first place `needle` occurs, or -1. Strings use the standard library's two-way search,
/// which is linear in the lengths and allocates nothing.
fn find(hay: &Value, needle: &Value, cx: &mut Cx<'_, '_>) -> Result<Value, Fault> {
    let at = match hay {
        Value::Str(s) => {
            let n = want_str(needle)?;
            cx.fuel
                .burn(byte_fuel(s.as_str().len().saturating_add(n.as_str().len())))?;
            s.as_str().find(n.as_str())
        }
        other => {
            let list = want_list(other)?;
            let mut found = None;
            for (i, v) in list.items().iter().enumerate() {
                cx.fuel.burn(1)?;
                if ops::equal(v, needle, cx.fuel)? {
                    found = Some(i);
                    break;
                }
            }
            found
        }
    };
    Ok(Value::Int(at.map_or(-1, |i| i as i64)))
}

fn count(v: &Value) -> Result<usize, Fault> {
    usize::try_from(want_int(v)?)
        .map_err(|_| Fault::new(ErrorKind::Value, "a count cannot be negative"))
}

fn repeat(s: &Value, n: &Value, cx: &mut Cx<'_, '_>) -> Result<Value, Fault> {
    let s = want_str(s)?.as_str();
    let n = count(n)?;
    let total = s.len().checked_mul(n).ok_or_else(Fault::memory)?;
    if !cx.meter.fits(total) {
        return Err(Fault::memory());
    }
    // The copy loop turns `n` times. Nothing to copy means no loop at all; otherwise each
    // turn copies at least one byte, so the byte charge covers the turns too.
    if total == 0 {
        return Str::concat(cx.meter, &[]).map(Value::Str);
    }
    cx.fuel.burn(byte_fuel(total.max(n)))?;
    Str::build(cx.meter, total, |out| (0..n).for_each(|_| out.push_str(s))).map(Value::Str)
}

fn filled(n: &Value, v: &Value, cx: &mut Cx<'_, '_>) -> Result<Value, Fault> {
    let n = count(n)?;
    let list = List::with_capacity(cx.meter, n)?;
    cx.fuel.burn(item_fuel(n))?;
    for _ in 0..n {
        list.push(v.clone())?;
    }
    Ok(Value::List(list))
}

fn min_max(b: Builtin, x: &Value, y: &Value, cx: &mut Cx<'_, '_>) -> Result<Value, Fault> {
    if !matches!(x, Value::Int(_) | Value::Float(_))
        || !matches!(y, Value::Int(_) | Value::Float(_))
    {
        return Err(Fault::type_error("min and max take numbers"));
    }
    let x_first = ops::order(x, y, cx.fuel)?.is_le() == (b == Builtin::Min);
    Ok(if x_first { x.clone() } else { y.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn text(cx: &Cx<'_, '_>, s: &str) -> Value {
        Value::Str(Str::concat(cx.meter, &[s]).expect("fits"))
    }

    fn with_cx<T>(f: impl FnOnce(&mut Cx<'_, '_>) -> T) -> T {
        let flag = AtomicBool::new(false);
        let mut fuel = Fuel::new(1_000_000, &flag);
        let meter = Meter::new(1 << 20);
        f(&mut Cx {
            fuel: &mut fuel,
            meter: &meter,
        })
    }

    #[test]
    fn every_builtin_is_found_by_name() {
        for (name, b, arity) in TABLE {
            assert_eq!(lookup(name), Some((b, arity)));
        }
        assert_eq!(lookup("open"), None);
    }

    #[test]
    fn repeat_over_the_cap_is_a_memory_error() {
        with_cx(|cx| {
            let s = text(cx, "ab");
            let e = repeat(&s, &Value::Int(1 << 40), cx).expect_err("too big");
            assert_eq!(e.kind, ErrorKind::Memory);
            let e = repeat(&s, &Value::Int(i64::MAX), cx).expect_err("too big");
            assert_eq!(e.kind, ErrorKind::Memory);
        });
    }

    #[test]
    fn find_in_strings_and_lists() {
        with_cx(|cx| {
            let s = text(cx, "hello world");
            let n = text(cx, "o w");
            assert!(matches!(find(&s, &n, cx), Ok(Value::Int(4))));
            let list = List::with_capacity(cx.meter, 0).expect("fits");
            list.push(Value::Int(3)).expect("fits");
            list.push(Value::Int(7)).expect("fits");
            assert!(matches!(
                find(&Value::List(list.clone()), &Value::Int(7), cx),
                Ok(Value::Int(1))
            ));
            assert!(matches!(
                find(&Value::List(list), &Value::Int(8), cx),
                Ok(Value::Int(-1))
            ));
        });
    }

    #[test]
    fn slices_check_bounds_and_boundaries() {
        with_cx(|cx| {
            let s = text(cx, "añb");
            let ok = slice(&[s.clone(), Value::Int(1), Value::Int(3)], cx).expect("fits");
            assert_eq!(ok.as_str(), Some("ñ"));
            assert!(slice(&[s.clone(), Value::Int(2), Value::Int(3)], cx).is_err());
            assert!(slice(&[s.clone(), Value::Int(3), Value::Int(1)], cx).is_err());
            assert!(slice(&[s, Value::Int(0), Value::Int(9)], cx).is_err());
        });
    }
}
