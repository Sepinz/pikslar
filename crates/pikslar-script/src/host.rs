//! The only way out of a script: functions the embedder lists in a [`HostTable`] and carries
//! out in its [`Host`]. The language itself has no file, network, process, environment, time
//! or system access.

use crate::error::Fault;
use crate::fuel::Fuel;
use crate::limits::MAX_ARGS;
use crate::meter::Meter;
use crate::value::{List, Str, Value};
use std::fmt;
use std::rc::Rc;

/// A host function's number: its place in the [`HostTable`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostId(pub u16);

/// Why a function could not be added to a [`HostTable`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableError {
    /// Not a name a script can write: ASCII letters, digits and `_`, not starting with a digit.
    BadName(String),
    /// A keyword or a builtin's name.
    Reserved(String),
    Duplicate(String),
    TooManyArgs(String),
    Full,
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TableError::BadName(n) => write!(f, "'{n}' is not a valid function name"),
            TableError::Reserved(n) => write!(f, "'{n}' is reserved by the language"),
            TableError::Duplicate(n) => write!(f, "'{n}' is already in the table"),
            TableError::TooManyArgs(n) => write!(f, "'{n}' takes more than {MAX_ARGS} arguments"),
            TableError::Full => f.write_str("the host table is full"),
        }
    }
}

impl std::error::Error for TableError {}

/// The host functions a script may call, with how many arguments each takes. A script can
/// name only what is listed here.
#[derive(Clone, Debug, Default)]
pub struct HostTable {
    fns: Vec<(String, usize)>,
}

impl HostTable {
    pub fn new() -> HostTable {
        HostTable::default()
    }

    /// Lists `name`, taking `arity` arguments, and returns the id calls will carry.
    pub fn add(&mut self, name: &str, arity: usize) -> Result<HostId, TableError> {
        if !is_name(name) {
            return Err(TableError::BadName(name.into()));
        }
        if crate::lexer::is_keyword(name) || crate::builtins::lookup(name).is_some() {
            return Err(TableError::Reserved(name.into()));
        }
        if self.lookup(name).is_some() {
            return Err(TableError::Duplicate(name.into()));
        }
        if arity > MAX_ARGS {
            return Err(TableError::TooManyArgs(name.into()));
        }
        let id = u16::try_from(self.fns.len()).map_err(|_| TableError::Full)?;
        self.fns.push((name.into(), arity));
        Ok(HostId(id))
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<(u16, usize)> {
        let i = self.fns.iter().position(|(n, _)| n == name)?;
        Some((i as u16, self.fns[i].1))
    }
}

fn is_name(s: &str) -> bool {
    let mut b = s.bytes();
    b.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && b.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// What carries out host calls for a run.
pub trait Host {
    /// Runs host function `id` with arguments already checked for count. Work that grows
    /// with its input (a fill over many pixels, a command over many layers) must be paid for
    /// with [`HostCx::charge`] before it is done, and strings or lists it returns must be
    /// made with [`HostCx`], so the memory cap holds.
    fn call(
        &mut self,
        id: HostId,
        args: &[Value],
        cx: &mut HostCx<'_, '_>,
    ) -> Result<Value, HostError>;
}

/// The run's fuel and memory, lent to a host call.
pub struct HostCx<'a, 'c> {
    pub(crate) fuel: &'a mut Fuel<'c>,
    pub(crate) meter: &'a Rc<Meter>,
}

impl HostCx<'_, '_> {
    /// Burns `units` of fuel for work the host is about to do. On an error the host should
    /// return it as it is: the run then stops with fuel exhausted or cancelled.
    pub fn charge(&mut self, units: u64) -> Result<(), HostError> {
        self.fuel.burn(units).map_err(HostError::stop)
    }

    pub fn fuel_left(&self) -> u64 {
        self.fuel.left()
    }

    /// A string value, charged to the run's memory.
    pub fn new_str(&mut self, text: &str) -> Result<Value, HostError> {
        self.charge(crate::limits::byte_fuel(text.len()))?;
        Str::concat(self.meter, &[text])
            .map(Value::Str)
            .map_err(HostError::stop)
    }

    /// A list value holding `items` (scalars and strings), charged to the run's memory.
    pub fn new_list(&mut self, items: &[Value]) -> Result<Value, HostError> {
        self.charge(crate::limits::item_fuel(items.len()))?;
        let list = List::with_capacity(self.meter, items.len()).map_err(HostError::stop)?;
        for v in items {
            list.push(v.clone()).map_err(HostError::stop)?;
        }
        Ok(Value::List(list))
    }

    /// A list of integers, charged to the run's fuel and memory before it is made, with no
    /// copy of the items held outside the run's memory.
    pub fn new_int_list(&mut self, items: &[u32]) -> Result<Value, HostError> {
        self.charge(crate::limits::item_fuel(items.len()))?;
        let list = List::with_capacity(self.meter, items.len()).map_err(HostError::stop)?;
        for &v in items {
            list.push(Value::Int(i64::from(v)))
                .map_err(HostError::stop)?;
        }
        Ok(Value::List(list))
    }

    /// Stops the run as the memory cap does unless a string of `bytes` would fit in the
    /// run's memory now: a host can refuse to work on data it has before it looks at it.
    pub fn need_room(&self, bytes: usize) -> Result<(), HostError> {
        match crate::limits::alloc_charge(bytes).is_some_and(|b| self.meter.fits(b)) {
            true => Ok(()),
            false => Err(HostError::stop(Fault::memory())),
        }
    }

    /// True when `items` list items would fit in the run's memory now. A host can refuse work
    /// that would make a list too large before doing the work.
    pub fn list_fits(&self, items: usize) -> bool {
        items
            .checked_mul(std::mem::size_of::<Value>())
            .is_some_and(|bytes| self.meter.fits(bytes))
    }

    /// Charges `bytes` the host is about to allocate outside the script's values (a copy of
    /// an argument, a result before it becomes a list) to the run's memory, for as long as
    /// the returned [`Held`] lives. Stops the run as the memory cap does when they would not
    /// fit, before anything is allocated.
    pub fn hold(&self, bytes: usize) -> Result<Held, HostError> {
        let cost = self.meter.charge_alloc(bytes).map_err(HostError::stop)?;
        Ok(Held {
            meter: Rc::clone(self.meter),
            cost,
        })
    }
}

/// Host memory charged to a run by [`HostCx::hold`], given back when dropped.
#[must_use = "the charge is given back as soon as this is dropped"]
pub struct Held {
    meter: Rc<Meter>,
    cost: usize,
}

impl Drop for Held {
    fn drop(&mut self) {
        self.meter.refund(self.cost);
    }
}

/// An error from a host function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostError(pub(crate) Repr);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Repr {
    Message(String),
    Stop(Fault),
}

impl HostError {
    /// An error the script sees as a host error with this message.
    pub fn new(message: impl Into<String>) -> HostError {
        HostError(Repr::Message(crate::error::capped(message.into())))
    }

