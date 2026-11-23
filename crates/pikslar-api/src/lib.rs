//! Pikslar's scripting API, version 1: plain Rust that any script engine, or the command line's
//! JSON lines, can call.
//!
//! A script runs as one [`run`]: every operation is checked at this boundary, then carried out
//! by the editor's own commands, and everything the run changes is one undo step. If any
//! operation fails, or the run is cancelled, nothing it did remains. Layers and swatches are
//! named by the ids the project file keeps, so a script can name them again after the project
//! is saved and opened. Frames are named by handles that follow them while the program runs.
//! A reference names the object it was given for or is refused, never another object. See
//! `docs/scripting-api.md` and `docs/stable-ids.md`.

mod error;
mod input;
mod op;
mod query;
mod script;

pub use error::ApiError;
pub use op::{DocumentInfo, FrameRef, LayerInfo, LayerRef, Mode, Op, Reply, SwatchRef};
pub use script::{Script, MOST_READ_PIXELS};

use pikslar_core::{Editor, LimitError};
use std::sync::atomic::AtomicBool;

/// The version of the API this build provides.
pub const API_VERSION: u32 = 1;

/// Runs a script asking for API version `version` on the editor's document, as one undo step.
/// Refused, changing nothing, for any version but [`API_VERSION`].
pub fn run<T>(
    editor: &mut Editor,
    version: u32,
    body: impl FnOnce(&mut Script) -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    run_cancellable(editor, version, None, body)
}

/// [`run`], checking `cancel` before every operation: once it is set, the next operation fails
/// with [`ApiError::Cancelled`] and the run is taken back.
pub fn run_cancellable<T>(
    editor: &mut Editor,
    version: u32,
    cancel: Option<&AtomicBool>,
    body: impl FnOnce(&mut Script) -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    run_with(editor, version, cancel, body)
}

/// [`run_cancellable`] for an engine with errors of its own, such as a script interpreter's:
/// any error the body returns takes the run back, as an [`ApiError`] does.
pub fn run_with<T, E: From<ApiError> + From<LimitError>>(
    editor: &mut Editor,
    version: u32,
    cancel: Option<&AtomicBool>,
    body: impl FnOnce(&mut Script) -> Result<T, E>,
) -> Result<T, E> {
    if version != API_VERSION {
        return Err(ApiError::Version {
            wanted: version,
            supported: API_VERSION,
        }
        .into());
    }
    let mut seen = 0;
    let ran = editor.transaction(|scratch| {
        let mut script = Script::new(scratch, cancel);
        let ran = body(&mut script);
        seen = script.seen_color;
        ran
    });
    // Every color id the run was given or read stays retired, whether or not it is kept, so
    // an id a script saw never comes to name another color. Ids it never saw (colors it only
    // painted with, and the retired ones below a new color) are left free.
    editor.doc().colors.retire_up_to(seen);
    ran
}

/// Runs a list of operations as one script, giving back each one's reply.
pub fn run_ops(editor: &mut Editor, version: u32, ops: &[Op]) -> Result<Vec<Reply>, ApiError> {
    run(editor, version, |script| {
        ops.iter().map(|op| script.call(op.clone())).collect()
    })
}
