//! Arguments read from script values, and replies made into them.

use pikslar_api::ApiError;
use pikslar_script::{HostError, Value};
use std::fmt::{self, Write};

/// What a host function's argument must be, for its error message.
fn wrong(name: &str, at: usize, wanted: &str, got: &Value) -> HostError {
    HostError::new(format!(
        "{name}: argument {} must be {wanted}, not {}",
        at + 1,
        got.type_name()
    ))
}

/// Most bytes of a string argument the binding copies.
pub const MOST_TEXT: usize = 1024;

pub struct Args<'v> {
    pub name: &'static str,
    pub values: &'v [Value],
}

impl<'v> Args<'v> {
    fn at(&self, i: usize) -> Result<&'v Value, HostError> {
        self.values
            .get(i)
            .ok_or_else(|| HostError::new(format!("{}: argument {} is missing", self.name, i + 1)))
    }

    pub fn int(&self, i: usize) -> Result<i64, HostError> {
        let v = self.at(i)?;
        v.as_int()
            .ok_or_else(|| wrong(self.name, i, "an integer", v))
    }

    /// A string of at most [`MOST_TEXT`] bytes, copied for the API. A longer one is refused
    /// before it is copied: no name, blend or title is anywhere near that long.
    pub fn text(&self, i: usize) -> Result<String, HostError> {
        let v = self.at(i)?;
        let text = v
            .as_str()
            .ok_or_else(|| wrong(self.name, i, "a string", v))?;
        self.short(text, i)
    }

    fn short(&self, text: &str, i: usize) -> Result<String, HostError> {
        match text.len() <= MOST_TEXT {
            true => Ok(text.to_string()),
            false => Err(HostError::new(format!(
                "{}: argument {} is longer than {MOST_TEXT} bytes",
                self.name,
                i + 1
            ))),
        }
    }

    /// A string argument as the script holds it, not copied: a host that keeps it keeps it
    /// in the run's memory, where the meter counts it.
    pub fn string(&self, i: usize) -> Result<&'v Value, HostError> {
        let v = self.at(i)?;
        match v.as_str() {
            Some(_) => Ok(v),
            None => Err(wrong(self.name, i, "a string", v)),
        }
    }

    pub fn flag(&self, i: usize) -> Result<bool, HostError> {
        match self.at(i)? {
            Value::Bool(b) => Ok(*b),
            other => Err(wrong(self.name, i, "true or false", other)),
        }
    }

    /// A list of at most `most` strings, each as [`Self::text`] takes one: a longer list is
    /// refused before anything is copied.
    pub fn texts(&self, i: usize, most: usize) -> Result<Vec<String>, HostError> {
        let Value::List(list) = self.at(i)? else {
            return Err(wrong(self.name, i, "a list", self.at(i)?));
        };
        if list.len() > most {
            return Err(HostError::new(format!(
                "{}: argument {} holds at most {most} items",
                self.name,
                i + 1
            )));
        }
        (0..list.len())
            .map(|k| match list.get(k) {
                Some(Value::Str(s)) => self.short(s.as_str(), i),
                _ => Err(HostError::new(format!(
                    "{}: item {k} of argument {} must be a string",
                    self.name,
                    i + 1
                ))),
            })
            .collect()
    }

    /// A list of integers.
    pub fn ints(&self, i: usize) -> Result<Vec<i64>, HostError> {
        let Value::List(list) = self.at(i)? else {
            return Err(wrong(self.name, i, "a list", self.at(i)?));
        };
        (0..list.len())
            .map(|k| match list.get(k) {
                Some(Value::Int(n)) => Ok(n),
                _ => Err(HostError::new(format!(
                    "{}: item {k} of argument {} must be an integer",
                    self.name,
                    i + 1
                ))),
            })
            .collect()
    }
}

/// An integer from a count, id or handle, which all fit.
pub fn int(n: impl TryInto<i64>) -> Value {
    Value::Int(n.try_into().unwrap_or(i64::MAX))
}

/// A place or size inside a document, which always fits.
pub fn as_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// At most this many bytes of a returned value are kept as text.
pub const MOST_RETURNED: usize = 64 << 10;

/// A value as `str()` would write it, cut at [`MOST_RETURNED`] bytes.
pub fn render(value: &Value) -> String {
    let mut out = Capped(String::new());
    let _ = write!(out, "{value}");
    out.0
}

struct Capped(String);

impl Write for Capped {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let room = MOST_RETURNED.saturating_sub(self.0.len());
        if s.len() <= room {
            self.0.push_str(s);
            return Ok(());
        }
        let mut end = room;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        self.0.push_str(&s[..end]);
        Err(fmt::Error)
    }
}

/// The error a refused operation gives a script.
pub fn refusal(name: &str, error: &ApiError) -> HostError {
    HostError::new(format!("{name}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendering_is_capped() {
        assert_eq!(render(&Value::Int(-3)), "-3");
        assert_eq!(render(&Value::Nil), "nil");
    }

    #[test]
    fn arguments_are_checked_by_type() {
        let values = [Value::Int(3), Value::Bool(true)];
        let a = Args {
            name: "f",
            values: &values,
        };
        assert_eq!(a.int(0), Ok(3));
        assert!(a.flag(1).expect("a flag"));
        assert!(a.int(1).is_err());
        assert!(a.text(0).is_err());
        assert!(a.ints(0).is_err());
        assert!(a.int(5).is_err());
    }
}
