//! Pikslar's own scripting language: small, deterministic, and held to hard limits.
//!
//! A script is compiled once against a [`HostTable`] (the only functions it may call outside
//! the language) and run any number of times with [`run`]. Every run is held to its
//! [`Limits`]: fuel for all work, including work inside builtins and host calls; a hard cap
//! on live memory, charged before anything is allocated; a call depth limit; and a cancel
//! flag read at least every [`CANCEL_INTERVAL`] fuel units. The language has no file,
//! network, process, environment, time or system access, and no randomness.
//!
//! See `docs/scripting-language.md`.

mod ast;
mod builtins;
mod error;
mod fuel;
mod host;
mod lexer;
mod limits;
mod meter;
mod ops;
mod parser;
mod value;
mod vm;

pub use ast::Program;
pub use error::{ErrorKind, Pos, ScriptError};
pub use host::{Held, Host, HostCx, HostError, HostId, HostTable, TableError};
pub use limits::{
    alloc_charge, Limits, ALLOC_OVERHEAD, BYTES_PER_FUEL, CANCEL_INTERVAL, MAX_ARGS,
    MAX_CALL_DEPTH, MAX_FUNCTIONS, MAX_LOCALS, MAX_NEST, RECOMMENDED_STACK, STACK_BUDGET,
};
pub use parser::compile;
pub use value::{List, Str, Value};
pub use vm::{run, RunReport};

/// A host with no functions, for scripts that need none.
pub struct NoHost;

impl Host for NoHost {
    fn call(
        &mut self,
        _id: HostId,
        _args: &[Value],
        _cx: &mut HostCx<'_, '_>,
    ) -> Result<Value, HostError> {
        Err(HostError::new("no host functions are available"))
    }
}
