//! `batch`: opens a project, runs a file of scripting API operations on it as one transaction,
//! and saves it, or writes a new project with `--out`. If any operation is refused, or the file
//! is not exactly what is expected, nothing is written.

use crate::apply::{self, Planned, Ran};
use crate::failure::Failure;
use crate::fields::{Fields, Refusal};
use crate::json::{self, Value};
use crate::paths::{check_name, check_target, require_extension};
use crate::project;
use crate::script_args::{BatchArgs, Source};
use pikslar_core::Editor;
use std::io::Read;
use std::path::Path;

/// The largest operations file: the same as the longest request line of a session.
pub const MAX_BATCH_BYTES: u64 = 1 << 20;

/// Runs the batch, giving back the line it prints: each operation's reply, whether anything
/// changed and the file written, if any.
pub fn run(args: &BatchArgs) -> Result<String, Failure> {
    if let Some(out) = &args.out {
        check_name(out)?;
        require_extension(out, "piks")?;
        check_target(out, &args.project, args.overwrite)?;
    }
    let (version, ops) = read_ops(&args.ops)?;
    let (doc, lossless) = project::load_noted(&args.project)?;
    if args.out.is_none() {
        if let Some(why) = project::write_over_refusal(&args.project, lossless) {
            return Err(Failure::Export(format!(
                "{why}. Give --out to write a new file and keep this one"
            )));
        }
    }
    let mut editor = Editor::new(doc);
    let ran = apply::run(&mut editor, version, &ops).map_err(refused)?;
    let target = args.out.as_deref().unwrap_or(&args.project);
    let write = args.out.is_some() || ran.changed;
    if write {
        project::save(target, editor.doc())?;
    }
    Ok(report(&ran, write.then_some(target)))
}

fn refused(refusal: Refusal) -> Failure {
    Failure::Export(match refusal.at {
        Some(at) => format!("operation {at} ({}): {}", refusal.code, refusal.message),
        None => refusal.message,
    })
}

fn report(ran: &Ran, written: Option<&Path>) -> String {
    let written = written.map_or(Value::Null, |p| Value::Str(p.display().to_string()));
    let line = Value::Object(vec![
        ("results".into(), Value::Array(ran.results.clone())),
        ("changed".into(), Value::Bool(ran.changed)),
        ("written".into(), written),
    ]);
    format!("{}\n", json::to_text(&line))
}

fn shown(source: &Source) -> String {
    match source {
        Source::Stdin => "standard input".into(),
        Source::File(path) => path.display().to_string(),
    }
}

/// The bytes of the operations, refused past [`MAX_BATCH_BYTES`].
fn read_bytes(source: &Source) -> Result<Vec<u8>, Failure> {
    let fail = |e: &dyn std::fmt::Display| Failure::Read(format!("{}: {e}", shown(source)));
    let mut bytes = Vec::new();
    let read = match source {
        Source::Stdin => std::io::stdin()
            .lock()
            .take(MAX_BATCH_BYTES + 1)
            .read_to_end(&mut bytes),
        Source::File(path) if path.is_dir() => return Err(fail(&"is a folder")),
        Source::File(path) => std::fs::File::open(path)
            .and_then(|f| f.take(MAX_BATCH_BYTES + 1).read_to_end(&mut bytes)),
    };
    read.map_err(|e| fail(&e))?;
    if bytes.len() as u64 > MAX_BATCH_BYTES {
        return Err(fail(&format!(
            "the operations are longer than {MAX_BATCH_BYTES} bytes"
        )));
    }
    Ok(bytes)
}

/// `{"api": 1, "ops": [...]}`, every operation read before any runs.
fn read_ops(source: &Source) -> Result<(i64, Vec<Planned>), Failure> {
    let bytes = read_bytes(source)?;
    let fail = |what: String| Failure::Read(format!("{}: {what}", shown(source)));
    let value = json::parse(&bytes, &json::LIMITS).map_err(|e| fail(e.to_string()))?;
    let bad = |r: Refusal| match r.at {
        Some(at) => fail(format!("operation {at}: {}", r.message)),
        None => fail(r.message),
    };
    let mut fields = Fields::of(&value, "a batch").map_err(bad)?;
    let version = fields.int("api").map_err(bad)?;
    let list = fields.list("ops").map_err(bad)?;
    fields.finish().map_err(bad)?;
    let ops = apply::read_ops(list).map_err(bad)?;
    Ok((version, ops))
}
