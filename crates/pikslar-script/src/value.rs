//! Script values. Strings and lists live on the heap; each one charges the run's meter before
//! it is allocated and refunds the charge when the last reference to it is dropped.
//!
//! Lists hold scalars and strings only, never lists, so values cannot form cycles, and
//! dropping, comparing or printing a value never recurses.

use crate::error::Fault;
use crate::meter::Meter;
use std::cell::{Cell, Ref, RefCell};
use std::fmt::{self, Write};
use std::mem::size_of;
use std::rc::Rc;

/// A script value.
#[derive(Clone, Debug, Default)]
pub enum Value {
    #[default]
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Str),
    List(List),
}

impl Value {
    /// The type's name as scripts see it, from the `type` builtin.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::List(_) => "list",
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Bytes [`Value::render`] writes.
    pub(crate) fn render_len(&self) -> usize {
        match self {
            Value::Str(s) => s.as_str().len(),
            Value::List(l) => l.render_len(),
            other => {
                let mut c = Count(0);
                let _ = write_scalar(&mut c, other);
                c.0
            }
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Str(s) => f.write_str(s.as_str()),
            Value::List(l) => l.write_to(f),
            other => write_scalar(f, other),
        }
    }
}

/// Floats print with Rust's `Debug` form: always a point or an exponent, and never more than
/// about 24 characters, so `1.0` stays distinct from `1` and `1e300` stays short.
fn write_scalar(w: &mut impl Write, v: &Value) -> fmt::Result {
    match v {
        Value::Nil => w.write_str("nil"),
        Value::Bool(b) => write!(w, "{b}"),
        Value::Int(n) => write!(w, "{n}"),
        Value::Float(x) => write!(w, "{x:?}"),
        Value::Str(s) => w.write_str(s.as_str()),
        Value::List(_) => w.write_str("[list]"),
    }
}

struct Count(usize);

impl Write for Count {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0 += s.len();
        Ok(())
    }
}

/// An immutable string.
#[derive(Clone)]
pub struct Str(Rc<StrBox>);

struct StrBox {
    text: String,
    meter: Rc<Meter>,
    charged: usize,
}

impl Drop for StrBox {
    fn drop(&mut self) {
        self.meter.refund(self.charged);
    }
}

/// Bytes of the shared box behind a string or list: the reference counts and the fields.
const RC_COUNTS: usize = 2 * size_of::<usize>();
const STR_BOX: usize = size_of::<StrBox>() + RC_COUNTS;
const LIST_BOX: usize = size_of::<ListBox>() + RC_COUNTS;

impl Str {
    pub fn as_str(&self) -> &str {
        &self.0.text
    }

    /// Builds a string from `parts`, charging it in full before anything is allocated.
    pub(crate) fn concat(meter: &Rc<Meter>, parts: &[&str]) -> Result<Str, Fault> {
        let len = parts
            .iter()
            .try_fold(0usize, |n, p| n.checked_add(p.len()))
            .ok_or_else(Fault::memory)?;
        Str::build(meter, len, |s| parts.iter().for_each(|p| s.push_str(p)))
    }

    /// Builds a string of `len` bytes written by `fill`, charged before it is allocated.
    pub(crate) fn build(
        meter: &Rc<Meter>,
        len: usize,
        fill: impl FnOnce(&mut String),
    ) -> Result<Str, Fault> {
        let head = meter.charge_alloc(STR_BOX)?;
        let body = match len {
            0 => 0,
            n => meter.charge_alloc(n).inspect_err(|_| meter.refund(head))?,
        };
        let mut text = String::new();
        if text.try_reserve_exact(len).is_err() {
            meter.refund(head + body);
            return Err(Fault::alloc_failed());
        }
        fill(&mut text);
        let charged = head + body + extra_charge(meter, len, text.capacity(), body)?;
        Ok(Str(Rc::new(StrBox {
            text,
            meter: meter.clone(),
            charged,
        })))
    }

    pub(crate) fn ptr_eq(a: &Str, b: &Str) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }
}

/// If `fill` grew a buffer past what was reserved, charges the difference.
fn extra_charge(meter: &Meter, len: usize, cap: usize, charged: usize) -> Result<usize, Fault> {
    if cap <= len {
        return Ok(0);
    }
    let real = crate::limits::alloc_charge(cap).ok_or_else(Fault::memory)?;
    let extra = real.saturating_sub(charged);
    meter.charge(extra)?;
    Ok(extra)
}

impl fmt::Debug for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

/// A growable list of scalars and strings, shared by reference.
#[derive(Clone)]
pub struct List(Rc<ListBox>);

struct ListBox {
    items: RefCell<Vec<Value>>,
    meter: Rc<Meter>,
    head: usize,
    buffer: Cell<usize>,
}

impl Drop for ListBox {
    fn drop(&mut self) {
        self.meter.refund(self.head + self.buffer.get());
    }
}

impl fmt::Debug for List {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<list of {}>", self.len())
    }
}

const SLOT: usize = size_of::<Value>();

