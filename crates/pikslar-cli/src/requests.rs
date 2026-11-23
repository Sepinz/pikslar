//! One JSON lines session's requests: `hello`, `open`, `run`, `save` and `close`. Each request
//! gets exactly one reply, and a refused request changes nothing, in memory or on disk.
//!
//! A `run` is one transaction through the scripting API: one undo step if every operation is
//! done, nothing at all if any is refused. Nothing reaches a file until a `save`.

use crate::api_json;
use crate::apply::{self, BUDGET, MAX_OPS};
use crate::fields::{Fields, Refusal};
use crate::json::{self, Value, LIMITS};
use crate::paths::{check_target, require_extension};
use crate::project;
use crate::sandbox::Sandbox;
use pikslar_api::API_VERSION;
use pikslar_core::Editor;
use std::path::PathBuf;

/// The version of the request protocol itself, apart from the API's.
pub const PROTOCOL: i64 = 1;
/// The longest request line, in bytes, not counting its line break.
pub const MAX_LINE_BYTES: usize = 1 << 20;
/// The longest request id string.
const MAX_ID_CHARS: usize = 256;

/// The project a session has open.
struct Open {
    editor: Editor,
    /// Where it is, every link resolved.
    path: PathBuf,
    /// The revision when it was last opened or saved.
    saved: (u64, u64),
    /// Why it must not be saved over, if it must not.
    keep: Option<String>,
}

impl Open {
    fn revision(&self) -> (u64, u64) {
        (self.editor.revision(), self.editor.revision_rewinds())
    }
}

pub struct Session {
    sandbox: Sandbox,
    greeted: bool,
    open: Option<Open>,
}

fn int(n: impl TryInto<i64>) -> Value {
    Value::Int(n.try_into().unwrap_or(i64::MAX))
}

