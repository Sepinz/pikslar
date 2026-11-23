//! No words are cut off by the slot that holds them: every text drawn in an open dock slot lies
//! wholly inside it, in both faces (Large text is what 150 % Windows text turns on), on every
//! preset at 1x and 2x, and with each panel no preset shows opened into Draw.

use crate::app::App;
use crate::test_app::app;
use crate::workspace::model::{Panel, Preset, Side, Slot};
use pikslar_platform::Window;
use pikslar_render::{Rect, Surface};
use pikslar_ui::font;

const SCENES: [(i32, i32, i32); 2] = [(1366, 705, 1), (1920, 1003, 2)];
const PRESETS: [Preset; 4] = [
    Preset::Draw,
    Preset::Animate,
    Preset::Tiles,
    Preset::Palette,
];

fn draw_logged(a: &mut App, (w, h): (i32, i32)) -> font::DrawLog {
    let mut px = vec![0u32; (w * h) as usize];
    for pass in 0..2 {
        font::log_text(pass == 1);
        let mut surface = Surface { px: &mut px, w, h };
        a.draw_interface(&mut surface, Window::detached(), &mut Vec::new());
    }
    let log = font::take_text_log();
    font::log_text(false);
    log
}

/// The texts whose middle is in an open slot but which reach past its top or bottom.
fn cut_off(a: &App, log: &font::DrawLog) -> Vec<String> {
    let slots: Vec<(Rect, String)> = [Side::Left, Side::Right, Side::Bottom]
        .into_iter()
        .flat_map(|side| a.layout.solved.area(side).slots().to_vec())
        .filter(|sl| sl.open && sl.rect.w > 0 && sl.rect.h > 0)
        .map(|sl| (sl.rect, format!("{:?}", sl.panel)))
        .collect();
    let mut found = Vec::new();
    for (t, words) in &log.texts {
        let (cx, cy) = (t.x + t.w / 2, t.y + t.h / 2);
        let Some((r, panel)) = slots.iter().find(|(r, _)| r.contains(cx, cy)) else {
            continue;
        };
        if t.y < r.y || t.bottom() > r.bottom() {
            found.push(format!("{words:?} {t:?} cut by {panel} slot {r:?}"));
        }
    }
    found
}

/// Panels no preset shows in front: opened from the Window menu into Draw, or, where Draw has
/// them behind another tab, brought to the front of their slot.
const EXTRA: [Panel; 5] = [
    Panel::History,
    Panel::Preview,
    Panel::Slices,
    Panel::Scripts,
    Panel::Lint,
];

fn bring_forward(a: &mut App, p: Panel) {
    match a.workspaces.active().find(p) {
        Some((side, i, t)) => a.edit_layout(|ws| {
            crate::dock::with_slot(ws, side, i, |sl| Slot {
                active: t as u8,
                ..sl
            })
        }),
        None => a.toggle_panel(p),
    }
}

fn check(large: bool) {
    let mut bad = Vec::new();
    let scenes = PRESETS
        .into_iter()
        .map(|p| (p, None))
        .chain(EXTRA.into_iter().map(|p| (Preset::Draw, Some(p))));
    let scenes: Vec<(Preset, Option<Panel>)> = scenes.collect();
    for (w, h, scale) in SCENES {
        for &(preset, extra) in &scenes {
            let mut a = app(6);
            a.resized(w, h, scale);
            a.layer_action(0);
            a.layer_action(0);
            a.choose_preset(preset);
            if let Some(p) = extra {
                bring_forward(&mut a, p);
            }
            a.set_large_text(large);
            let log = draw_logged(&mut a, (w, h));
            if let Some(p) = extra {
                let solved = &a.layout.solved.effective;
                let shown = solved.find(p).is_some_and(|(side, i, t)| {
                    usize::from(solved.area(side).slots()[i].active) == t
                });
                assert!(shown, "{p:?} was not shown at {w}x{h}");
            }
            for line in cut_off(&a, &log) {
                bad.push(format!("{preset:?}+{extra:?} {w}x{h} at {scale}: {line}"));
            }
        }
    }
    font::set_face(font::Face::Text);
    bad.sort();
    bad.dedup();
    assert!(bad.is_empty(), "{} cut off:\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn no_words_are_cut_off_by_their_slot_in_pikslar_text() {
    check(false);
}

#[test]
fn no_words_are_cut_off_by_their_slot_in_large_text() {
    check(true);
}

fn open_panels(a: &App) -> Vec<String> {
    [Side::Left, Side::Right, Side::Bottom]
        .into_iter()
        .flat_map(|side| a.layout.solved.area(side).slots().to_vec())
        .filter(|sl| sl.open)
        .map(|sl| format!("{:?}", sl.panel))
        .collect()
}

/// Showing the empty Tiles panel's words whole costs the panels beside it nothing: its least
/// stays its header and one row of tiles in both faces, and Large text folds no panel of the
/// Tiles preset, the Palette above all. (Draw's Palette folding at 2x in Large text is the dock
/// system's own fit and is unchanged.)
#[test]
fn the_empty_tiles_message_takes_no_room_from_the_palette() {
    for large in [false, true] {
        font::set_face(if large {
            font::Face::Mono
        } else {
            font::Face::Text
        });
        let least = crate::panel_tiles::body_least();
        font::set_face(font::Face::Text);
        assert_eq!(least, 60, "large text {large}");
    }
    let mut bad = Vec::new();
    for (w, h, scale) in SCENES {
        let mut a = app(6);
        a.resized(w, h, scale);
        a.choose_preset(Preset::Tiles);
        a.set_large_text(false);
        let _ = draw_logged(&mut a, (w, h));
        let small = open_panels(&a);
        a.set_large_text(true);
        let _ = draw_logged(&mut a, (w, h));
        let large = open_panels(&a);
        for p in small.iter().filter(|p| !large.contains(p)) {
            bad.push(format!("{w}x{h} at {scale}: {p} folds"));
        }
    }
    font::set_face(font::Face::Text);
    assert!(bad.is_empty(), "{bad:?}");
}
