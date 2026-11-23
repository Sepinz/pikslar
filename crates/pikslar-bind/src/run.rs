//! One script run: compiled and run on a thread of its own, inside one API transaction.

use crate::binding::{Binding, WAITING};
use crate::caps::Capabilities;
use crate::files::FileAccess;
use crate::functions::granted;
use crate::settings::ScriptLimits;
use crate::values::render;
use pikslar_api::{run_with, ApiError, API_VERSION};
use pikslar_core::{Editor, LimitError};
use pikslar_plugin::files::FileError;
use pikslar_script::{compile, ErrorKind, Pos, ScriptError, RECOMMENDED_STACK};
use std::fmt;
use std::sync::atomic::AtomicBool;

/// Why a run left the document as it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The script did not compile, made a mistake, or was stopped by a limit or a cancel.
    Script(ScriptError),
    /// The API refused an operation; `at` is the call that asked for it, or `None` when
    /// pixels still waiting at the end of the run were refused. `waiting` is true when what
    /// was refused was writing pixels `set_pixel` had kept waiting: then `at` is the later
    /// call that wrote them.
    Refused {
        error: ApiError,
        at: Option<Pos>,
        waiting: bool,
    },
    /// The run's thread could not be started.
    Thread(String),
    /// The script succeeded, but the files it asked to write could not all be written (or
    /// the run was cancelled as they were): the document was not changed, and nor was any
    /// file, except after [`FileError::NotPutBack`], when a file already written could not be
    /// taken back (its old contents are kept beside it).
    Files(FileError),
}

impl Failure {
    /// True for a stop by fuel, memory, call depth or cancel, not a mistake.
    pub fn is_stop(&self) -> bool {
        self.kind().is_some_and(ErrorKind::is_stop)
    }

    /// True when the run was cancelled by its caller.
    pub fn is_cancel(&self) -> bool {
        self.kind() == Some(ErrorKind::Cancelled)
    }

    pub fn kind(&self) -> Option<ErrorKind> {
        match self {
            Failure::Script(e) => Some(e.kind),
            // A cancel the API saw before the interpreter did is a cancel all the same.
            Failure::Refused {
                error: ApiError::Cancelled,
                ..
            } => Some(ErrorKind::Cancelled),
            Failure::Files(FileError::Cancelled) => Some(ErrorKind::Cancelled),
            _ => None,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Script(e) => write!(f, "{e}"),
            Failure::Refused {
                error,
                at: Some(p),
                waiting,
            } => {
                write!(f, "line {}, column {}: ", p.line, p.column)?;
                if *waiting {
                    write!(f, "{WAITING}: ")?;
                }
                write!(f, "{error}")
            }
            Failure::Refused {
                error, at: None, ..
            } => write!(f, "at the end of the script: {error}"),
            Failure::Thread(why) => write!(f, "the script could not start: {why}"),
            Failure::Files(e @ FileError::NotPutBack) => write!(f, "at the end of the script: {e}"),
            Failure::Files(e) => write!(f, "at the end of the script: no file was written: {e}"),
        }
    }
}

impl From<ApiError> for Failure {
    fn from(error: ApiError) -> Failure {
        Failure::Refused {
            error,
            at: None,
            waiting: false,
        }
    }
}

impl From<LimitError> for Failure {
    fn from(error: LimitError) -> Failure {
        Failure::from(ApiError::Refused(error))
    }
}

/// What a run did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// What the script returned, as `str()` writes it (at most 64 KiB), or why nothing was
    /// kept.
    pub result: Result<String, Failure>,
    /// What it printed, at most 64 KiB.
    pub printed: String,
    pub fuel_used: u64,
    /// The most memory the script held at once, in bytes.
    pub peak_memory: usize,
    /// The files the run saved were written. Then a `result` that is still an error (from
    /// keeping the document's step, which can no longer fail in any way we know of) means
    /// the document was left as it was but the files were not.
    pub files_written: bool,
}

/// Runs `source` on the editor's document as one undo step, with the host functions `caps`
/// grants, held to `limits`. Setting `cancel` stops the run within a bounded amount of work
/// and takes it back. Blocks until the run is over; the run itself is on a thread of its own
/// with [`RECOMMENDED_STACK`] of stack, started for this run and ended with it.
pub fn run_script(
    editor: &mut Editor,
    source: &str,
    caps: Capabilities,
    limits: ScriptLimits,
    cancel: &AtomicBool,
) -> Outcome {
    run_script_with(editor, source, caps, limits, cancel, None)
}