fn object(members: Vec<(&str, Value)>) -> Value {
    Value::Object(members.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

/// The reply line for a request with id `id`.
pub fn reply(id: Value, result: Result<Value, Refusal>) -> Value {
    match result {
        Ok(result) => object(vec![
            ("id", id),
            ("ok", Value::Bool(true)),
            ("result", result),
        ]),
        Err(refusal) => {
            let mut error = vec![
                ("code", Value::Str(refusal.code.into())),
                ("message", Value::Str(refusal.message)),
            ];
            if let Some(at) = refusal.at {
                error.push(("at", int(at)));
            }
            let error = object(error);
            object(vec![
                ("id", id),
                ("ok", Value::Bool(false)),
                ("error", error),
            ])
        }
    }
}

/// A request id: a whole number or a short string, given back in the reply.
fn id(fields: &mut Fields) -> Result<Value, Refusal> {
    match fields.raw("id") {
        Some(id @ Value::Int(_)) => Ok(id.clone()),
        Some(Value::Str(s)) if s.chars().count() <= MAX_ID_CHARS => Ok(Value::Str(s.clone())),
        Some(Value::Str(_)) => Err(Refusal::bad(format!(
            "\"id\" is longer than {MAX_ID_CHARS} characters"
        ))),
        Some(other) => Err(Refusal::bad(format!(
            "\"id\" must be a whole number or a string, not {}",
            other.kind()
        ))),
        None => Err(Refusal::bad("a request needs an \"id\"")),
    }
}

impl Session {
    pub fn new(sandbox: Sandbox) -> Self {
        Session {
            sandbox,
            greeted: false,
            open: None,
        }
    }

    /// The reply to one request line.
    pub fn handle(&mut self, line: &[u8]) -> Value {
        let value = match json::parse(line, &LIMITS) {
            Ok(value) => value,
            Err(e) => return reply(Value::Null, Err(Refusal::new("bad_json", e.to_string()))),
        };
        let mut fields = match Fields::of(&value, "a request") {
            Ok(fields) => fields,
            Err(refusal) => return reply(Value::Null, Err(refusal)),
        };
        let id = match id(&mut fields) {
            Ok(id) => id,
            Err(refusal) => return reply(Value::Null, Err(refusal)),
        };
        let result = self.request(fields);
        reply(id, result)
    }

    fn request(&mut self, mut f: Fields) -> Result<Value, Refusal> {
        let kind = f.text("type")?;
        let f = f.named(&format!("a {kind} request"));
        if kind != "hello" && !self.greeted {
            return Err(Refusal::new(
                "hello_first",
                "the first request must be hello, naming the API version",
            ));
        }
        match kind {
            "hello" => self.hello(f),
            "open" => self.open(f),
            "run" => self.run(f),
            "save" => self.save(f),
            "close" => self.close(f),
            other => Err(Refusal::new(
                "unknown_type",
                format!(
                    "there is no request {other:?}; requests are hello, open, run, save and close"
                ),
            )),
        }
    }

    fn hello(&mut self, mut f: Fields) -> Result<Value, Refusal> {
        let api = f.int("api")?;
        f.finish()?;
        if api != i64::from(API_VERSION) {
            return Err(Refusal::new(
                "version",
                format!("API version {api} is not available; this Pikslar provides version {API_VERSION}"),
            ));
        }
        self.greeted = true;
        let limits = object(vec![
            ("line_bytes", int(MAX_LINE_BYTES)),
            ("ops", int(MAX_OPS)),
            ("reply_cells", int(BUDGET.cells)),
            ("pixel_work", int(BUDGET.work)),
            ("depth", int(LIMITS.depth)),
            ("string_chars", int(LIMITS.string_chars)),
        ]);
        Ok(object(vec![
            ("api", int(API_VERSION)),
            ("protocol", int(PROTOCOL)),
            (
                "program",
                Value::Str(concat!("pikslar-cli ", env!("CARGO_PKG_VERSION")).into()),
            ),
            ("limits", limits),
        ]))
    }

    fn open(&mut self, mut f: Fields) -> Result<Value, Refusal> {
        let given = f.text("path")?;
        f.finish()?;
        if self.open.is_some() {
            return Err(Refusal::new(
                "already_open",
                "a project is open; close it first",
            ));
        }
        let path = self.sandbox.existing(given)?;
        let (doc, lossless) = project::read_noted(&path)
            .map_err(|e| Refusal::new("unreadable", format!("{given:?}: {e}")))?;
        let keep = project::write_over_refusal(&path, lossless);
        let editor = Editor::new(doc);
        let saved = (editor.revision(), editor.revision_rewinds());
        let mut open = Open {
            editor,
            path,
            saved,
            keep,
        };
        let outline = document(&mut open.editor);
        let can_save_over = Value::Bool(open.keep.is_none());
        self.open = Some(open);
        Ok(object(vec![
            ("document", outline),
            ("can_save_over", can_save_over),
        ]))
    }

    fn opened(&mut self) -> Result<&mut Open, Refusal> {
        self.open
            .as_mut()
            .ok_or_else(|| Refusal::new("not_open", "no project is open; open one first"))
    }

    fn run(&mut self, mut f: Fields) -> Result<Value, Refusal> {
        let list = f.list("ops")?;
        f.finish()?;
        let ops = apply::read_ops(list)?;
        let open = self.opened()?;
        let ran = apply::run(&mut open.editor, i64::from(API_VERSION), &ops)?;
        Ok(object(vec![
            ("results", Value::Array(ran.results)),
            ("changed", Value::Bool(ran.changed)),
        ]))
    }

    fn save(&mut self, mut f: Fields) -> Result<Value, Refusal> {
        let given = f.opt_text("path")?;
        let overwrite = f.opt_flag("overwrite")?.unwrap_or(false);
        f.finish()?;
        let target = match given {
            Some(given) => Some(self.sandbox.target(given)?),
            None => None,
        };
        let open = self.opened()?;
        let target = target.unwrap_or_else(|| open.path.clone());
        let over = same_file(&target, &open.path);
        if over {
            if let Some(why) = &open.keep {
                return Err(Refusal::new(
                    "cannot_save_over",
                    format!("{why}. Save to a new path to keep this one"),
                ));
            }
        } else {
            require_extension(&target, "piks")
                .map_err(|e| Refusal::new("bad_path", e.to_string()))?;
            check_target(&target, &open.path, overwrite)
                .map_err(|e| target_refusal(e.code(), e.to_string()))?;
        }
        project::save(&target, open.editor.doc())
            .map_err(|e| Refusal::new("write_failed", e.to_string()))?;
        (open.path, open.saved, open.keep) = (target, open.revision(), None);
        Ok(object(vec![("saved", Value::Bool(true))]))
    }

    fn close(&mut self, mut f: Fields) -> Result<Value, Refusal> {
        let discard = f.opt_flag("discard")?.unwrap_or(false);
        f.finish()?;
        let open = self.opened()?;
        if open.revision() != open.saved && !discard {
            return Err(Refusal::new(
                "unsaved",
                "the project has changes that are not saved; save it, or close with \"discard\": true",
            ));
        }
        self.open = None;
        Ok(Value::Null)
    }
}

fn target_refusal(code: u8, message: String) -> Refusal {
    match message.contains("--overwrite") {
        true => Refusal::new(
            "exists",
            message.replace("Add --overwrite", "Give \"overwrite\": true"),
        ),
        false if code == 1 => Refusal::new("bad_path", message),
        false => Refusal::new("bad_target", message),
    }
}

fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    a == b || std::fs::canonicalize(a).is_ok_and(|a| std::fs::canonicalize(b).is_ok_and(|b| a == b))
}

/// The document's outline, as the `document` operation gives it. Reading records no step.
fn document(editor: &mut Editor) -> Value {
    match pikslar_api::run_ops(editor, API_VERSION, &[pikslar_api::Op::Document]) {
        Ok(replies) => replies.first().map_or(Value::Null, api_json::reply),
        Err(_) => Value::Null,
    }
}
