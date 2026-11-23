//! Writing a file the user chose to save as, and what no plugin can reach beyond what it was
//! granted: the write happens only when the run succeeds, together with its undo step.

mod common;

use common::{but_ids, layer_ids, project, state};
use pikslar_bind::{run_script_with, Capabilities, FileAccess, Outcome, ScriptLimits};
use pikslar_core::Editor;
use pikslar_plugin::files::{Disk, FilePicker, FileRequest, PlainDisk};
use pikslar_plugin::{Capability as Word, CapabilitySet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

type Asked = Arc<Mutex<Vec<FileRequest>>>;

/// Answers each dialog with the next path, keeps what was asked, and may set the run's
/// cancel flag as it answers.
struct StandIn {
    choices: Vec<PathBuf>,
    asked: Asked,
    cancel: Option<Arc<AtomicBool>>,
}

impl FilePicker for StandIn {
    fn pick(&mut self, _: &str, request: &FileRequest) -> Option<PathBuf> {
        if let Ok(mut asked) = self.asked.lock() {
            asked.push(request.clone());
        }
        if let Some(c) = &self.cancel {
            c.store(true, Ordering::Relaxed);
        }
        match self.choices.is_empty() {
            true => None,
            false => Some(self.choices.remove(0)),
        }
    }
}

fn words(list: &[Word]) -> CapabilitySet {
    list.iter().fold(CapabilitySet::NONE, |s, &w| s.with(w))
}

struct Run {
    choices: Vec<PathBuf>,
    granted: CapabilitySet,
    cancel: Option<Arc<AtomicBool>>,
    disk: Option<Box<dyn Disk>>,
}

impl Run {
    fn new(choices: &[PathBuf], granted: &[Word]) -> Run {
        Run {
            choices: choices.to_vec(),
            granted: words(granted),
            cancel: None,
            disk: None,
        }
    }

    /// Runs `src` with exactly the capabilities the plugin was granted.
    fn go(self, e: &mut Editor, src: &str) -> (Outcome, Asked) {
        let asked = Asked::default();
        let cancel = self.cancel.clone().unwrap_or_default();
        let picker = StandIn {
            choices: self.choices,
            asked: asked.clone(),
            cancel: self.cancel,
        };
        let mut files = FileAccess::new("Exporter", self.granted, Box::new(picker));
        if let Some(disk) = self.disk {
            files = files.with_disk(disk);
        }
        let caps = Capabilities::from(self.granted);
        let limits = ScriptLimits::default();
        let out = run_script_with(e, src, caps, limits, &cancel, Some(files));
        (out, asked)
    }
}

fn folder(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-write-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a folder");
    dir
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("listed")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn said(out: &Outcome) -> String {
    out.result
        .as_ref()
        .map_or_else(|f| f.to_string(), |s| s.clone())
}

const WRITES: &str = "let h = pick_file_to_write(\"Save the report\", [\"txt\"]);\n\
                      write_file(h, \"report\");\nreturn 1;";

const WRITE: &[Word] = &[Word::PickFileToWrite];

#[test]
fn a_file_is_written_through_the_save_dialog_when_the_run_succeeds() {
    let mut e = project();
    let dir = folder("ok");
    std::fs::write(dir.join("out.txt"), "old").expect("written");
    let (out, asked) = Run::new(&[dir.join("out.txt")], WRITE).go(&mut e, WRITES);
    assert_eq!(out.result, Ok("1".into()), "{}", said(&out));
    assert_eq!(std::fs::read(dir.join("out.txt")).expect("read"), b"report");
    assert_eq!(names(&dir), ["out.txt"], "no temporary file is left");
    let asked = asked.lock().expect("kept");
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].title, "Save the report");
    assert_eq!(asked[0].extensions, ["txt"]);
    assert_eq!(asked[0].access, pikslar_plugin::files::Access::Write);
}