impl List {
    /// An empty list with room for `capacity` items, all charged before allocating.
    pub(crate) fn with_capacity(meter: &Rc<Meter>, capacity: usize) -> Result<List, Fault> {
        let head = meter.charge_alloc(LIST_BOX)?;
        let list = List(Rc::new(ListBox {
            items: RefCell::new(Vec::new()),
            meter: meter.clone(),
            head,
            buffer: Cell::new(0),
        }));
        list.reserve(capacity)?;
        Ok(list)
    }

    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, index: usize) -> Option<Value> {
        self.0.items.borrow().get(index).cloned()
    }

    pub(crate) fn items(&self) -> Ref<'_, Vec<Value>> {
        self.0.items.borrow()
    }

    pub(crate) fn ptr_eq(a: &List, b: &List) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }

    /// Makes room for `additional` more items. Growth at least doubles the capacity. The new
    /// buffer is charged in full while the old one is still charged, because a reallocation
    /// holds both while it copies; the old buffer's charge is refunded afterwards.
    pub(crate) fn reserve(&self, additional: usize) -> Result<(), Fault> {
        let mut items = self.0.items.borrow_mut();
        let need = items
            .len()
            .checked_add(additional)
            .ok_or_else(Fault::memory)?;
        if need <= items.capacity() {
            return Ok(());
        }
        let cap = need.max(items.capacity().saturating_mul(2)).max(4);
        let bytes = cap.checked_mul(SLOT).ok_or_else(Fault::memory)?;
        let meter = &self.0.meter;
        let charge = meter.charge_alloc(bytes)?;
        let len = items.len();
        if items.try_reserve_exact(cap - len).is_err() {
            meter.refund(charge);
            return Err(Fault::alloc_failed());
        }
        meter.refund(self.0.buffer.replace(charge));
        Ok(())
    }

    /// Adds an item at the end.
    pub(crate) fn push(&self, v: Value) -> Result<(), Fault> {
        check_item(&v)?;
        self.reserve(1)?;
        self.0.items.borrow_mut().push(v);
        Ok(())
    }

    pub(crate) fn set(&self, index: usize, v: Value) -> Result<(), Fault> {
        check_item(&v)?;
        let mut items = self.0.items.borrow_mut();
        let slot = items.get_mut(index).ok_or_else(index_fault)?;
        *slot = v;
        Ok(())
    }

    pub(crate) fn insert(&self, index: usize, v: Value) -> Result<(), Fault> {
        check_item(&v)?;
        if index > self.len() {
            return Err(index_fault());
        }
        self.reserve(1)?;
        self.0.items.borrow_mut().insert(index, v);
        Ok(())
    }

    pub(crate) fn remove(&self, index: usize) -> Result<Value, Fault> {
        let mut items = self.0.items.borrow_mut();
        if index >= items.len() {
            return Err(index_fault());
        }
        Ok(items.remove(index))
    }

    pub(crate) fn pop(&self) -> Option<Value> {
        self.0.items.borrow_mut().pop()
    }

    fn render_len(&self) -> usize {
        let items = self.items();
        let sep = items.len().saturating_sub(1).saturating_mul(2);
        items.iter().fold(2usize.saturating_add(sep), |n, v| {
            n.saturating_add(v.render_len())
        })
    }

    fn write_to(&self, w: &mut impl Write) -> fmt::Result {
        w.write_char('[')?;
        for (i, v) in self.items().iter().enumerate() {
            if i > 0 {
                w.write_str(", ")?;
            }
            write_scalar(w, v)?;
        }
        w.write_char(']')
    }
}

/// Lists hold no lists: values cannot form cycles, and nothing recurses over them.
fn check_item(v: &Value) -> Result<(), Fault> {
    match v {
        Value::List(_) => Err(Fault::type_error("lists cannot contain lists")),
        _ => Ok(()),
    }
}

pub(crate) fn index_fault() -> Fault {
    Fault::new(crate::ErrorKind::Index, "index out of range")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_two_words() {
        assert_eq!(size_of::<Value>(), 16);
    }

    #[test]
    fn strings_charge_before_and_refund_after() {
        let m = Meter::new(1 << 20);
        let s = Str::concat(&m, &["ab", "cd"]).expect("fits");
        assert_eq!(s.as_str(), "abcd");
        assert!(m.used() >= STR_BOX + 4);
        drop(s);
        assert_eq!(m.used(), 0);
    }

    #[test]
    fn a_string_over_the_cap_is_refused_and_charges_nothing() {
        let m = Meter::new(1000);
        let big = "x".repeat(2000);
        let e = Str::concat(&m, &[&big]).expect_err("over");
        assert_eq!(e.kind, crate::ErrorKind::Memory);
        assert_eq!(m.used(), 0);
    }

    #[test]
    fn list_growth_charges_capacity_and_refunds_on_drop() {
        let m = Meter::new(1 << 20);
        let l = List::with_capacity(&m, 0).expect("fits");
        for i in 0..100 {
            l.push(Value::Int(i)).expect("fits");
        }
        let cap = l.items().capacity();
        assert!(m.used() >= cap * SLOT);
        drop(l);
        assert_eq!(m.used(), 0);
    }

    #[test]
    fn list_growth_reserves_old_and_new_buffers_at_once() {
        let m = Meter::new(1 << 20);
        let l = List::with_capacity(&m, 64).expect("fits");
        let before = m.used();
        for i in 0..65 {
            l.push(Value::Int(i)).expect("fits");
        }
        // The peak covered the old buffer and the doubled one together.
        assert!(m.peak() >= before + 128 * SLOT);
    }

    #[test]
    fn lists_refuse_lists() {
        let m = Meter::new(1 << 20);
        let a = List::with_capacity(&m, 0).expect("fits");
        let e = a.push(Value::List(a.clone())).expect_err("no nesting");
        assert_eq!(e.kind, crate::ErrorKind::Type);
        assert_eq!(a.len(), 0);
    }

    #[test]
    fn rendering_lengths_match_the_text() {
        let m = Meter::new(1 << 20);
        let l = List::with_capacity(&m, 0).expect("fits");
        for v in [
            Value::Int(-12),
            Value::Float(1.0),
            Value::Nil,
            Value::Bool(true),
        ] {
            l.push(v).expect("fits");
        }
        let v = Value::List(l);
        assert_eq!(v.to_string(), "[-12, 1.0, nil, true]");
        assert_eq!(v.render_len(), v.to_string().len());
        assert_eq!(Value::Float(1e300).to_string(), "1e300");
    }
}
