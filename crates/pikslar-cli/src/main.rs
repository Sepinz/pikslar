//! pikslar-cli: Pikslar's exports and scripting, from the command line. It never opens a
//! window or touches the GPU. Exports draw only what the project file holds; `batch` and
//! `session` edit projects through the scripting API, one transaction at a time.

mod api_json;
mod apply;
mod args;
mod batch;
mod export;
mod failure;
mod fields;
mod help;
mod info;
use pikslar_json as json;
mod palette;
mod palette_args;
mod paths;
mod project;
mod requests;
mod results;
mod sandbox;
mod script_args;
mod session;

use args::Command;
use failure::Failure;
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

fn say(text: &str) -> Result<(), Failure> {
    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|e| Failure::Write(format!("Could not write to standard output: {e}")))
}

/// Tells the notes on standard error and the paths written on standard output. The files are
/// written: a closed standard output must not report a failure, or a build script could redo
/// work that is already done.
fn written(done: &export::Done) {
    for note in &done.notes {
        let _ = writeln!(std::io::stderr(), "pikslar-cli: note: {note}");
    }
    let lines: Vec<String> = done
        .written
        .iter()
        .map(|p| format!("{}\n", p.display()))
        .collect();
    let _ = say(&lines.concat());
}

fn run(args: Vec<OsString>) -> Result<(), Failure> {
    match args::parse(args)? {
        Command::Help(command) => say(help::for_command(command.as_deref())),
        Command::Version => say(concat!("pikslar-cli ", env!("CARGO_PKG_VERSION"), "\n")),
        Command::Info { project, json } => {
            let doc = project::load(&project)?;
            say(&if json {
                info::json(&doc)
            } else {
                info::text(&doc)
            })
        }
        Command::Palette(request) => {
            written(&palette::run(&request)?);
            Ok(())
        }
        Command::Export(request) => {
            written(&export::run(&request)?);
            Ok(())
        }
        Command::Batch(request) => {
            let line = batch::run(&request)?;
            // As for an export: the project is written, and a closed standard output must not
            // make a build script run the batch again.
            let _ = say(&line);
            Ok(())
        }
        Command::Session { dir } => session::run(&dir),
    }
}

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            let hint = if failure.code() == 1 {
                "\nSee: pikslar-cli help"
            } else {
                ""
            };
            let _ = writeln!(std::io::stderr(), "pikslar-cli: {failure}{hint}");
            ExitCode::from(failure.code())
        }
    }
}
