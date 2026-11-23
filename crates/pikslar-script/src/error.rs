//! Script errors. Every error names the line and column it happened at.

use std::borrow::Cow;
use std::fmt;

/// A place in the source: 1-based line, and 1-based column counted in characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: u32,
    pub column: u32,
}

impl Pos {
    pub const START: Pos = Pos { line: 1, column: 1 };
}

/// What went wrong. Stops (fuel, memory, depth, cancel) are distinct from mistakes in the
/// script, so a host can tell "the script is wrong" from "the script was stopped".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The source does not parse, or names something that does not exist.
    Syntax,
    /// The source or its parse tree is larger than the limits allow.
    TooLarge,
    /// A value of the wrong type for an operation.
    Type,
    /// Integer overflow, division by zero, or a float result that is not finite.
    Arithmetic,
    /// An index or slice outside a string or list.
    Index,
    /// Any other misuse of a builtin, such as popping an empty list.
    Value,
    /// The fuel allowance ran out.
    Fuel,
    /// The memory cap would have been passed. Nothing was allocated.
    Memory,
    /// The memory was charged within the cap, but the system could not allocate it.
    AllocFailed,
    /// Calls nested deeper than the call depth limit.
    CallDepth,
    /// The host's cancel flag was set.
    Cancelled,
    /// A host function reported an error.
    Host,
}

impl ErrorKind {
    /// True for the kinds that mean the run was stopped by a limit or by the host, not by a
    /// mistake in the script.
    pub fn is_stop(self) -> bool {
        matches!(
            self,
            ErrorKind::Fuel
                | ErrorKind::Memory
                | ErrorKind::AllocFailed
                | ErrorKind::CallDepth
                | ErrorKind::Cancelled
        )
    }
}

/// An error from compiling or running a script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    pub kind: ErrorKind,
    pub pos: Pos,
    pub message: String,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "line {}, column {}: {}",
            self.pos.line, self.pos.column, self.message
        )
    }
}

impl std::error::Error for ScriptError {}

/// An error not yet tied to a place. The node that ran the failing operation adds its place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fault {
    pub kind: ErrorKind,
    pub message: Cow<'static, str>,
}

impl Fault {
    pub fn new(kind: ErrorKind, message: impl Into<Cow<'static, str>>) -> Fault {
        Fault {
            kind,
            message: message.into(),
        }
    }

    pub fn type_error(message: impl Into<Cow<'static, str>>) -> Fault {
        Fault::new(ErrorKind::Type, message)
    }

    pub fn memory() -> Fault {
        Fault::new(ErrorKind::Memory, "the memory cap was reached")
    }

    pub fn alloc_failed() -> Fault {
        Fault::new(ErrorKind::AllocFailed, "memory could not be allocated")
    }

    pub fn overflow() -> Fault {
        Fault::new(ErrorKind::Arithmetic, "integer overflow")
    }

    pub fn at(self, pos: Pos) -> Box<ScriptError> {
        Box::new(ScriptError {
            kind: self.kind,
            pos,
            message: capped(self.message.into_owned()),
        })
    }
}

/// The error type inside the interpreter: boxed, so results stay small on the hot path.
pub(crate) type Trap = Box<ScriptError>;

pub(crate) fn syntax(pos: Pos, message: impl Into<String>) -> Trap {
    Box::new(ScriptError {
        kind: ErrorKind::Syntax,
        pos,
        message: capped(message.into()),
    })
}

/// Most bytes of an error message. Messages can carry what a script wrote (a name, a
/// string), and are shown and copied by hosts, so they are cut at the source.
pub const MOST_MESSAGE: usize = 512;
/// Most characters of a name or other script text quoted in a message.
pub const MOST_QUOTED: usize = 64;

/// `text` as a message quotes it: at most [`MOST_QUOTED`] characters, then an ellipsis.
pub fn quoted(text: &str) -> Cow<'_, str> {
    match text.char_indices().nth(MOST_QUOTED) {
        None => Cow::Borrowed(text),
        Some((end, _)) => Cow::Owned(format!("{}\u{2026}", &text[..end])),
    }
}

/// `message` cut to at most [`MOST_MESSAGE`] bytes on a character boundary, with an
/// ellipsis where it was cut.
pub fn capped(mut message: String) -> String {
    if message.len() > MOST_MESSAGE {
        let mut end = MOST_MESSAGE - '\u{2026}'.len_utf8();
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.push('\u{2026}');
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_text_and_messages_are_cut() {
        assert_eq!(quoted("short"), "short");
        let q = quoted(&"é".repeat(100)).into_owned();
        assert_eq!(q.chars().count(), MOST_QUOTED + 1);
        assert!(q.ends_with('\u{2026}'));
        let m = capped("ñ".repeat(1000));
        assert!(m.len() <= MOST_MESSAGE && m.ends_with('\u{2026}'));
        assert_eq!(capped("fine".into()), "fine");
    }
}