#[test]
fn a_run_that_fails_after_asking_writes_nothing_and_changes_nothing() {
    let mut e = project();
    let before = but_ids(&e);
    let dir = folder("fails");
    std::fs::write(dir.join("out.txt"), "old").expect("written");
    let src = "add_layer(\"Report\");\n\
               let h = pick_file_to_write(\"Save\", []);\n\
               write_file(h, \"new\");\nreturn 1 + nil;";
    let all = &[
        Word::ReadDocument,
        Word::EditDocument,
        Word::PickFileToWrite,
    ];
    let (out, _) = Run::new(&[dir.join("out.txt")], all).go(&mut e, src);
    assert!(out.result.is_err());
    assert_eq!(std::fs::read(dir.join("out.txt")).expect("read"), b"old");
    assert_eq!(names(&dir), ["out.txt"]);
    assert_eq!(but_ids(&e), before);
}

#[test]
fn a_document_edit_and_a_file_are_kept_together() {
    let mut e = project();
    let dir = folder("together");
    let src = "add_layer(\"Report\");\n\
               let h = pick_file_to_write(\"Save\", []);\n\
               write_file(h, \"new\");\nreturn layers()[len(layers()) - 1];";
    let all = &[
        Word::ReadDocument,
        Word::EditDocument,
        Word::PickFileToWrite,
    ];
    let (out, _) = Run::new(&[dir.join("new.txt")], all).go(&mut e, src);
    assert!(out.result.is_ok(), "{}", said(&out));
    assert_eq!(std::fs::read(dir.join("new.txt")).expect("read"), b"new");
    assert_eq!(layer_ids(&e).len(), 3);
}

#[test]
fn a_cancelled_run_leaves_the_file_untouched() {
    let mut e = project();
    let before = state(&e);
    let dir = folder("cancel");
    std::fs::write(dir.join("out.txt"), "old").expect("written");
    let mut run = Run::new(&[dir.join("out.txt")], WRITE);
    // The flag is set as the dialog is answered: the run stops before it ends.
    run.cancel = Some(Arc::new(AtomicBool::new(false)));
    let src = "let h = pick_file_to_write(\"Save\", []);\nwrite_file(h, \"new\");\n\
               let n = 0;\nwhile n < 100000 { n = n + 1; }\nreturn n;";
    let (out, _) = run.go(&mut e, src);
    assert!(
        out.result.as_ref().is_err_and(|f| f.is_cancel()),
        "{}",
        said(&out)
    );
    assert_eq!(std::fs::read(dir.join("out.txt")).expect("read"), b"old");
    assert_eq!(names(&dir), ["out.txt"]);
    assert_eq!(state(&e), before);
}

/// A disk that runs out of space while it writes.
struct Full;

impl Disk for Full {
    fn fill(&mut self, _: &mut std::fs::File, _: &[u8]) -> std::io::Result<()> {
        Err(std::io::Error::new(std::io::ErrorKind::StorageFull, "full"))
    }

    fn replace(&mut self, t: &Path, f: &Path, b: &Path) -> std::io::Result<()> {
        PlainDisk.replace(t, f, b)
    }

    fn place_new(&mut self, f: &Path, t: &Path) -> std::io::Result<()> {
        PlainDisk.place_new(f, t)
    }

    fn put_back(&mut self, b: &Path, t: &Path) -> std::io::Result<()> {
        PlainDisk.put_back(b, t)
    }
}

#[test]
fn a_write_that_fails_at_the_end_takes_back_the_whole_run() {
    let mut e = project();
    let before = but_ids(&e);
    let dir = folder("full");
    std::fs::write(dir.join("out.txt"), "old").expect("written");
    let src = "add_layer(\"Report\");\n\
               let h = pick_file_to_write(\"Save\", []);\nwrite_file(h, \"new\");";
    let all = &[Word::EditDocument, Word::PickFileToWrite];
    let mut run = Run::new(&[dir.join("out.txt")], all);
    run.disk = Some(Box::new(Full));
    let (out, _) = run.go(&mut e, src);
    let why = said(&out);
    assert!(why.contains("no file was written"), "{why}");
    assert!(!why.contains("pikslar-write-full"), "{why}");
    assert_eq!(std::fs::read(dir.join("out.txt")).expect("read"), b"old");
    assert_eq!(names(&dir), ["out.txt"]);
    assert_eq!(but_ids(&e), before);
}

