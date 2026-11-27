use crate::app::App;
use crate::test_app::*;
use pikslar_core::Document;
use pikslar_platform::Window;
use std::fs;
use std::path::{Path, PathBuf};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-rewind-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    dir
}

/// An app with Rewind on, its store in `dir`.
fn with_rewind(dir: &Path) -> App {
    let mut a = app(1);
    a.settings_saved.rewind = true;
    a.use_rewind(Some(dir.join("store")));
    assert!(a.rewind.on);
    a
}

/// Draws `n` dots, keeping every state, the one before the first included.
fn strokes(a: &mut App, n: i32) -> Vec<Document> {
    let mut states = vec![a.editor.doc().clone()];
    for i in 0..n {
        dot(a, 2 + i, 3 + i % 4);
        states.push(a.editor.doc().clone());
    }
    states
}

/// The same file, whatever id each was given.
fn same(a: &Document, b: &Document) -> bool {
    let bare = |d: &Document| {
        let mut d = d.clone();
        d.document_id = Default::default();
        pikslar_format::piks::save(&d)
    };
    bare(a) == bare(b)
}

/// Save As: the file open now stays as it is.
fn save_as(a: &mut App, path: &Path) {
    assert!(a.ready_to_save());
    assert!(a.save_to(Window::detached(), path.to_path_buf()), "saved");
    a.finish_rewind();
}

fn save_at(a: &mut App, path: &Path) {
    a.path = Some(path.to_path_buf());
    assert!(a.save(Window::detached(), false), "saved");
    a.finish_rewind();
}

fn reopened(dir: &Path, path: &Path) -> App {
    let mut b = with_rewind(dir);
    b.open_now(Window::detached(), path.to_path_buf());
    b
}