    fn stop(f: Fault) -> HostError {
        HostError(Repr::Stop(f))
    }

    pub(crate) fn into_fault(self) -> Fault {
        match self.0 {
            Repr::Message(m) => Fault::new(crate::ErrorKind::Host, m),
            Repr::Stop(f) => f,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        let mut t = HostTable::new();
        assert_eq!(t.add("set_pixel", 3), Ok(HostId(0)));
        assert_eq!(t.add("get_pixel", 2), Ok(HostId(1)));
        assert!(matches!(
            t.add("set_pixel", 3),
            Err(TableError::Duplicate(_))
        ));
        assert!(matches!(t.add("9x", 0), Err(TableError::BadName(_))));
        assert!(matches!(t.add("a-b", 0), Err(TableError::BadName(_))));
        assert!(matches!(t.add("", 0), Err(TableError::BadName(_))));
        assert!(matches!(t.add("while", 0), Err(TableError::Reserved(_))));
        assert!(matches!(t.add("len", 1), Err(TableError::Reserved(_))));
        assert!(matches!(t.add("many", 17), Err(TableError::TooManyArgs(_))));
        assert_eq!(t.lookup("get_pixel"), Some((1, 2)));
    }

    #[test]
    fn host_copies_are_charged_while_held_and_given_back() {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut fuel = Fuel::new(1000, &cancel);
        let meter = Meter::new(1000);
        let cx = HostCx {
            fuel: &mut fuel,
            meter: &meter,
        };
        let held = cx.hold(600).expect("fits");
        assert!(meter.used() >= 600);
        assert!(cx.hold(600).is_err(), "both would pass the cap");
        drop(held);
        assert_eq!(meter.used(), 0);
        assert!(cx.hold(usize::MAX).is_err());
        assert_eq!(meter.used(), 0);
    }
}
