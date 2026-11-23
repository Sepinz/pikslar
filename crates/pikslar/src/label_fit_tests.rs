//! No label's text runs past the control that holds it, in either face, on every preset at 1x
//! and 2x (docs/ui-redesign.md 2.6: control heights follow the text's metrics).

use crate::app::App;
use crate::test_app::app;
use crate::workspace::model::Preset;
use pikslar_platform::Window;
use pikslar_render::{Rect, Surface};
use pikslar_ui::font;
use pikslar_ui::role::Role;

/// Controls with no outline, whose words start at their edge on purpose: the size readout.
const FLAT: [u32; 1] = [50_020];

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

/// The texts that stick out of the smallest control around their middle.
fn overflows(a: &App, log: &font::DrawLog, pad: i32) -> Vec<String> {
    let widgets: Vec<(Rect, u32)> =
        a.ui.widgets()
            .iter()
            .copied()
            .filter(|&(r, id)| {
                let container = a.ui.kind_of(id).is_some_and(|k| {
                    matches!(k.role, Role::TabList | Role::Group | Role::Splitter)
                });
                !container && r.w > 0 && r.h > 0
            })
            .collect();
    // A control is its drawn face or well where it has one, else the area that answers to it.
    let boxes: Vec<(Rect, u32)> = widgets
        .iter()
        .copied()
        .chain(log.boxes.iter().map(|&r| (r, 0)))
        .collect();
    let mut found = Vec::new();
    for (t, words) in &log.texts {
        let (cx, cy) = (t.x + t.w / 2, t.y + t.h / 2);
        let owner = boxes
            .iter()
            .filter(|(r, _)| r.contains(cx, cy))
            .min_by_key(|(r, _)| r.w * r.h);
        let Some(&(r, id)) = owner else { continue };
        if FLAT.contains(&id) {
            continue;
        }
        // Capitals keep three units from every side (cap height plus padding); a descender
        // may hang into the padding but stays inside the outline.
        let caps = font::cap_height() * font::text_scale(pad);
        let gap = 3 * pad;
        let inside = t.x >= r.x + gap
            && t.right() <= r.right() - gap
            && t.y >= r.y + gap
            && t.y + caps <= r.bottom() - gap
            && t.bottom() <= r.bottom() - pad;
        if !inside {
            found.push(format!("{words:?} {t:?} out of control {id} {r:?}"));
        }
    }
    found
}

fn check(face_large: bool) {
    let mut bad = Vec::new();
    for (w, h, scale) in SCENES {
        for (preset, matrix) in PRESETS.into_iter().flat_map(|p| [(p, false), (p, true)]) {
            let mut a = app(3);
            a.ui.record_names();
            a.resized(w, h, scale);
            a.layer_action(0);
            a.layer_action(0);
            a.choose_preset(preset);
            a.set_large_text(face_large);
            if matrix {
                a.toggle_matrix();
            }
            let texts = draw_logged(&mut a, (w, h));
            for line in overflows(&a, &texts, scale) {
                bad.push(format!(
                    "{preset:?} {w}x{h} at {scale} matrix {matrix}: {line}"
                ));
            }
        }
    }
    font::set_face(font::Face::Text);
    bad.sort();
    bad.dedup();
    assert!(
        bad.is_empty(),
        "{} overflows:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

#[test]
fn no_label_runs_past_its_control_in_pikslar_text() {
    check(false);
}

#[test]
fn no_label_runs_past_its_control_in_large_text() {
    check(true);
}

/// Every tab strip is as tall as the face needs (`fit_h`), so words in tabs keep the same
/// padding as words in any other control, and the tabs' words are among the checked texts.
fn check_strips(face_large: bool) {
    let mut bad = Vec::new();
    for (w, h, scale) in SCENES {
        for preset in PRESETS {
            let mut a = app(3);
            a.ui.record_names();
            a.resized(w, h, scale);
            a.choose_preset(preset);
            a.set_large_text(face_large);
            let texts = draw_logged(&mut a, (w, h));
            let want = font::fit_h(20) * scale;
            let mut strips = 0;
            for &(r, id) in a.ui.widgets() {
                let strip = a.ui.kind_of(id).is_some_and(|k| k.role == Role::TabList);
                if strip && r.h > 0 {
                    strips += 1;
                    if r.h != want {
                        bad.push(format!("{preset:?} {w}x{h}: strip {id} is {} high", r.h));
                    }
                }
            }
            let tab_words = texts
                .texts
                .iter()
                .any(|(_, t)| t == "Timeline" || t == "Palette");
            if strips == 0 || !tab_words {
                bad.push(format!("{preset:?} {w}x{h}: no strips or tab words seen"));
            }
        }
    }
    font::set_face(font::Face::Text);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn tab_strips_take_their_height_from_the_face_in_both_faces() {
    check_strips(false);
    check_strips(true);
}