#[test]
fn rewind_is_off_unless_turned_on_and_then_keeps_nothing() {
    let dir = temp_dir("off");
    let mut a = app(1);
    a.use_rewind(Some(dir.join("store")));
    assert!(!a.rewind.on);
    strokes(&mut a, 3);
    save_at(&mut a, &dir.join("a.piks"));
    assert!(!dir.join("store").exists(), "nothing written");
    assert_eq!(a.editor.doc().document_id.get(), None, "no id given");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_reopened_file_undoes_into_the_kept_steps_and_redoes_back() {
    let dir = temp_dir("reopen");
    let path = dir.join("walk.piks");
    let mut a = with_rewind(&dir);
    let states = strokes(&mut a, 6);
    save_at(&mut a, &path);
    let mut b = reopened(&dir, &path);
    assert_eq!(b.rewind_head().map(|h| h.tip), Some(6));
    for state in states.iter().rev().skip(1) {
        b.undo();
        assert!(same(b.editor.doc(), state), "undone to an earlier state");
    }
    b.undo();
    assert!(
        same(b.editor.doc(), &states[0]),
        "nothing older: undo does nothing"
    );
    for state in &states[1..] {
        b.redo();
        assert!(same(b.editor.doc(), state));
    }
    // Undoing into kept steps and drawing replaces what was above, at the next save.
    for _ in 0..3 {
        b.undo();
    }
    dot(&mut b, 30, 30);
    let newest = b.editor.doc().clone();
    save_at(&mut b, &path);
    assert_eq!(b.rewind_head().map(|h| h.tip), Some(4));
    let mut c = reopened(&dir, &path);
    c.undo();
    assert!(same(c.editor.doc(), &states[3]));
    c.redo();
    assert!(same(c.editor.doc(), &newest));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_file_changed_elsewhere_attaches_nothing_and_its_history_stays() {
    let dir = temp_dir("changed");
    let path = dir.join("walk.piks");
    let mut a = with_rewind(&dir);
    strokes(&mut a, 3);
    save_at(&mut a, &path);
    let kept = a.rewind_head();
    // Another program changes the file, keeping its id.
    dot(&mut a, 20, 20);
    crate::files::save(&path, a.editor.doc()).expect("saved");
    let mut b = reopened(&dir, &path);
    assert_eq!(b.rewind_head(), None);
    assert!(said(&b).contains("not used"), "{}", said(&b));
    let store = b.rewind.store().expect("store");
    let id = b.editor.doc().document_id.get().expect("id");
    assert_eq!(store.read_head(id).expect("reads"), kept, "untouched");
    // Its next save gives the file an id of its own and history of its own.
    dot(&mut b, 21, 21);
    save_at(&mut b, &path);
    let new_id = b.editor.doc().document_id.get().expect("id");
    assert_ne!(new_id, id);
    assert_eq!(store.read_head(id).expect("reads"), kept, "still untouched");
    assert!(store.has_history(new_id));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_store_that_cannot_be_written_never_fails_or_changes_a_save() {
    let dir = temp_dir("blocked");
    // A file where the store's folder should be.
    fs::write(dir.join("store"), b"in the way").expect("written");
    let path = dir.join("walk.piks");
    let mut a = with_rewind(&dir);
    let states = strokes(&mut a, 4);
    save_at(&mut a, &path);
    assert!(said(&a).contains("The file is saved"), "{}", said(&a));
    let opened = crate::files::open(&path).expect("opens");
    assert!(
        same(&opened.doc, states.last().expect("state")),
        "saved as drawn"
    );
    for state in states.iter().rev().skip(1) {
        a.undo();
        assert!(same(a.editor.doc(), state), "undo in memory is as ever");
    }
    // Steps not written are written by the next save that can.
    let _ = fs::remove_file(dir.join("store"));
    for _ in 0..4 {
        a.redo();
    }
    save_at(&mut a, &path);
    assert_eq!(a.rewind_head().map(|h| h.tip), Some(4));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_history_another_pikslar_holds_is_left_and_said_calmly() {
    let dir = temp_dir("held");
    let path = dir.join("walk.piks");
    let mut a = with_rewind(&dir);
    strokes(&mut a, 2);
    save_at(&mut a, &path);
    let store = a.rewind.store().expect("store");
    let id = a.editor.doc().document_id.get().expect("id");
    let lock = store.lock(id).expect("held");
    dot(&mut a, 9, 9);
    let started = std::time::Instant::now();
    save_at(&mut a, &path);
    assert!(started.elapsed().as_secs() < 5, "no wait for the other");
    assert!(said(&a).contains("another Pikslar"), "{}", said(&a));
    drop(lock);
    dot(&mut a, 10, 10);
    save_at(&mut a, &path);
    assert_eq!(
        a.rewind_head().map(|h| h.tip),
        Some(4),
        "the missed step too"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn save_as_gives_the_new_file_its_own_history_from_the_old() {
    let dir = temp_dir("save-as");
    let (first, second) = (dir.join("a.piks"), dir.join("b.piks"));
    let mut a = with_rewind(&dir);
    let states = strokes(&mut a, 3);
    save_at(&mut a, &first);
    let old = a.editor.doc().document_id.get().expect("id");
    dot(&mut a, 25, 25);
    save_as(&mut a, &second);
    let new = a.editor.doc().document_id.get().expect("id");
    assert_ne!(old, new);
    let mut b = reopened(&dir, &second);
    for state in states.iter().rev() {
        b.undo();
        assert!(same(b.editor.doc(), state));
    }
    let c = reopened(&dir, &first);
    assert_eq!(
        c.rewind_head().map(|h| h.tip),
        Some(3),
        "the old file keeps its own"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn forgetting_removes_the_history_and_nothing_else() {
    let dir = temp_dir("forget");
    let path = dir.join("walk.piks");
    let mut a = with_rewind(&dir);
    strokes(&mut a, 2);
    save_at(&mut a, &path);
    let bytes = fs::read(&path).expect("saved");
    a.forget_rewind();
    assert_eq!(said(&a), "Forgot this file's Rewind history");
    let mut b = reopened(&dir, &path);
    assert_eq!(b.rewind_head(), None);
    b.undo();
    assert_eq!(
        fs::read(&path).expect("saved"),
        bytes,
        "the file is untouched"
    );
    save_at(&mut b, &path);
    b.forget_all_rewind();
    assert_eq!(said(&b), "Forgot all Rewind history");
    assert!(b.rewind.store().expect("store").projects().is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn turning_rewind_on_and_off_is_a_setting_that_is_kept() {
    let dir = temp_dir("setting");
    let mut a = app(1);
    a.use_settings(crate::settings::SettingsFile::in_dir(&dir));
    a.use_rewind(Some(dir.join("store")));
    a.toggle_rewind();
    assert!(a.rewind.on);
    assert!(said(&a).contains("never in the file"), "{}", said(&a));
    a.tick_settings();
    let mut b = app(1);
    b.use_settings(crate::settings::SettingsFile::in_dir(&dir));
    b.use_rewind(Some(dir.join("store")));
    assert!(b.rewind.on);
    b.toggle_rewind();
    assert!(!b.rewind.on);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_view_menu_turns_rewind_on_and_forgets() {
    let dir = temp_dir("menu");
    let mut a = app(1);
    a.use_rewind(Some(dir.join("store")));
    let at = |a: &App, label: &str| {
        let items = a.view_items();
        items.iter().position(|i| i.label.starts_with(label))
    };
    let on = at(&a, "Keep undo history").expect("offered");
    assert!(
        !a.view_items()[on + 1].enabled,
        "nothing to forget without an id"
    );
    a.view_menu_action(on, Window::detached());
    assert!(a.rewind.on);
    assert_eq!(at(&a, "Stop keeping"), Some(on));
    strokes(&mut a, 2);
    save_at(&mut a, &dir.join("m.piks"));
    assert!(a.view_items()[on + 1].enabled);
    a.view_menu_action(on + 1, Window::detached());
    assert!(said(&a).starts_with("Forgot this file"));
    // Every file's history goes only once the person says Yes (review finding M2).
    strokes(&mut a, 1);
    save_at(&mut a, &dir.join("m.piks"));
    a.ask_forget_all = |_| false;
    a.view_menu_action(on + 2, Window::detached());
    assert_eq!(said(&a), "Kept all Rewind history");
    assert!(a.view_items()[on + 1].enabled, "the history is still there");
    a.ask_forget_all = |_| true;
    a.view_menu_action(on + 2, Window::detached());
    assert_eq!(said(&a), "Forgot all Rewind history");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn undo_over_floating_pixels_after_reopening_still_reaches_the_kept_steps() {
    let dir = temp_dir("floating");
    let path = dir.join("walk.piks");
    let mut a = with_rewind(&dir);
    let states = strokes(&mut a, 3);
    save_at(&mut a, &path);
    let mut b = reopened(&dir, &path);
    b.run(pikslar_core::FloatingCmd::Paste {
        clip: std::sync::Arc::new(pikslar_core::Clip::from_image(1, 1, vec![0xff00_00ff])),
        x: 1,
        y: 1,
    });
    assert!(b.editor.is_floating());
    b.undo();
    while b.editor.is_floating() || !same(b.editor.doc(), &states[3]) {
        assert!(
            b.editor.history().undo_depth() > 0,
            "the paste is undone in memory"
        );
        b.undo();
    }
    b.undo();
    assert!(
        same(b.editor.doc(), &states[2]),
        "and then a kept step: {}",
        said(&b)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn turning_rewind_off_and_on_again_joins_the_kept_steps_once() {
    let dir = temp_dir("off-on");
    let path = dir.join("walk.piks");
    let mut a = with_rewind(&dir);
    let states = strokes(&mut a, 3);
    save_at(&mut a, &path);
    let mut b = reopened(&dir, &path);
    b.toggle_rewind();
    b.toggle_rewind();
    assert!(b.rewind.on);
    assert_eq!(b.editor.rewind_marks().below, 3, "positions moved up once");
    dot(&mut b, 30, 30);
    save_at(&mut b, &path);
    assert_eq!(b.rewind_head().map(|h| h.tip), Some(4));
    let mut c = reopened(&dir, &path);
    for state in states.iter().rev() {
        c.undo();
        assert!(same(c.editor.doc(), state), "kept history is whole");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_save_as_that_fails_changes_nothing_of_the_history() {
    let dir = temp_dir("save-as-fails");
    let (first, second) = (dir.join("a.piks"), dir.join("b.piks"));
    let mut a = with_rewind(&dir);
    let states = strokes(&mut a, 3);
    save_at(&mut a, &first);
    let (id, head) = (a.editor.doc().document_id.get(), a.rewind_head());
    assert!(a.ready_to_save());
    let nowhere = dir.join("no such folder").join("c.piks");
    assert!(!a.save_to(Window::detached(), nowhere), "not saved");
    assert_eq!(a.editor.doc().document_id.get(), id, "the id is as it was");
    assert_eq!(a.rewind_head(), head, "and so is the history");
    save_as(&mut a, &second);
    assert_ne!(a.editor.doc().document_id.get(), id);
    let mut b = reopened(&dir, &second);
    for state in states.iter().rev().skip(1) {
        b.undo();
        assert!(
            same(b.editor.doc(), state),
            "the copy reaches the old steps"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn turning_rewind_on_joins_the_history_of_the_file_as_it_was_opened() {
    let dir = temp_dir("opened-print");
    let path = dir.join("walk.piks");
    let mut a = with_rewind(&dir);
    strokes(&mut a, 3);
    save_at(&mut a, &path);
    let mut b = app(1);
    b.use_rewind(Some(dir.join("store")));
    assert!(!b.rewind.on);
    b.open_now(Window::detached(), path.clone());
    // Another window saves on, so the file and its history move past what this one opened.
    dot(&mut a, 30, 30);
    save_at(&mut a, &path);
    b.toggle_rewind();
    assert_ne!(
        b.rewind_head().map(|h| h.tip),
        Some(4),
        "the newer history is not this document's"
    );
    let mut c = reopened(&dir, &path);
    c.toggle_rewind();
    c.toggle_rewind();
    assert_eq!(
        c.rewind_head().map(|h| h.tip),
        Some(4),
        "as opened, it joins"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// A 512 by 512 picture of noise, with `strokes` dots and `layers` layers added on it.
fn large_history(a: &mut App, strokes: i32, layers: usize) {
    let n = 512usize;
    let px = (0..n * n)
        .map(|i| 0xff00_0000 | ((i as u32).wrapping_mul(2_654_435_761) >> 8))
        .collect();
    let image = pikslar_format::png::Image {
        width: n,
        height: n,
        px,
    };
    let doc = crate::files::document_from_image(&image).expect("a document");
    a.replace_document(doc, None);
    for i in 0..strokes {
        dot(a, i * 7 % 500, i * 13 % 500);
        if (i as usize) < layers {
            a.run(pikslar_core::LayerCmd::Add {
                at: 1,
                mode: pikslar_core::ColorMode::Rgba,
            });
        }
    }
}

/// How long a save takes the interface with a large history of `layers` structural steps
/// among 200: Rewind off, then on for the first save, which keeps every step, and the next,
/// after one more stroke. Medians of five.
fn save_latency(dir: &Path, layers: usize) -> [std::time::Duration; 3] {
    let path = dir.join("large.piks");
    let timed = |a: &mut App| {
        let started = std::time::Instant::now();
        assert!(a.save(Window::detached(), false));
        started.elapsed()
    };
    let mut runs: [Vec<_>; 3] = Default::default();
    for _ in 0..5 {
        let _ = fs::remove_dir_all(dir.join("store"));
        let mut plain = app(1);
        large_history(&mut plain, 200, layers);
        plain.path = Some(path.clone());
        runs[0].push(timed(&mut plain));
        let mut a = with_rewind(dir);
        large_history(&mut a, 200, layers);
        a.path = Some(path.clone());
        runs[1].push(timed(&mut a));
        dot(&mut a, 3, 3);
        runs[2].push(timed(&mut a));
        a.finish_rewind();
        assert!(a.rewind_head().is_some_and(|h| h.tip > 200), "{}", said(&a));
    }
    runs.map(|mut run| {
        run.sort();
        run[2]
    })
}

#[test]
#[ignore = "a measurement: cargo test --release -p pikslar rewind_save_latency -- --ignored"]
fn rewind_save_latency() {
    let dir = temp_dir("latency");
    for layers in [0, 12] {
        let [off, first, next] = save_latency(&dir, layers);
        eprintln!("{layers} whole: off {off:?}, first {first:?}, next {next:?}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// A write that holds on until it is let go, standing in for a slow disk.
fn slow_write(a: &mut App) -> std::sync::mpsc::Sender<()> {
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let handle = std::thread::spawn(move || {
        let _ = wait.recv();
        Err(pikslar_rewind::RewindError::InUse)
    });
    let id = a.editor.doc().document_id.get().expect("id");
    a.rewind.job = Some(super::Job {
        handle,
        id,
        from: u64::MAX,
        left: false,
        started: std::time::Instant::now(),
    });
    go
}

#[test]
fn a_save_never_waits_for_a_write_under_way_and_its_steps_follow_it() {
    let dir = temp_dir("no-wait");
    let path = dir.join("walk.piks");
    let mut a = with_rewind(&dir);
    let states = strokes(&mut a, 2);
    save_at(&mut a, &path);
    let go = slow_write(&mut a);
    dot(&mut a, 30, 30);
    within(20, "a save during a slow Rewind write", || {
        assert!(a.save(Window::detached(), false), "saved")
    });
    assert!(a.rewind.job.is_some(), "the slow write is still under way");
    let _ = go.send(());
    a.finish_rewind();
    assert_eq!(a.rewind_head().map(|h| h.tip), Some(3), "{}", said(&a));
    let mut b = reopened(&dir, &path);
    b.undo();
    for state in states.iter().rev().skip(1) {
        b.undo();
        assert!(same(b.editor.doc(), state));
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn undo_into_kept_steps_never_waits_for_a_write_under_way() {
    let dir = temp_dir("undo-no-wait");
    let path = dir.join("walk.piks");
    let mut a = with_rewind(&dir);
    let states = strokes(&mut a, 2);
    save_at(&mut a, &path);
    let mut b = reopened(&dir, &path);
    let go = slow_write(&mut b);
    within(20, "undo during a slow Rewind write", || b.undo());
    assert!(same(b.editor.doc(), &states[1]), "{}", said(&b));
    let _ = go.send(());
    b.finish_rewind();
    let _ = fs::remove_dir_all(&dir);
}

/// Review finding M1 (RC3): with undo memory full, undo stops at the kept steps with a word,
/// Rewind is not taken as stuck, and redo reaches the newest work.
#[test]
fn undo_into_kept_steps_stops_with_a_word_once_memory_is_full() {
    let dir = temp_dir("full");
    let path = dir.join("full.piks");
    let mut a = with_rewind(&dir);
    let states = strokes(&mut a, 6);
    save_at(&mut a, &path);
    let mut b = reopened(&dir, &path);
    b.editor.set_undo_budget(1);
    b.undo();
    assert!(
        same(b.editor.doc(), &states[5]),
        "one kept step is taken back"
    );
    b.undo();
    assert!(same(b.editor.doc(), &states[5]), "then undo stops");
    assert!(
        said(&b).starts_with("Undo stops here for now"),
        "{}",
        said(&b)
    );
    assert!(!said(&b).contains("  "), "no runs of spaces: {}", said(&b));
    assert!(!b.rewind.stuck, "Rewind is not stuck");
    b.redo();
    assert!(
        same(b.editor.doc(), &states[6]),
        "redo reaches the newest work"
    );
    let _ = fs::remove_dir_all(&dir);
}
