//! The minute-276 saving stall (B1 Batch 1, docs/b1-stall-investigation.md): what a key read
//! under the wrong modifier does to the soak's workload.
//!
//! The soak posts plain letters and sets Ctrl separately, and the program reads Ctrl when it
//! handles each key, so a plain E or N still queued when Ctrl goes down arrives as Ctrl+E or
//! Ctrl+N. These tests drive the app through the window's own events, on the soak's project,
//! zoom and client size, and record what follows.
//!
//! Before Batch 2R, while a drawn dialog had the keyboard, Ctrl+S wrote nothing and said
//! nothing. It still writes nothing under the dialog, and now says so on the status bar
//! (docs/b1-plan.md, section 13, requirement 2; `save_keys.rs`).

use crate::app::App;
use crate::test_app::{draw_whole_at, said};
use pikslar_platform::{Button, Event, Handler, Key, Mods, Window};
use pikslar_render::{Rect, Surface};
use std::path::PathBuf;

/// The soak's client area: `--window 100 60 1400 900` at 100 % scaling, as its layout dump shows.
const SOAK_CLIENT: (i32, i32) = (1384, 861);
const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};
const PLAIN: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: false,
};
/// Where the soak's strokes start: x 520 to 759, y 300 to 499 (`Scribble` in spikes/soak.ps1).
const STROKE_START: Rect = Rect {
    x: 520,
    y: 300,
    w: 240,
    h: 200,
};

/// A copy of the soak's project in a folder of its own, opened at the soak's zoom and size.
fn soak_app(name: &str) -> (App, PathBuf) {
    let dir = std::env::temp_dir().join(format!("pikslar-stall-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a folder");
    let corpus = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../pikslar-safety/corpus/v3-groups.piks"
    );
    let art = dir.join("work.piks");
    std::fs::copy(corpus, &art).expect("the soak's project");
    let mut a = App::launch(Some(art.clone()), None, Some(8));
    a.resized(SOAK_CLIENT.0, SOAK_CLIENT.1, 1);
    frame(&mut a);
    (a, art)
}

fn save_label(a: &App) -> String {
    a.save_label().map(|l| l.text).unwrap_or_default()
}

fn frame(a: &mut App) {
    let (w, h) = SOAK_CLIENT;
    let mut px = vec![0u32; (w * h) as usize];
    let mut surface = Surface { px: &mut px, w, h };
    a.frame(&mut surface, Window::detached(), &mut Vec::new());
}

fn press(a: &mut App, letter: u8, mods: Mods) {
    let key = Key::Char(letter);
    a.event(
        Event::KeyDown {
            key,
            mods,
            repeat: false,
        },
        Window::detached(),
    );
    a.event(Event::KeyUp { key }, Window::detached());
    frame(a);
}

/// One of the soak's strokes: down at the start, two moves, up at the end.
fn stroke(a: &mut App, points: [(i32, i32); 3]) {
    let w = Window::detached();
    let [(x0, y0), (x1, y1), (x2, y2)] = points;
    a.event(Event::Move { x: x0, y: y0 }, w);
    let (button, mods) = (Button::Left, PLAIN);
    a.event(
        Event::Down {
            x: x0,
            y: y0,
            button,
            mods,
        },
        w,
    );
    a.event(Event::Move { x: x1, y: y1 }, w);
    a.event(Event::Move { x: x2, y: y2 }, w);
    a.event(
        Event::Up {
            x: x2,
            y: y2,
            button,
        },
        w,
    );
    frame(a);
}

/// Strokes at the corners and middle of the soak's stroke area.
const STROKES: [[(i32, i32); 3]; 3] = [
    [(520, 300), (600, 340), (550, 240)],
    [(640, 400), (560, 440), (670, 460)],
    [(759, 499), (839, 539), (789, 559)],
];

/// Saving, undo and strokes as the soak uses them, with a drawn dialog open.
fn assert_dialog_swallows_the_workload(a: &mut App, art: &std::path::Path, what: &str) {
    a.message = None;
    std::fs::remove_file(art).expect("the control save's file removed");
    press(a, b'S', CTRL);
    assert!(
        !art.exists(),
        "{what}: Ctrl+S wrote nothing under the dialog"
    );
    assert_eq!(
        save_label(a),
        "Not saved: close the Export dialog first (Esc)",
        "{what}: Ctrl+S says why"
    );
    let revision = a.editor.revision();
    press(a, b'Z', CTRL);
    assert_eq!(
        a.editor.revision(),
        revision,
        "{what}: Ctrl+Z changed nothing"
    );
    for points in STROKES {
        stroke(a, points);
    }
    assert_eq!(
        a.editor.revision(),
        revision,
        "{what}: no stroke changed a pixel"
    );
}

/// The control: without a dialog, the soak's Ctrl+S saves and its strokes draw.
fn assert_the_workload_works(a: &mut App, art: &std::path::Path) {
    press(a, b'S', CTRL);
    crate::test_app::finish_saves(a);
    assert!(
        said(a).starts_with("Saved"),
        "the control save: {}",
        said(a)
    );
    assert!(art.exists());
    let revision = a.editor.revision();
    stroke(a, STROKES[1]);
    assert_ne!(
        a.editor.revision(),
        revision,
        "a soak stroke draws on the canvas"
    );
    let drawn = a.editor.revision();
    press(a, b'Z', CTRL);
    assert_ne!(a.editor.revision(), drawn, "and Ctrl+Z undoes it");
}