/// [`run_script`] for a plugin granted files: `files` holds what it may ask the user for,
/// and is dropped with the run, so no handle outlives it.
pub fn run_script_with(
    editor: &mut Editor,
    source: &str,
    caps: Capabilities,
    limits: ScriptLimits,
    cancel: &AtomicBool,
    files: Option<FileAccess>,
) -> Outcome {
    let thread = std::thread::Builder::new()
        .name("pikslar script".into())
        .stack_size(RECOMMENDED_STACK);
    std::thread::scope(|s| {
        let run = thread.spawn_scoped(s, || run_here(editor, source, caps, limits, cancel, files));
        match run.map(|handle| handle.join()) {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => failed(Failure::Thread("the script's thread stopped".into())),
            Err(e) => failed(Failure::Thread(e.to_string())),
        }
    })
}

fn failed(failure: Failure) -> Outcome {
    Outcome {
        result: Err(failure),
        printed: String::new(),
        fuel_used: 0,
        peak_memory: 0,
        files_written: false,
    }
}

/// The run itself, on the run's thread.
fn run_here(
    editor: &mut Editor,
    source: &str,
    caps: Capabilities,
    limits: ScriptLimits,
    cancel: &AtomicBool,
    files: Option<FileAccess>,
) -> Outcome {
    let growth = limits.growth();
    let limits = limits.interpreter();
    let (table, fns) = match granted(caps) {
        Ok(granted) => granted,
        Err(e) => return failed(Failure::Thread(e.to_string())),
    };
    let program = match compile(source, &table, &limits) {
        Ok(p) => p,
        Err(e) => return failed(Failure::Script(e)),
    };
    let mut used = (0, 0, String::new(), false);
    let result = run_with(editor, API_VERSION, None, |script| {
        let mut binding = Binding::new(script, fns);
        binding.files = files;
        binding.growth.budget = growth;
        let report = pikslar_script::run(&program, &mut binding, &limits, cancel);
        used.0 = report.fuel_used;
        used.1 = report.peak_memory;
        let ran = match report.result {
            Ok(value) => finish(&mut binding, &value, cancel),
            Err(e) => Err(Fail::Script(e)),
        };
        used.2 = std::mem::take(&mut binding.printed);
        used.3 = binding.held.written;
        let refused = binding.refused.take().map(|r| (r, binding.waiting_refused));
        ran.map_err(|e| failure(e, refused))
    });
    Outcome {
        result,
        printed: used.2,
        fuel_used: used.0,
        peak_memory: used.1,
        files_written: used.3,
    }
}

/// After a script that returned `value`: its waiting pixels written, the growth checked, and
/// the files it saved written, in that order.
fn finish(
    binding: &mut Binding<'_, '_>,
    value: &pikslar_script::Value,
    cancel: &AtomicBool,
) -> Result<String, Fail> {
    binding.flush().map_err(Fail::from)?;
    binding.check_growth_at_end().map_err(|_| Fail::Grown)?;
    let kept = render(value);
    // Last, so that nothing in the run can fail it once files are written. Keeping the step
    // afterwards (`run_with`) can no longer fail in any way we know of; if it ever did,
    // `files_written` says the files were written all the same.
    crate::files::commit(binding, cancel).map_err(Fail::Files)?;
    Ok(kept)
}

/// Either the script's own error, or the API's refusal it stands for.
fn failure(error: Fail, refused: Option<(ApiError, bool)>) -> Failure {
    match (error, refused) {
        (Fail::Script(e), Some((r, waiting))) if e.kind == ErrorKind::Host => Failure::Refused {
            error: r,
            at: Some(e.pos),
            waiting,
        },
        (Fail::Script(e), _) => Failure::Script(e),
        (Fail::Flush, Some((r, _))) => Failure::Refused {
            error: r,
            at: None,
            waiting: true,
        },
        (Fail::Flush, None) => Failure::Refused {
            error: ApiError::Refused(LimitError::NoSuchItem),
            at: None,
            waiting: true,
        },
        (Fail::Files(e), _) => Failure::Files(e),
        // Grown past the budget by the pixels written at the end: no call to name.
        (Fail::Grown, _) => Failure::Refused {
            error: ApiError::Refused(LimitError::NoMemory),
            at: None,
            waiting: false,
        },
    }
}

/// How the body of a run failed, before it is told apart.
enum Fail {
    Script(ScriptError),
    Flush,
    Grown,
    Files(FileError),
}

impl From<ScriptError> for Fail {
    fn from(e: ScriptError) -> Fail {
        Fail::Script(e)
    }
}

impl From<pikslar_script::HostError> for Fail {
    fn from(_: pikslar_script::HostError) -> Fail {
        Fail::Flush
    }
}