#[test]
fn a_handle_is_written_once() {
    let mut e = project();
    let dir = folder("once");
    let src = "let h = pick_file_to_write(\"Save\", []);\n\
               write_file(h, \"a\");\nwrite_file(h, \"b\");";
    let (out, _) = Run::new(&[dir.join("out.txt")], WRITE).go(&mut e, src);
    assert!(said(&out).contains("written once"), "{}", said(&out));
    assert!(names(&dir).is_empty(), "the failed run wrote nothing");
}

#[test]
fn a_cancelled_save_dialog_gives_nil_and_nil_is_no_handle() {
    let mut e = project();
    let src = "let h = pick_file_to_write(\"Save\", []);\nreturn h == nil;";
    let (out, _) = Run::new(&[], WRITE).go(&mut e, src);
    assert_eq!(out.result, Ok("true".into()));
    let src = "write_file(pick_file_to_write(\"Save\", []), \"x\");";
    let (out, _) = Run::new(&[], WRITE).go(&mut e, src);
    assert!(out.result.is_err());
}

#[test]
fn read_and_write_handles_are_never_swapped() {
    let mut e = project();
    let dir = folder("swap");
    std::fs::write(dir.join("in.txt"), "in").expect("written");
    let both = &[Word::PickFileToRead, Word::PickFileToWrite];
    let src = "let r = pick_file_to_read(\"Open\", []);\nwrite_file(r, \"x\");";
    let (out, _) = Run::new(&[dir.join("in.txt")], both).go(&mut e, src);
    assert!(
        said(&out).contains("other kind of access"),
        "{}",
        said(&out)
    );
    let src = "let w = pick_file_to_write(\"Save\", []);\nreturn read_file(w);";
    let (out, _) = Run::new(&[dir.join("in.txt")], both).go(&mut e, src);
    assert!(
        said(&out).contains("other kind of access"),
        "{}",
        said(&out)
    );
    assert_eq!(std::fs::read(dir.join("in.txt")).expect("read"), b"in");
    assert_eq!(names(&dir), ["in.txt"]);
}

/// Each source names one function the grant does not hold: it must not compile, and no
/// dialog may be shown.
fn absent(granted: &[Word], src: &str, function: &str) {
    let mut e = project();
    let before = state(&e);
    let dir = folder("absent");
    let (out, asked) = Run::new(&[dir.join("x.txt")], granted).go(&mut e, src);
    let why = said(&out);
    assert!(
        out.result.is_err() && why.contains(function),
        "{granted:?} {src}: {why}"
    );
    assert!(asked.lock().expect("kept").is_empty(), "a dialog was shown");
    assert_eq!(state(&e), before);
    assert!(names(&dir).is_empty());
}

#[test]
fn no_plugin_reaches_beyond_its_grant() {
    let read = &[Word::ReadDocument];
    let edit = &[Word::EditDocument];
    let pick_read = &[Word::PickFileToRead];
    // Without pick-file-to-write, the write functions do not exist.
    for granted in [&[][..], read, edit, pick_read] {
        absent(granted, WRITES, "pick_file_to_write");
        absent(granted, "write_file(0, \"x\");", "write_file");
    }
    // With it alone: no reading files, no document.
    absent(WRITE, "return read_file(0);", "read_file");
    absent(
        WRITE,
        "return pick_file_to_read(\"Open\", []);",
        "pick_file_to_read",
    );
    absent(WRITE, "return width();", "width");
    absent(WRITE, "add_layer(\"x\");", "add_layer");
    // Reading the document only: none of the edits exist.
    for edit_fn in [
        "add_layer(\"x\")",
        "remove_layer(0)",
        "set_pixel(0, 0, 0, 0, 0)",
        "fill_rect(0, 0, 0, 0, 1, 1, 0)",
        "write_rect(0, 0, 0, 0, 1, [0])",
        "add_frame(0)",
        "add_swatch(0)",
        "recolor(0, 0)",
    ] {
        let name = edit_fn.split('(').next().unwrap_or_default();
        absent(read, &format!("{edit_fn};"), name);
    }
    // Editing it only: no pixels, swatches or layers are read.
    for read_fn in [
        "get_pixel(0, 0, 0, 0)",
        "read_rect(0, 0, 0, 0, 1, 1)",
        "swatches()",
        "layers()",
        "width()",
    ] {
        let name = read_fn.split('(').next().unwrap_or_default();
        absent(edit, &format!("return {read_fn};"), name);
    }
}
