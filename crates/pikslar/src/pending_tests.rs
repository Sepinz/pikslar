//! With a writer stuck for up to 10 s, every handler on the window's thread returns at once,
//! the status line says what is still being written, and the frame loop finishes the rest.

use super::*;
use crate::loading::{Then, NO_THREAD};
use crate::recovery::{Origin, Recovery};
use crate::test_app::*;
use pikslar_platform::Window;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Far less than the 10 s the writer is stuck for, and the 250 ms the window must answer in.
const PROMPT: Duration = Duration::from_millis(250);

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-pending-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

fn gate() -> (Sender<()>, Receiver<()>) {
    channel()
}

fn timed(f: impl FnOnce()) -> Duration {
    let start = Instant::now();
    f();
    start.elapsed()
}

fn said_now(a: &App) -> String {
    said(a)
}

/// Frames until `done`, as the window would draw them, for at most 5 s.
fn frames_until(a: &mut App, done: impl Fn(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done(a) {
        assert!(Instant::now() < end, "the frame loop finished the wait");
        a.tick_recovery();
        a.settle_rewind(false);
        a.collect_opened(Window::detached());
        a.tick_pending();
        std::thread::sleep(Duration::from_millis(5));
    }
    a.tick_pending();
}

fn copies(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "piks"))
                .count()
        })
        .unwrap_or(0)
}

/// An app with a backup copy on disk and its writer stuck.
fn stuck_backup(name: &str) -> (App, PathBuf, Sender<()>) {
    let dir = temp_dir(name);
    let mut a = app(1);
    a.use_recovery(Recovery::try_in_dir(&dir).expect("usable"), false);
    dot(&mut a, 1, 1);
    a.recovery
        .write_now(a.editor.doc(), None, Origin::NONE)
        .expect("a copy");
    assert_eq!(copies(&dir), 1);
    let (open, shut) = gate();
    a.recovery.hold_writer(shut);
    (a, dir, open)
}

