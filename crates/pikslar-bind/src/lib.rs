//! Runs a Pikslar script on a document: the interpreter (`pikslar-script`) bound to the
//! scripting API (`pikslar-api`).
//!
//! Scripts are untrusted. The only things a script can reach are the host functions listed
//! in [`functions`], each carried out by API operations, which run the editor's own commands.
//! Which of them a run may call is decided per run by its [`Capabilities`]: a function whose
//! capability is not granted is not in the run's host table, so a script naming it does not
//! compile. Nothing else is reachable: no file, network, process, environment, clock or system.
//!
//! A whole run is one API transaction: on success one undo step, and on any error, stop or
//! cancel the document, its history, revision and saved bytes are exactly as before. Every
//! host function burns fuel for its work before doing it. Nothing exists before the first
//! [`run_script`]: no state, thread or allocation. See `docs/scripting-api.md`, "Scripts".

mod binding;
mod caps;
mod cost;
mod edits;
mod files;
mod functions;
mod growth;
mod pending;
mod reads;
mod run;
mod settings;
mod values;

pub use caps::{Capabilities, Capability};
pub use files::FileAccess;
pub use functions::{functions, FunctionInfo};
pub use growth::MOST_BYTES_PER_UNIT;
pub use run::{run_script, run_script_with, Failure, Outcome};
pub use settings::ScriptLimits;
