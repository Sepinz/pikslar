//! A small, strict JSON reader and writer: the JSON lines session and batch files, and plugin
//! manifests.
//!
//! Strict means that anything a careless or hostile tool could send is refused with a reason,
//! never guessed at: invalid UTF-8, a byte order mark, duplicate keys, trailing characters,
//! raw control characters, lone surrogate escapes, and numbers that are not whole or do not
//! fit an `i64` (`1.5`, `1e2`, `-0`, `NaN`, `Infinity`, `1e400`). Every reader limit, depth,
//! string length and items in an array or object, is checked as the text is read, so no input
//! can make it recurse deeply or hold more than the line it came in.

mod read;
mod write;

#[cfg(test)]
mod tests;

pub use read::parse;
pub use write::to_text;

/// A JSON value with whole numbers only: the protocol has no use for fractions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Array(Vec<Value>),
    /// Members in the order given. Keys are unique: a duplicate is refused when read.
    Object(Vec<(String, Value)>),
}

impl Value {
    /// What kind of value this is, in words for an error message.
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "true or false",
            Value::Int(_) => "a number",
            Value::Str(_) => "a string",
            Value::Array(_) => "an array",
            Value::Object(_) => "an object",
        }
    }
}

/// How much one text may hold.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Arrays and objects inside one another, the outermost counting as 1.
    pub depth: usize,
    /// Characters in one string, after escapes are read.
    pub string_chars: usize,
    pub array_items: usize,
    pub object_members: usize,
}

/// The limits of a request line or a batch file.
pub const LIMITS: Limits = Limits {
    depth: 8,
    string_chars: 4096,
    array_items: 10_000,
    object_members: 32,
};

/// Why a text is not JSON this reader takes, and the byte where that was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    pub at: usize,
    pub what: String,
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "at byte {}: {}", self.at, self.what)
    }
}