#[test]
fn saving_while_the_backup_writer_is_stuck_returns_at_once_and_the_copy_goes_later() {
    let (mut a, dir, open) = stuck_backup("save");
    let art = dir.join("art.piks");
    let took = timed(|| assert!(a.save_to(Window::detached(), art.clone())));
    assert!(took < Duration::from_secs(1), "the save waited {took:?}");
    assert!(a.recovery.clear_waiting());
    a.clear_message();
    a.tick_pending();
    assert_eq!(said_now(&a), STILL_BACKUP);
    assert!(a.pending_wait().is_some(), "the frame comes back to look");
    open.send(()).expect("let go");
    frames_until(&mut a, |a| !a.recovery.clear_waiting());
    assert_eq!(copies(&dir), 1, "only the saved project is left");
    assert_eq!(said_now(&a), "", "the words go once it is done");
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_document_while_the_backup_writer_is_stuck_returns_at_once() {
    let (mut a, dir, open) = stuck_backup("new");
    let doc = pikslar_core::Document::new(4, 4, pikslar_core::ColorMode::Rgba).expect("doc");
    let took = timed(|| a.replace_document(doc, None));
    assert!(took < PROMPT, "{took:?}");
    open.send(()).expect("let go");
    frames_until(&mut a, |a| !a.recovery.clear_waiting());
    assert_eq!(copies(&dir), 0);
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn writing_now_waits_only_as_long_as_it_is_given_and_says_why() {
    let (mut a, dir, open) = stuck_backup("now");
    let mut result = Ok(());
    let took = timed(|| {
        result = a.recovery.write_now_within(
            a.editor.doc(),
            None,
            Origin::NONE,
            Duration::from_millis(150),
        )
    });
    assert!(took < Duration::from_millis(600), "{took:?}");
    let why = result.expect_err("not written").to_string();
    assert!(why.contains("still being written"), "{why}");
    open.send(()).expect("let go");
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_session_ending_with_the_writer_stuck_waits_a_bounded_time_and_reports_it() {
    let (mut a, dir, open) = stuck_backup("ending");
    dot(&mut a, 2, 2);
    let mut kept = true;
    let took = timed(|| kept = a.end_session());
    assert!(!kept, "the copy could not be written");
    assert!(
        took >= QUIT_WAIT && took < QUIT_WAIT + Duration::from_secs(2),
        "{took:?}"
    );
    assert!(
        said_now(&a).contains("still being written"),
        "{}",
        said_now(&a)
    );
    open.send(()).expect("let go");
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_rewind_request_while_history_is_being_written_waits_its_turn() {
    let mut a = app(1);
    let (open, shut) = gate();
    a.hold_rewind(shut);
    let took = timed(|| a.forget_all_rewind());
    assert!(took < PROMPT, "{took:?}");
    assert!(a.rewind.waiting());
    assert_eq!(said_now(&a), STILL_HISTORY);
    let took = timed(|| a.toggle_rewind());
    assert!(
        took < PROMPT,
        "a second request does not wait either: {took:?}"
    );
    open.send(()).expect("let go");
    frames_until(&mut a, |a| !a.rewind.waiting());
    assert_ne!(said_now(&a), STILL_HISTORY);
}

#[test]
fn a_new_document_while_history_is_being_written_returns_at_once() {
    let mut a = app(1);
    let (open, shut) = gate();
    a.hold_rewind(shut);
    let doc = pikslar_core::Document::new(4, 4, pikslar_core::ColorMode::Rgba).expect("doc");
    let took = timed(|| a.replace_document(doc, None));
    assert!(took < PROMPT, "{took:?}");
    assert!(a.rewind.waiting());
    open.send(()).expect("let go");
    frames_until(&mut a, |a| !a.rewind.waiting());
}

#[test]
fn an_open_made_here_after_a_stuck_read_waits_for_the_frame_loop() {
    let dir = temp_dir("open");
    let art = dir.join("art.piks");
    let doc = pikslar_core::Document::new(4, 4, pikslar_core::ColorMode::Rgba).expect("doc");
    crate::files::save(&art, &doc).expect("written");
    let mut a = app(1);
    let (open, shut) = gate();
    a.stopping = Some(std::thread::spawn(move || {
        let _ = shut.recv_timeout(Duration::from_secs(10));
    }));
    NO_THREAD.with(|n| n.set(true));
    let took = timed(|| a.begin_open(Window::detached(), art.clone(), Then::Nothing));
    NO_THREAD.with(|n| n.set(false));
    assert!(took < PROMPT, "{took:?}");
    assert!(
        said_now(&a).contains("once the last read stops"),
        "{}",
        said_now(&a)
    );
    assert!(a.path.is_none(), "not opened yet");
    open.send(()).expect("let go");
    frames_until(&mut a, |a| a.open_after_stop.is_none());
    assert_eq!(a.path.as_deref(), Some(art.as_path()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A picker that is stuck until let go: a dialog nobody answers, or a slow disk.
struct Stuck {
    entered: Arc<AtomicBool>,
    shut: Receiver<()>,
}

impl pikslar_plugin::files::FilePicker for Stuck {
    fn pick(&mut self, _: &str, _: &pikslar_plugin::files::FileRequest) -> Option<PathBuf> {
        self.entered.store(true, Ordering::Relaxed);
        let _ = self.shut.recv_timeout(Duration::from_secs(10));
        None
    }
}

#[test]
fn a_stopped_script_still_in_a_slow_call_is_waited_for_with_the_window_answering() {
    let mut a = app(1);
    let (open, shut) = gate();
    let entered = Arc::new(AtomicBool::new(false));
    let picker = Stuck {
        entered: Arc::clone(&entered),
        shut,
    };
    let granted =
        pikslar_plugin::CapabilitySet::NONE.with(pikslar_plugin::Capability::PickFileToRead);
    let files = pikslar_bind::FileAccess::new("Stuck", granted, Box::new(picker));
    let caps = pikslar_bind::Capabilities::ALL.with(pikslar_bind::Capability::PickFileToRead);
    let source = "let h = pick_file_to_read(\"Choose\", [\"txt\"]);\nprint(file_name(h));";
    let (mut calls, mut open) = (0, Some(open));
    let mut longest = Duration::ZERO;
    let limits = a.script_limits;
    let out = within(20, "a stopped script", || {
        crate::script_panel::run_beside(
            &mut a.editor,
            (source, caps),
            limits,
            Some(files),
            |finished, _| {
                calls += 1;
                let start = Instant::now();
                if calls == 1 {
                    // Stop is pressed once the script is in the slow call.
                    while !entered.load(Ordering::Relaxed) && !finished() {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    return;
                }
                if calls == 4 {
                    open.take().map(|o| o.send(()));
                }
                longest = longest.max(start.elapsed());
            },
        )
    });
    assert!(calls >= 4, "the wait went on, not a bare join: {calls}");
    assert!(longest < PROMPT, "{longest:?}");
    assert!(out.result.is_err(), "the run was stopped");
}