#[test]
fn a_plain_e_read_under_ctrl_opens_export_which_then_swallows_save_undo_and_strokes() {
    let (mut a, art) = soak_app("export");
    assert_the_workload_works(&mut a, &art);
    press(&mut a, b'E', CTRL);
    assert!(
        a.export_dialog.is_some(),
        "Ctrl+E opens the drawn Export panel"
    );
    assert_dialog_swallows_the_workload(&mut a, &art, "Export");
    for letter in *b"RNOPBELU0" {
        press(&mut a, letter, PLAIN);
    }
    assert!(a.export_dialog.is_some(), "no key the soak posts closes it");
    a.event(
        Event::KeyDown {
            key: Key::Escape,
            mods: PLAIN,
            repeat: false,
        },
        Window::detached(),
    );
    frame(&mut a);
    assert!(a.export_dialog.is_none(), "Escape closes it");
    assert_the_workload_works(&mut a, &art);
}

#[test]
fn a_plain_n_read_under_ctrl_opens_new_which_then_swallows_save_undo_and_its_strokes() {
    let (mut a, art) = soak_app("new");
    assert_the_workload_works(&mut a, &art);
    press(&mut a, b'N', CTRL);
    assert!(
        a.new_dialog.is_some(),
        "Ctrl+N opens the drawn New sprite dialog"
    );
    a.message = None;
    std::fs::remove_file(&art).expect("removed");
    press(&mut a, b'S', CTRL);
    assert!(!art.exists(), "New: Ctrl+S wrote nothing under the dialog");
    assert_eq!(
        save_label(&a),
        "Not saved: close the New sprite dialog first (Esc)",
        "New: Ctrl+S says why"
    );
    let revision = a.editor.revision();
    press(&mut a, b'Z', CTRL);
    assert_eq!(a.editor.revision(), revision, "New: Ctrl+Z changed nothing");
    // Strokes that miss its buttons change nothing and leave it open.
    stroke(&mut a, STROKES[0]);
    assert_eq!(
        a.editor.revision(),
        revision,
        "New: a stroke changed no pixel"
    );
}

/// Where a dialog's widgets are, drawn at the soak's size.
fn widget(a: &App, id: u32) -> Rect {
    let found = a.ui.widgets().iter().find(|&&(_, seen)| seen == id);
    found
        .map(|&(r, _)| r)
        .unwrap_or_else(|| panic!("widget {id} not drawn"))
}

fn inside(r: Rect, (x, y): (i32, i32)) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

/// Of every stroke `Scribble` can make (start x 520 to 759, y 300 to 499, end 30 to the right
/// and -60 to 59 down), how many press and release on the same one of `buttons`.
fn soak_clicks(buttons: &[Rect]) -> u64 {
    let (r, mut hits) = (STROKE_START, 0);
    for x in r.x..r.x + r.w {
        for y in r.y..r.y + r.h {
            for b in buttons.iter().filter(|b| inside(**b, (x, y))) {
                hits += (-60..60).filter(|dy| inside(*b, (x + 30, y + dy))).count() as u64;
            }
        }
    }
    hits
}

/// The share of the soak's strokes that click a dialog button, in millionths.
fn per_million(hits: u64) -> u64 {
    hits * 1_000_000 / (STROKE_START.w as u64 * STROKE_START.h as u64 * 120)
}

#[test]
fn no_soak_stroke_can_click_exports_buttons_but_some_click_news() {
    let (mut a, _art) = soak_app("places");
    a.open_export_dialog(false);
    draw_whole_at(&mut a, SOAK_CLIENT.0, SOAK_CLIENT.1);
    let export = crate::export_panel::ID_EXPORT;
    let (go, cancel) = (widget(&a, export + 50), widget(&a, export + 51));
    eprintln!("Export at the soak's size: Export {go:?}, Cancel {cancel:?}");
    assert_eq!(soak_clicks(&[go, cancel]), 0, "every release misses them");
    // A stroke can start on Cancel's right edge, but its release lands 30 to the right.
    let start = (cancel.x + cancel.w - 1, cancel.y);
    assert!(inside(STROKE_START, start), "a stroke can press Cancel");
    stroke(&mut a, [start, start, (start.0 + 30, start.1)]);
    assert!(
        a.export_dialog.is_some(),
        "pressing alone does not close it"
    );
    a.close_export_dialog();
    a.open_new_dialog();
    draw_whole_at(&mut a, SOAK_CLIENT.0, SOAK_CLIENT.1);
    let (create, cancel) = (widget(&a, 180), widget(&a, 181));
    let hits = soak_clicks(&[create, cancel]);
    let share = per_million(hits);
    eprintln!("New: Create {create:?}, Cancel {cancel:?}: {share} per million strokes click one");
    assert!(share > 0, "some soak strokes click New's buttons");
    let middle = (cancel.x + cancel.w / 2, cancel.y + cancel.h / 2);
    stroke(&mut a, [middle, middle, middle]);
    assert!(a.new_dialog.is_none(), "a click on Cancel closes New");
}
