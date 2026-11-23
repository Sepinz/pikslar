//! Why the program stopped, and the exit code that says so to a script.

use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    /// 1: the arguments do not make sense.
    Usage(String),
    /// 2: the project could not be read.
    Read(String),
    /// 3: the project cannot be exported as asked.
    Export(String),
    /// 4: the output could not be written, or would replace a file without --overwrite.
    Write(String),
}

impl Failure {
    pub fn code(&self) -> u8 {
        match self {
            Failure::Usage(_) => 1,
            Failure::Read(_) => 2,
            Failure::Export(_) => 3,
            Failure::Write(_) => 4,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Usage(m) | Failure::Read(m) | Failure::Export(m) | Failure::Write(m) => {
                f.write_str(m)
            }
        }
    }
}
