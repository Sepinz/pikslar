//! Reading a file the user chose, through a handle: the only way a script reaches a file.

mod common;

use common::{project, state};
use pikslar_bind::{run_script_with, Capabilities, Capability, FileAccess, Outcome, ScriptLimits};
use pikslar_core::Editor;
use pikslar_plugin::files::{FilePicker, FileRequest};
use pikslar_plugin::{Capability as Word, CapabilitySet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// Answers every dialog with the same choice, and keeps what each one asked.
struct StandIn {
    choice: Option<PathBuf>,
    asked: Arc<Mutex<Vec<(String, FileRequest)>>>,
}

impl FilePicker for StandIn {
    fn pick(&mut self, plugin_name: &str, request: &FileRequest) -> Option<PathBuf> {
        if let Ok(mut asked) = self.asked.lock() {
            asked.push((plugin_name.to_string(), request.clone()));
        }
        self.choice.clone()
    }
}

type Asked = Arc<Mutex<Vec<(String, FileRequest)>>>;

fn granted_read() -> CapabilitySet {
    CapabilitySet::NONE.with(Word::PickFileToRead)
}

fn access(choice: Option<PathBuf>, granted: CapabilitySet) -> (FileAccess, Asked) {
    let asked = Asked::default();
    let picker = StandIn {
        choice,
        asked: asked.clone(),
    };
    (
        FileAccess::new("Palettes", granted, Box::new(picker)),
        asked,
    )
}

fn run(e: &mut Editor, src: &str, caps: Capabilities, files: Option<FileAccess>) -> Outcome {
    let limits = ScriptLimits::default();
    run_script_with(e, src, caps, limits, &AtomicBool::new(false), files)
}

fn pick_caps() -> Capabilities {
    Capabilities::NONE.with(Capability::PickFileToRead)
}

fn a_file(test: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-bind-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let path = dir.join("colors.hex");
    std::fs::write(&path, text).expect("written");
    path
}

const READS: &str = "let h = pick_file_to_read(\"Choose a palette\", [\"hex\"]);\n\
                     return file_name(h) + \"=\" + read_file(h);";

#[test]
fn a_chosen_file_is_read_through_its_handle_and_its_name_only() {
    let mut e = project();
    let path = a_file("read", "112233");
    let (files, asked) = access(Some(path), granted_read());
    let out = run(&mut e, READS, pick_caps(), Some(files));
    assert_eq!(out.result, Ok("colors.hex=112233".into()));
    let asked = asked.lock().expect("kept");
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].0, "Palettes", "the dialog names the plugin");
    assert_eq!(asked[0].1.title, "Choose a palette");
    assert_eq!(asked[0].1.extensions, ["hex"]);
}

#[test]
fn a_cancelled_dialog_gives_nil() {
    let mut e = project();
    let (files, _) = access(None, granted_read());
    let src = "return pick_file_to_read(\"Choose\", []) == nil;";
    let out = run(&mut e, src, pick_caps(), Some(files));
    assert_eq!(out.result, Ok("true".into()));
}

#[test]
fn without_the_capability_the_functions_do_not_exist() {
    let mut e = project();
    let before = state(&e);
    let (files, asked) = access(Some(a_file("caps", "x")), granted_read());
    let out = run(&mut e, READS, Capabilities::ALL, Some(files));
    assert!(out
        .result
        .is_err_and(|f| f.to_string().contains("pick_file_to_read")));
    assert!(
        asked.lock().expect("kept").is_empty(),
        "no dialog was shown"
    );
    assert_eq!(state(&e), before);
}

#[test]
fn handles_are_only_the_ones_the_user_gave_and_nothing_is_reached_without_a_picker() {
    let mut e = project();
    let (files, _) = access(Some(a_file("handles", "x")), granted_read());
    let made_up = run(&mut e, "return read_file(7);", pick_caps(), Some(files));
    let said = made_up.result.map_err(|f| f.to_string());
    assert!(
        said.as_ref()
            .is_err_and(|s| s.contains("not a file the user chose")),
        "{said:?}"
    );
    let none = run(&mut e, READS, pick_caps(), None);
    let said = none.result.map_err(|f| f.to_string());
    assert!(
        said.as_ref()
            .is_err_and(|s| s.contains("no way to ask for files")),
        "{said:?}"
    );
    // The plugin's own grant is checked again by the grants, before any dialog.
    let (files, asked) = access(Some(a_file("handles", "x")), CapabilitySet::NONE);
    let refused = run(&mut e, READS, pick_caps(), Some(files));
    assert!(refused.result.is_err());
    assert!(asked.lock().expect("kept").is_empty());
}

