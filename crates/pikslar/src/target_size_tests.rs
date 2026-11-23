//! Pointer targets are big enough: every control is at least 24x24 interface units, or sits in a
//! row or column of controls 24 units apart, so a slip of the hand lands on the intended one.
//!
//! The palette swatches are one exemption: they are 20x20 on a 22 unit pitch so that many
//! colours fit, and the palette is a picture to pick from rather than a list of commands. The
//! docks' tab strips are the other: 20 units tall (docs/ui-redesign.md 2.5), their tabs and
//! buttons side by side in a row whose pitch is checked.

use crate::app::App;
use crate::test_app::*;
use pikslar_render::Rect;

const LEAST: i32 = 24;
/// The first and last widget id of the palette swatches.
const SWATCHES: std::ops::Range<u32> = 1_000..2_000;
/// The docks' tab strips.
const STRIPS: std::ops::Range<u32> =
    crate::dock::ID_DOCK..crate::dock::ID_DOCK + 9 * pikslar_ui::tabs::TAB_IDS;

fn centers(r: Rect) -> (i32, i32) {
    (r.x + r.w / 2, r.y + r.h / 2)
}

/// Whether a row (`across`) or column of widgets that overlap `r` in the other direction is
/// spaced at least `LEAST` apart from `r`.
fn pitch_ok(widgets: &[(Rect, u32)], r: Rect, across: bool) -> bool {
    let (cx, cy) = centers(r);
    let mut near = widgets.iter().map(|(o, _)| *o).filter(|&o| {
        let same_line = if across {
            o.y < r.bottom() && r.y < o.bottom()
        } else {
            o.x < r.right() && r.x < o.right()
        };
        same_line && o != r
    });
    let closest = near.clone().map(|o| {
        let (ox, oy) = centers(o);
        if across {
            (ox - cx).abs()
        } else {
            (oy - cy).abs()
        }
    });
    near.next().is_some() && closest.min().is_some_and(|d| d >= LEAST)
}

fn offenders(a: &App) -> Vec<String> {
    // A splitter's gutter is four units with an eight-unit reach (docs/ui-redesign.md 2.5).
    let splitter = |id: u32| {
        a.ui.kind_of(id)
            .is_some_and(|k| k.role == pikslar_ui::role::Role::Splitter)
    };
    let kept: Vec<(Rect, u32)> =
        a.ui.widgets()
            .iter()
            .copied()
            .filter(|&(_, id)| !splitter(id))
            .collect();
    let widgets = &kept[..];
    let mut bad = Vec::new();
    for &(r, id) in widgets {
        if SWATCHES.contains(&id) {
            continue;
        }
        // A strip's own box holds its tabs; only they and its buttons are targets.
        if STRIPS.contains(&id) && (id - STRIPS.start) % pikslar_ui::tabs::TAB_IDS == 0 {
            continue;
        }
        let wide = r.w >= LEAST || pitch_ok(widgets, r, true);
        let strip = STRIPS.contains(&id) && r.h >= pikslar_ui::tabs::strip_h();
        let tall = r.h >= LEAST || strip || pitch_ok(widgets, r, false);
        if !(wide && tall) {
            let name = a.ui.layout().unwrap_or_default();
            let name = name
                .lines()
                .find(|l| l.starts_with(&format!("{id}\t")))
                .map(|l| l.split('\t').nth(5).unwrap_or("").to_string())
                .unwrap_or_default();
            bad.push(format!("{id} {name:?} {}x{}", r.w, r.h));
        }
    }
    bad
}

#[test]
fn every_control_is_24_units_or_sits_in_a_24_unit_pitch_at_the_smallest_size() {
    for (frames, layers) in [(1, 1), (3, 2)] {
        let mut a = app(frames);
        for _ in 1..layers {
            a.layer_action(0);
        }
        a.ui.record_names();
        a.resized(640, 480, 1);
        draw_whole_at(&mut a, 640, 480);
        let bad = offenders(&a);
        assert!(bad.is_empty(), "{frames} frames, {layers} layers: {bad:?}");
    }
}

#[test]
fn the_same_holds_in_a_roomy_window_where_the_sliders_and_every_section_show() {
    let mut a = app(2);
    a.ui.record_names();
    a.resized(1280, 900, 1);
    draw_whole_at(&mut a, 1280, 900);
    let bad = offenders(&a);
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
fn a_section_fold_header_is_as_wide_as_its_header_and_never_a_sliver() {
    let mut a = app(2);
    a.resized(1280, 900, 1);
    draw_whole_at(&mut a, 1280, 900);
    let headers: Vec<Rect> =
        a.ui.widgets()
            .iter()
            .filter(|(_, id)| {
                (crate::panel_right::ID_SECTION..crate::panel_right::ID_SECTION + 4).contains(id)
            })
            .map(|(r, _)| *r)
            .collect();
    // Pixel Lint's header is behind the Layers tab.
    assert_eq!(headers.len(), 3);
    for r in headers {
        assert!(r.w >= LEAST && r.h >= 22, "{r:?}");
    }
}

#[test]
fn a_slider_answers_the_pointer_over_at_least_24_units_though_its_track_is_16() {
    let mut a = app(2);
    a.resized(1280, 900, 1);
    draw_whole_at(&mut a, 1280, 900);
    let sliders: Vec<Rect> =
        a.ui.widgets()
            .iter()
            .filter(|(_, id)| {
                (crate::panel_right::ID_COLOR..crate::panel_right::ID_COLOR + 4).contains(id)
            })
            .map(|(r, _)| *r)
            .collect();
    assert_eq!(sliders.len(), 4);
    assert!(sliders.iter().all(|r| r.h >= LEAST), "{sliders:?}");
}
