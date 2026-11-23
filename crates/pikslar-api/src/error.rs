//! Why a script operation was refused. Any error ends the run and takes back all of it.

use pikslar_core::LimitError;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiError {
    /// The script asked for an API version this build does not provide.
    Version { wanted: u32, supported: u32 },
    /// No layer has this id.
    NoSuchLayer(i64),
    /// No frame has this handle: it was removed, or the handle is from before the project
    /// was opened.
    NoSuchFrame(i64),
    /// No swatch has this color id.
    NoSuchSwatch(i64),
    /// A number outside what the operation takes.
    OutOfRange {
        what: &'static str,
        value: i64,
        min: i64,
        max: i64,
    },
    /// A pixel outside the canvas.
    Outside { x: i64, y: i64 },
    /// A fully transparent color is 0; any other with alpha 0 is a mistake.
    Transparent(i64),
    /// A name that is empty, too long or holds control characters.
    BadName { name: String, why: &'static str },
    /// A blend mode this document's layers cannot take.
    UnknownBlend(String),
    /// The editor refused the command, for the reason given.
    Refused(LimitError),
    /// The run was cancelled.
    Cancelled,
}

impl From<LimitError> for ApiError {
    fn from(error: LimitError) -> Self {
        ApiError::Refused(error)
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Version { wanted, supported } => write!(
                f,
                "API version {wanted} is not available; this Pikslar provides version {supported}"
            ),
            ApiError::NoSuchLayer(id) => write!(f, "no layer has id {id}"),
            ApiError::NoSuchFrame(handle) => write!(
                f,
                "no frame has handle {handle}; it was removed, or is from before the project was opened"
            ),
            ApiError::NoSuchSwatch(id) => write!(f, "no swatch has color id {id}"),
            ApiError::OutOfRange {
                what,
                value,
                min,
                max,
            } => write!(f, "{what} must be from {min} to {max}, not {value}"),
            ApiError::Outside { x, y } => write!(f, "pixel ({x}, {y}) is outside the canvas"),
            ApiError::Transparent(color) => write!(
                f,
                "color {color:#010x} has alpha 0 but is not 0; a transparent color is 0"
            ),
            ApiError::BadName { name, why } => write!(f, "the name {name:?} {why}"),
            ApiError::UnknownBlend(name) => {
                write!(
                    f,
                    "{name:?} is not a blend mode this document's layers take"
                )
            }
            ApiError::Refused(error) => f.write_str(error.text()),
            ApiError::Cancelled => f.write_str("the script was cancelled"),
        }
    }
}

impl std::error::Error for ApiError {}
