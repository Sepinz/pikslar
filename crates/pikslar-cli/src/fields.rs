//! Reading the members of a request or operation object: each one taken by name with the type
//! it must have, and any left over refused, so a misspelt field is never silently ignored.

use crate::json::Value;

/// Why a request was not carried out, as a reply says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// A short stable word a tool can match on, such as `bad_request`.
    pub code: &'static str,
    pub message: String,
    /// The place of the operation in a list that caused it, from 0.
    pub at: Option<usize>,
}

impl Refusal {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Refusal {
            code,
            message: message.into(),
            at: None,
        }
    }

    pub fn bad(message: impl Into<String>) -> Self {
        Refusal::new("bad_request", message)
    }

    pub fn at(self, at: usize) -> Self {
        Refusal {
            at: Some(at),
            ..self
        }
    }
}

/// A member that is a whole number, or an object for the caller to read.
pub enum IntOr<'a> {
    Int(i64),
    Object(&'a Value),
}

/// The members of one object, each to be taken once.
pub struct Fields<'a> {
    what: String,
    members: &'a [(String, Value)],
    used: Vec<bool>,
}

impl<'a> Fields<'a> {
    /// `what` names the object in messages, such as "a fill operation".
    pub fn of(value: &'a Value, what: &str) -> Result<Self, Refusal> {
        match value {
            Value::Object(members) => Ok(Fields {
                what: what.into(),
                members,
                used: vec![false; members.len()],
            }),
            other => Err(Refusal::bad(format!(
                "{what} must be an object, not {}",
                other.kind()
            ))),
        }
    }

    /// Names the object anew, once its kind is known.
    pub fn named(self, what: &str) -> Self {
        Fields {
            what: what.into(),
            ..self
        }
    }

    fn get(&mut self, key: &str) -> Option<&'a Value> {
        let at = self.members.iter().position(|(k, _)| k == key)?;
        if let Some(used) = self.used.get_mut(at) {
            *used = true;
        }
        self.members.get(at).map(|(_, v)| v)
    }

    /// What the object is, in words for an error message.
    pub fn what(&self) -> &str {
        &self.what
    }

    fn wrong(&self, key: &str, wanted: &str, found: &Value) -> Refusal {
        Refusal::bad(format!(
            "\"{key}\" in {} must be {wanted}, not {}",
            self.what,
            found.kind()
        ))
    }

    fn missing(&self, key: &str) -> Refusal {
        Refusal::bad(format!("{} needs \"{key}\"", self.what))
    }

    /// A member as it was given, of any type, if it was.
    pub fn raw(&mut self, key: &str) -> Option<&'a Value> {
        self.get(key)
    }

    /// A member that may be left out or given as null.
    fn optional(&mut self, key: &str) -> Option<&'a Value> {
        self.get(key).filter(|v| **v != Value::Null)
    }

    pub fn int(&mut self, key: &str) -> Result<i64, Refusal> {
        self.opt_int(key)?.ok_or_else(|| self.missing(key))
    }

    pub fn opt_int(&mut self, key: &str) -> Result<Option<i64>, Refusal> {
        match self.optional(key) {
            None => Ok(None),
            Some(Value::Int(n)) => Ok(Some(*n)),
            Some(other) => Err(self.wrong(key, "a whole number", other)),
        }
    }

    /// A whole number or an object, which may be left out or given as null.
    pub fn opt_int_or_object(
        &mut self,
        key: &str,
        wanted: &str,
    ) -> Result<Option<IntOr<'a>>, Refusal> {
        match self.optional(key) {
            None => Ok(None),
            Some(Value::Int(n)) => Ok(Some(IntOr::Int(*n))),
            Some(v @ Value::Object(_)) => Ok(Some(IntOr::Object(v))),
            Some(other) => Err(self.wrong(key, wanted, other)),
        }
    }

    /// A whole number or an object.
    pub fn int_or_object(&mut self, key: &str, wanted: &str) -> Result<IntOr<'a>, Refusal> {
        self.opt_int_or_object(key, wanted)?
            .ok_or_else(|| self.missing(key))
    }

    pub fn text(&mut self, key: &str) -> Result<&'a str, Refusal> {
        self.opt_text(key)?.ok_or_else(|| self.missing(key))
    }

    pub fn opt_text(&mut self, key: &str) -> Result<Option<&'a str>, Refusal> {
        match self.optional(key) {
            None => Ok(None),
            Some(Value::Str(s)) => Ok(Some(s.as_str())),
            Some(other) => Err(self.wrong(key, "a string", other)),
        }
    }

    pub fn flag(&mut self, key: &str) -> Result<bool, Refusal> {
        self.opt_flag(key)?.ok_or_else(|| self.missing(key))
    }

    pub fn opt_flag(&mut self, key: &str) -> Result<Option<bool>, Refusal> {
        match self.optional(key) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(other) => Err(self.wrong(key, "true or false", other)),
        }
    }

    /// `[x, y]`.
    pub fn point(&mut self, key: &str) -> Result<(i64, i64), Refusal> {
        match self.optional(key) {
            None => Err(self.missing(key)),
            Some(Value::Array(items)) => match items.as_slice() {
                [Value::Int(x), Value::Int(y)] => Ok((*x, *y)),
                _ => Err(Refusal::bad(format!(
                    "\"{key}\" in {} must be [x, y], two whole numbers",
                    self.what
                ))),
            },
            Some(other) => Err(self.wrong(key, "[x, y]", other)),
        }
    }

    pub fn list(&mut self, key: &str) -> Result<&'a [Value], Refusal> {
        match self.optional(key) {
            None => Err(self.missing(key)),
            Some(Value::Array(items)) => Ok(items),
            Some(other) => Err(self.wrong(key, "an array", other)),
        }
    }

    /// Refuses any member not taken.
    pub fn finish(self) -> Result<(), Refusal> {
        let unused = self
            .members
            .iter()
            .zip(&self.used)
            .find(|(_, used)| !**used);
        match unused {
            Some(((key, _), _)) => Err(Refusal::bad(format!(
                "{} has no field \"{key}\"",
                self.what
            ))),
            None => Ok(()),
        }
    }
}