#[test]
fn a_file_that_is_not_text_is_refused_without_its_path() {
    let mut e = project();
    let dir = std::env::temp_dir().join(format!("pikslar-bind-bin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let path = dir.join("secret-place.bin");
    std::fs::write(&path, [0xff, 0xfe, 0x00]).expect("written");
    let (files, _) = access(Some(path), granted_read());
    let src = "let h = pick_file_to_read(\"Choose\", []);\nreturn read_file(h);";
    let out = run(&mut e, src, pick_caps(), Some(files));
    let said = out.result.map_err(|f| f.to_string()).expect_err("refused");
    assert!(
        said.contains("not UTF-8 text") && !said.contains("secret-place"),
        "{said}"
    );
}

#[test]
fn a_file_larger_than_the_script_may_hold_is_refused_before_it_is_looked_at() {
    let mut e = project();
    // Not text at its very end: the size is what refuses it, before any check of the bytes.
    let path = a_file("large", &"ab".repeat(1 << 20));
    let mut bytes = std::fs::read(&path).expect("written");
    bytes.push(0xff);
    std::fs::write(&path, bytes).expect("written");
    let (files, _) = access(Some(path), granted_read());
    let small = ScriptLimits {
        memory: 1 << 20,
        ..ScriptLimits::default()
    };
    let src = "let h = pick_file_to_read(\"Choose\", []);\nreturn len(read_file(h));";
    let out = run_script_with(
        &mut e,
        src,
        pick_caps(),
        small,
        &AtomicBool::new(false),
        Some(files),
    );
    assert_eq!(
        out.result.map_err(|f| f.kind()),
        Err(Some(pikslar_script::ErrorKind::Memory))
    );
    // Refused from the file's length, before it is read or the read paid for.
    assert!(out.fuel_used < (2 << 20) / 64, "{}", out.fuel_used);
}

#[test]
fn a_long_list_of_file_kinds_is_refused_before_it_is_copied() {
    let mut e = project();
    let (files, asked) = access(Some(a_file("kinds", "x")), granted_read());
    let src = "return pick_file_to_read(\"Choose\", list(100000, \"hex\"));";
    let out = run(&mut e, src, pick_caps(), Some(files));
    let said = out.result.map_err(|f| f.to_string()).expect_err("refused");
    assert!(said.contains("at most 8"), "{said}");
    assert!(asked.lock().expect("kept").is_empty());
}

#[test]
fn a_file_is_read_once_and_reading_it_again_costs_nothing_more() {
    let mut e = project();
    let path = a_file("again", &"ab".repeat(1 << 19));
    let once = "let h = pick_file_to_read(\"Choose\", []);\nreturn len(read_file(h));";
    let twice = "let h = pick_file_to_read(\"Choose\", []);\n\
                 let a = read_file(h);\nreturn len(a) + len(read_file(h));";
    let (files, _) = access(Some(path.clone()), granted_read());
    let one = run(&mut e, once, pick_caps(), Some(files));
    let (files, _) = access(Some(path), granted_read());
    let two = run(&mut e, twice, pick_caps(), Some(files));
    assert_eq!(one.result, Ok((1 << 20).to_string()));
    assert_eq!(two.result, Ok((2 << 20).to_string()));
    assert!(one.fuel_used > (1 << 20) / 64, "the read was paid for");
    assert!(
        two.fuel_used - one.fuel_used < 1000,
        "{} then {}",
        one.fuel_used,
        two.fuel_used
    );
    assert!(
        two.peak_memory < one.peak_memory + (1 << 19),
        "one copy is held"
    );
}

/// Sets the run's cancel flag as the dialog is answered.
struct Cancels(PathBuf, Arc<AtomicBool>);

impl FilePicker for Cancels {
    fn pick(&mut self, _: &str, _: &FileRequest) -> Option<PathBuf> {
        self.1.store(true, std::sync::atomic::Ordering::Relaxed);
        Some(self.0.clone())
    }
}

#[test]
fn a_cancel_stops_a_read_of_a_large_file() {
    let mut e = project();
    let before = state(&e);
    let path = a_file("cancel", &"ab".repeat(4 << 20));
    let cancel = Arc::new(AtomicBool::new(false));
    let files = FileAccess::new(
        "Palettes",
        granted_read(),
        Box::new(Cancels(path, cancel.clone())),
    );
    let src = "return len(read_file(pick_file_to_read(\"Choose\", [])));";
    let limits = ScriptLimits::default();
    let out = run_script_with(&mut e, src, pick_caps(), limits, &cancel, Some(files));
    assert!(
        out.result.as_ref().is_err_and(|f| f.is_cancel()),
        "{:?}",
        out.result
    );
    assert!(out.peak_memory < 1 << 20, "the text was never made");
    assert_eq!(state(&e), before);
}
