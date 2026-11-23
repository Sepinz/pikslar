//! Real process deaths: a child is terminated at random moments while it writes.
//!
//! Termination is what Task Manager does and what a crash amounts to: nothing gets to
//! clean up. Files must afterwards hold either the old contents or the new, never a mix.

use crate::app::App;
use crate::recovery::{Origin, Recovery};
use pikslar_core::{ColorMode, Document, FloatingSnapshot, Grid, Mask, SelectOp};
use pikslar_core::{FloatingCmd, SelectionCmd};
use pikslar_format::piks;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DIR_VAR: &str = "PIKSLAR_CRASH_TEST_DIR";
const KILLS: u32 = 30;
const SIDE: usize = 320;

/// Colors that do not compress away, so each write takes real time.
fn busy_document(seed: u32) -> Document {
    let mut cells = vec![0u32; SIDE * SIDE];
    let mut x = seed.wrapping_mul(2_654_435_761) | 1;
    for cell in &mut cells {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *cell = 0xff00_0000 | (x >> 8);
    }
    let mut doc = Document::new(SIDE, SIDE, ColorMode::Rgba).expect("valid");
    doc.set_cel(0, 0, Grid::from_cells(SIDE, SIDE, &cells))
        .expect("canvas size");
    doc
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-kill-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Starts a copy of this test program running one ignored test.
fn spawn_child(test: &str, dir: &Path) -> std::process::Child {
    Command::new(std::env::current_exe().expect("test program"))
        .args(["--exact", test, "--ignored", "--nocapture"])
        .env(DIR_VAR, dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("child starts")
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.exists() {
        assert!(Instant::now() < deadline, "the child never got going");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A delay that lands at a different point in the write each time.
fn kill_after(i: u32) -> Duration {
    Duration::from_micros(u64::from(i.wrapping_mul(2_654_435_761) % 24_000))
}

#[test]
#[ignore = "run by interrupted_saves_leave_a_whole_file"]
fn child_saves_forever() {
    let Some(dir) = std::env::var_os(DIR_VAR).map(PathBuf::from) else {
        return;
    };
    let target = dir.join("art.piks");
    let (a, b) = (busy_document(1), busy_document(2));
    let ready = dir.join("ready");
    loop {
        crate::files::save(&target, &a).expect("save");
        if !ready.exists() {
            std::fs::write(&ready, b"1").expect("marker");
        }
        crate::files::save(&target, &b).expect("save");
    }
}

#[test]
fn interrupted_saves_leave_a_whole_file() {
    let dir = temp_dir("save");
    let target = dir.join("art.piks");
    let first = busy_document(1);
    let second = busy_document(2);
    crate::files::save(&target, &first).expect("first save");
    let mut interrupted = 0;
    let mut seen_left = 0;
    // Whether a kill lands mid-write depends on load, so an unlucky run keeps trying.
    let mut i = 0;
    while i < KILLS || (interrupted == 0 && i < KILLS * 8) {
        i += 1;
        let _ = std::fs::remove_file(dir.join("ready"));
        let mut child = spawn_child("crash_tests::child_saves_forever", &dir);
        wait_for(&dir.join("ready"));
        std::thread::sleep(kill_after(i));
        child.kill().expect("terminated");
        child.wait().expect("gone");
        // Each save has a temporary name of its own: one left is a kill mid-write.
        let left = temps_left(&dir);
        interrupted += u32::from(left > seen_left);
        seen_left = left;
        let bytes = std::fs::read(&target).expect("still there");
        let doc = piks::load(&bytes).expect("the file is whole");
        assert!(
            doc == first || doc == second,
            "kill {i}: some other contents"
        );
    }
    assert!(interrupted > 0, "no kill landed during a write");
    std::fs::remove_dir_all(&dir).expect("clean up");
}

fn floating_app(dir: &Path) -> App {
    let mut app = App::launch(None, Some(8), None);
    app.resized(800, 600, 1);
    app.replace_document(busy_document(3), None);
    app.use_recovery(Recovery::in_dir(dir), false);
    app.editor
        .apply(SelectionCmd::Select {
            shape: Box::new(Mask::rect(10, 10, 200, 150)),
            op: SelectOp::Replace,
        })
        .expect("select");
    app.nudge(1, 0, false);
    app
}

fn temps_left(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .expect("dir")
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "pikslar-tmp"))
        .count()
}

fn has_unfinished_write(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .expect("dir")
        .flatten()
        .any(|e| e.path().extension().is_some_and(|x| x == "pikslar-tmp"))
}

fn snapshot(app: &App) -> FloatingSnapshot {
    app.editor.floating_snapshot().expect("floating")
}

#[test]
#[ignore = "run by a_killed_session_can_always_be_restored"]
fn child_moves_pixels_forever() {
    let Some(dir) = std::env::var_os(DIR_VAR).map(PathBuf::from) else {
        return;
    };
    let mut app = floating_app(&dir);
    let doc = app.editor.doc().clone();
    let ready = dir.join("ready");
    let mut step = 0;
    loop {
        app.nudge(if step % 2 == 0 { 1 } else { -1 }, 0, false);
        step += 1;
        let state = snapshot(&app);
        app.recovery
            .write_now(&doc, Some(&state), Origin::NONE)
            .expect("write");
        if !ready.exists() {
            std::fs::write(&ready, b"1").expect("marker");
        }
    }
}

#[test]
fn a_killed_session_can_always_be_restored() {
    let art = busy_document(3);
    let mut interrupted = 0;
    for i in 0..KILLS {
        let dir = temp_dir(&format!("float{i}"));
        let mut child = spawn_child("crash_tests::child_moves_pixels_forever", &dir);
        wait_for(&dir.join("ready"));
        std::thread::sleep(kill_after(i));
        child.kill().expect("terminated");
        child.wait().expect("gone");
        interrupted += u32::from(has_unfinished_write(&dir));

        let mut next = App::launch(None, Some(8), None);
        next.resized(800, 600, 1);
        next.use_recovery(Recovery::in_dir(&dir), false);
        let orphan = next.recovery.find_orphan().expect("work was left");
        next.restore_orphan(&orphan).expect("the copy is whole");
        assert!(next.editor.is_floating(), "kill {i}");
        let position = next.editor.selection().map(|m| m.bounds().0);
        assert!(matches!(position, Some(11 | 12)), "kill {i}: {position:?}");
        next.editor.apply(FloatingCmd::Revert).expect("revert");
        assert_eq!(
            next.editor.doc().layers()[0]
                .cel(0)
                .to_cells(next.editor.doc().width, next.editor.doc().height),
            art.layers()[0].cel(0).to_cells(art.width, art.height),
            "kill {i}: the art changed"
        );
        drop(next);
        let _ = std::fs::remove_dir_all(&dir);
    }
    assert!(interrupted > 0, "no kill landed during a write");
}
