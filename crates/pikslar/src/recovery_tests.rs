//! Crashing part way through moving, pasting or transforming, and coming back.

use crate::app::{App, Gesture, ToolChoice};
use crate::recovery::{Origin, Recovery};
use pikslar_core::{rgba, Clip, ColorMode, Document, Editor, Mask, SelectOp, Tool};
use pikslar_core::{DrawCmd, FloatingCmd, SelectionCmd};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

const INK: u32 = rgba(180, 30, 60, 255);
const CELLS: [(i32, i32); 4] = [(2, 2), (3, 2), (2, 3), (5, 4)];

static NEXT: AtomicU32 = AtomicU32::new(0);

fn temp_dir() -> PathBuf {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("pikslar-restore-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn artwork(mode: ColorMode) -> Document {
    let mut e = Editor::new(Document::new(32, 32, mode).expect("valid"));
    e.color = INK;
    for (x, y) in CELLS {
        e.apply(DrawCmd::Begin {
            tool: Tool::Pencil,
            x,
            y,
        })
        .expect("begin");
        e.apply(DrawCmd::Commit).expect("commit");
    }
    e.doc().clone()
}

fn session(dir: &std::path::Path, doc: Document) -> App {
    let mut app = App::launch(None, Some(8), None);
    app.resized(800, 600, 1);
    app.replace_document(doc, None);
    app.use_recovery(Recovery::in_dir(dir), false);
    app
}

fn select(app: &mut App) {
    app.editor
        .apply(SelectionCmd::Select {
            shape: Box::new(Mask::rect(2, 2, 4, 3)),
            op: SelectOp::Replace,
        })
        .expect("select");
}

fn write_copy_now(app: &mut App) {
    app.recovery_due = Some(Instant::now());
    app.tick_recovery();
    app.recovery.wait_for_writer();
}

fn crash_and_return(mut app: App, dir: &std::path::Path) -> App {
    app.recovery.simulate_crash();
    drop(app);
    let mut next = session(dir, artwork(ColorMode::Palette));
    let orphan = next.recovery.find_orphan().expect("work was left behind");
    next.restore_orphan(&orphan).expect("restores");
    next
}

fn picture(app: &mut App) -> Vec<u32> {
    app.editor.settle_floating();
    let mut seen = Vec::with_capacity(36 * 36);
    for y in -2..34 {
        for x in -2..34 {
            seen.push(app.editor.color_at(x, y));
        }
    }
    seen
}

#[test]
fn a_move_is_protected_even_on_a_saved_document() {
    let dir = temp_dir();
    let mut app = session(&dir, artwork(ColorMode::Palette));
    select(&mut app);
    app.nudge(7, 5, false);
    assert!(
        app.is_modified(),
        "what is shown differs from what was saved"
    );
    let mut expected = {
        write_copy_now(&mut app);
        picture(&mut app)
    };
    let mut back = crash_and_return(app, &dir);
    assert!(back.editor.is_floating());
    assert_eq!(back.tool, ToolChoice::Move);
    assert_eq!(picture(&mut back), std::mem::take(&mut expected));
    assert!(back.is_modified(), "restored work reads as unsaved");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_kind_of_floating_work_comes_back_the_same() {
    type Work = fn(&mut App);
    let works: [(&str, Work); 5] = [
        ("move", |a| a.nudge(9, 1, false)),
        ("off canvas", |a| a.nudge(-30, 0, false)),
        ("flip and turn", |a| {
            a.editor
                .apply(FloatingCmd::FlipVertical)
                .expect("flipvertical");
            a.editor
                .apply(FloatingCmd::Rotate { clockwise: false })
                .expect("rotate");
        }),
        ("scale", |a| {
            a.editor
                .apply(FloatingCmd::Lift { copy: false })
                .expect("lift");
            a.editor
                .apply(FloatingCmd::Scale {
                    x: 10,
                    y: 10,
                    w: 9,
                    h: 7,
                })
                .expect("scale");
        }),
        ("copy", |a| {
            a.editor
                .apply(FloatingCmd::Lift { copy: true })
                .expect("lift");
            a.editor
                .apply(FloatingCmd::Move {
                    dx: 11,
                    dy: 11,
                    join: true,
                })
                .expect("move");
        }),
    ];
    for mode in [ColorMode::Palette, ColorMode::Rgba] {
        for (name, work) in works {
            let dir = temp_dir();
            let mut app = session(&dir, artwork(mode));
            select(&mut app);
            work(&mut app);
            write_copy_now(&mut app);
            let expected = picture(&mut app);
            let mut back = crash_and_return(app, &dir);
            assert_eq!(picture(&mut back), expected, "{name} {mode:?}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

#[test]
fn a_paste_comes_back_and_can_be_cancelled() {
    let dir = temp_dir();
    let mut app = session(&dir, artwork(ColorMode::Palette));
    let table = app.editor.doc().colors.len();
    let clip = Clip::from_image(3, 1, vec![rgba(1, 2, 3, 255); 3]);
    app.editor
        .apply(FloatingCmd::Paste {
            clip: std::sync::Arc::new(clip.clone()),
            x: 20,
            y: 20,
        })
        .expect("fits");
    write_copy_now(&mut app);
    let expected = picture(&mut app);
    let mut back = crash_and_return(app, &dir);
    assert!(back.editor.is_pasting());
    assert_eq!(picture(&mut back), expected);
    back.editor.apply(FloatingCmd::Revert).expect("revert");
    assert_eq!(back.editor.doc().colors.len(), table);
    assert_eq!(back.editor.color_at(20, 20), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_drag_in_progress_is_protected_but_a_stroke_is_left_alone() {
    let dir = temp_dir();
    let mut app = session(&dir, artwork(ColorMode::Palette));
    select(&mut app);
    app.nudge(1, 0, false);
    app.gesture = Gesture::Move {
        x: 0,
        y: 0,
        lifted: false,
    };
    write_copy_now(&mut app);
    assert_eq!(app.recovered, app.recovery_stamp());
    app.editor
        .apply(FloatingCmd::Move {
            dx: 1,
            dy: 0,
            join: true,
        })
        .expect("move");
    app.gesture = Gesture::Stroke;
    write_copy_now(&mut app);
    assert_ne!(app.recovered, app.recovery_stamp());
    app.gesture = Gesture::None;
    drop(app);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancelling_leaves_no_copy_of_a_document_with_no_other_changes() {
    let dir = temp_dir();
    let mut app = session(&dir, artwork(ColorMode::Palette));
    select(&mut app);
    app.nudge(1, 0, false);
    write_copy_now(&mut app);
    app.editor.apply(FloatingCmd::Revert).expect("revert");
    app.tick_recovery();
    app.recovery.simulate_crash();
    drop(app);
    let next = session(&dir, artwork(ColorMode::Palette));
    assert_eq!(next.recovery.find_orphan(), None);
    drop(next);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancelling_after_a_restore_gives_back_the_art_exactly() {
    let dir = temp_dir();
    let mut app = session(&dir, artwork(ColorMode::Rgba));
    let original = app.editor.doc().layers()[0]
        .cel(0)
        .to_cells(app.editor.doc().width, app.editor.doc().height);
    select(&mut app);
    app.nudge(3, 3, false);
    write_copy_now(&mut app);
    let mut back = crash_and_return(app, &dir);
    back.editor.apply(FloatingCmd::Revert).expect("revert");
    assert_eq!(
        back.editor.doc().layers()[0]
            .cel(0)
            .to_cells(back.editor.doc().width, back.editor.doc().height),
        original
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_original_file_is_never_touched_by_recovery() {
    let dir = temp_dir();
    let source = dir.with_extension("piks");
    crate::files::save(&source, &artwork(ColorMode::Palette)).expect("saved");
    let before = std::fs::read(&source).expect("read");
    let mut app = session(&dir, artwork(ColorMode::Palette));
    app.path = Some(source.clone());
    app.recovery.note_origin(Some(&source));
    select(&mut app);
    app.nudge(4, 0, false);
    write_copy_now(&mut app);
    let mut back = crash_and_return(app, &dir);
    assert_eq!(
        back.path.as_ref(),
        Some(&source),
        "still knows where it came from"
    );
    assert_eq!(std::fs::read(&source).expect("read"), before);
    back.editor.apply(FloatingCmd::Anchor).expect("anchor");
    assert_eq!(std::fs::read(&source).expect("read"), before);
    drop(back);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&source);
}

#[test]
fn crashing_again_straight_after_a_restore_loses_nothing() {
    let dir = temp_dir();
    let mut app = session(&dir, artwork(ColorMode::Palette));
    select(&mut app);
    app.nudge(6, 6, false);
    write_copy_now(&mut app);
    let expected = picture(&mut app);
    let back = crash_and_return(app, &dir);
    let mut again = crash_and_return(back, &dir);
    assert!(again.editor.is_floating());
    assert_eq!(picture(&mut again), expected);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_windows_shutdown_keeps_pixels_floating() {
    let dir = temp_dir();
    let mut app = session(&dir, artwork(ColorMode::Palette));
    select(&mut app);
    app.nudge(-30, 0, false);
    let expected = picture(&mut app);
    assert!(app.end_session());
    assert!(
        app.editor.is_floating(),
        "nothing was applied on the artist's behalf"
    );
    let mut back = crash_and_return(app, &dir);
    assert_eq!(picture(&mut back), expected);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn floating_pixels_that_no_longer_fit_still_restore_the_art() {
    let dir = temp_dir();
    let mut app = session(&dir, artwork(ColorMode::Palette));
    select(&mut app);
    app.nudge(1, 1, false);
    let snapshot = app.editor.floating_snapshot().expect("floating");
    let damaged = pikslar_core::FloatingSnapshot {
        layer: 40,
        ..snapshot
    };
    app.recovery
        .write_now(&app.editor.doc().clone(), Some(&damaged), Origin::NONE)
        .expect("write");
    let original = app.editor.doc().layers()[0]
        .cel(0)
        .to_cells(app.editor.doc().width, app.editor.doc().height);
    let back = crash_and_return(app, &dir);
    assert!(!back.editor.is_floating());
    assert_eq!(
        back.editor.doc().layers()[0]
            .cel(0)
            .to_cells(back.editor.doc().width, back.editor.doc().height),
        original
    );
    assert!(back.message.is_some(), "the artist is told");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_copy_cut_short_is_refused_and_the_current_sprite_is_kept() {
    let dir = temp_dir();
    let mut app = session(&dir, artwork(ColorMode::Palette));
    select(&mut app);
    app.nudge(1, 1, false);
    write_copy_now(&mut app);
    app.recovery.simulate_crash();
    drop(app);
    let mut next = session(&dir, artwork(ColorMode::Rgba));
    let orphan = next.recovery.find_orphan().expect("found");
    let bytes = std::fs::read(&orphan.project).expect("read");
    std::fs::write(&orphan.project, &bytes[..bytes.len() / 2]).expect("cut");
    assert!(next.restore_orphan(&orphan).is_err());
    assert_eq!(next.editor.doc().layers()[0].mode, ColorMode::Rgba);
    let kept = Recovery::set_aside(&orphan);
    assert!(kept.is_file(), "a damaged copy is set aside, not destroyed");
    drop(next);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn leftovers_holding_no_work_are_cleared_and_live_sessions_are_not() {
    let dir = temp_dir();
    let mut crashed = Recovery::in_dir(&dir);
    crashed.simulate_crash();
    let mut unfinished = Recovery::in_dir(&dir);
    unfinished.simulate_crash();
    let live = Recovery::in_dir(&dir);
    let files = || std::fs::read_dir(&dir).expect("dir").count();
    assert_eq!(files(), 3);
    live.sweep();
    assert_eq!(files(), 1, "only the running session's lock remains");
    drop(live);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A session on a real file, with a move in progress.
fn editing_file(dir: &std::path::Path) -> (App, PathBuf) {
    std::fs::create_dir_all(dir).expect("dir");
    let file = dir.join("hero.piks");
    std::fs::write(&file, b"as opened").expect("file");
    let mut app = session(dir, artwork(ColorMode::Palette));
    app.path = Some(file.clone());
    app.recovery.note_origin(Some(&file));
    select(&mut app);
    app.nudge(7, 5, false);
    (app, file)
}

#[test]
fn a_restored_copy_keeps_its_file_when_nothing_has_touched_it() {
    let dir = temp_dir();
    let (mut app, file) = editing_file(&dir);
    write_copy_now(&mut app);
    let next = crash_and_return(app, &dir);
    assert_eq!(next.path, Some(file));
}

#[test]
fn a_file_changed_after_the_crash_is_not_saved_over() {
    let dir = temp_dir();
    let (mut app, file) = editing_file(&dir);
    write_copy_now(&mut app);
    app.recovery.simulate_crash();
    drop(app);
    std::fs::write(&file, b"saved later by someone else").expect("newer");
    let mut next = session(&dir, artwork(ColorMode::Palette));
    let orphan = next.recovery.find_orphan().expect("left behind");
    next.restore_orphan(&orphan).expect("restores");
    assert_eq!(next.path, None, "Save has to ask");
    assert_eq!(
        std::fs::read(&file).expect("file"),
        b"saved later by someone else"
    );
}

#[test]
fn a_file_changed_before_the_copy_was_written_is_not_saved_over() {
    let dir = temp_dir();
    let (mut app, file) = editing_file(&dir);
    std::fs::write(&file, b"another instance saved first").expect("newer");
    write_copy_now(&mut app);
    let next = crash_and_return(app, &dir);
    assert_eq!(next.path, None);
}

#[test]
fn crashing_again_after_a_restore_still_knows_the_file_changed() {
    let dir = temp_dir();
    let (mut app, file) = editing_file(&dir);
    write_copy_now(&mut app);
    app.recovery.simulate_crash();
    drop(app);
    std::fs::write(&file, b"newer").expect("newer");
    let mut next = session(&dir, artwork(ColorMode::Palette));
    let orphan = next.recovery.find_orphan().expect("left behind");
    next.restore_orphan(&orphan).expect("restores");
    next.recovery.simulate_crash();
    drop(next);
    let last = session(&dir, artwork(ColorMode::Palette));
    let orphan = last.recovery.find_orphan().expect("still there");
    assert!(!orphan.may_save_over_origin());
}

/// A session on a file holding what this version cannot keep, so Save must not replace it.
fn editing_protected_file(dir: &std::path::Path) -> (App, PathBuf) {
    let (mut app, file) = editing_file(dir);
    app.path = None;
    app.protected_path = Some(file.clone());
    (app, file)
}

#[test]
fn work_on_a_file_that_must_not_be_replaced_stays_protected_after_a_crash() {
    let dir = temp_dir();
    let (mut app, file) = editing_protected_file(&dir);
    write_copy_now(&mut app);
    app.recovery.simulate_crash();
    drop(app);
    let mut next = session(&dir, artwork(ColorMode::Palette));
    let orphan = next.recovery.find_orphan().expect("left behind");
    assert_eq!(
        orphan.display_name(),
        "hero.piks",
        "offered under its own name"
    );
    assert!(!orphan.may_save_over_origin());
    next.restore_orphan(&orphan).expect("restores");
    assert_eq!(next.path, None, "Save has to ask");
    assert!(next.is_protected(&file), "and may not choose the original");
    assert_eq!(std::fs::read(&file).expect("file"), b"as opened");
}

#[test]
fn crashing_again_keeps_a_file_that_must_not_be_replaced_protected() {
    let dir = temp_dir();
    let (mut app, file) = editing_protected_file(&dir);
    write_copy_now(&mut app);
    let mut next = crash_and_return(app, &dir);
    next.nudge(1, 0, false);
    write_copy_now(&mut next);
    let last = crash_and_return(next, &dir);
    assert_eq!(last.path, None);
    assert!(last.is_protected(&file));
}

#[test]
fn a_file_that_must_not_be_replaced_stays_protected_after_saving_elsewhere_and_crashing() {
    let dir = temp_dir();
    let (mut app, file) = editing_protected_file(&dir);
    let copy = dir.join("copy.piks");
    std::fs::write(&copy, b"saved as a copy").expect("copy");
    app.path = Some(copy.clone());
    app.recovery.note_origin(Some(&copy));
    write_copy_now(&mut app);
    let next = crash_and_return(app, &dir);
    assert_eq!(next.path, Some(copy), "Save goes on writing the copy");
    assert!(
        next.is_protected(&file),
        "and still may not choose the original"
    );
}

#[test]
fn a_file_named_from_another_folder_is_restored_to_the_same_file() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("hero.piks");
    std::fs::write(&file, b"as opened").expect("file");
    let here = std::env::current_dir().expect("working folder");
    let relative = pathdiff(&file, &here);
    let mut app = session(&dir, artwork(ColorMode::Palette));
    app.path = None;
    app.protected_path = Some(relative);
    select(&mut app);
    app.nudge(2, 0, false);
    write_copy_now(&mut app);
    app.recovery.simulate_crash();
    drop(app);
    let next = session(&dir, artwork(ColorMode::Palette));
    let orphan = next.recovery.find_orphan().expect("left behind");
    assert_eq!(
        orphan.origin.as_deref(),
        Some(file.as_path()),
        "recorded in full"
    );
}

/// `to` written relative to `from`, going up as far as needed.
fn pathdiff(to: &std::path::Path, from: &std::path::Path) -> PathBuf {
    let (to, from): (Vec<_>, Vec<_>) = (to.components().collect(), from.components().collect());
    let shared = to.iter().zip(&from).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in shared..from.len() {
        out.push("..");
    }
    to[shared..].iter().for_each(|c| out.push(c));
    out
}

#[test]
fn a_recovered_document_stays_modified_through_every_undo() {
    let dir = temp_dir();
    let mut app = session(&dir, artwork(ColorMode::Palette));
    select(&mut app);
    app.nudge(7, 5, false);
    write_copy_now(&mut app);
    let mut back = crash_and_return(app, &dir);
    assert!(back.is_modified());
    back.editor.settle_floating();
    while back.editor.history().can_undo() {
        back.undo();
        assert!(back.is_modified(), "recovered work differs from any file");
    }
    while back.editor.history().can_redo() {
        back.redo();
        assert!(back.is_modified());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
