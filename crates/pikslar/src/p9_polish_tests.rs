//! The final visual review's polish defects (docs/ui-redesign/final-review.md), each checked
//! through the same entry points the window uses.

use crate::app::App;
use crate::test_app::*;
use pikslar_render::Rect;
use pikslar_ui::font::{self, Face};

const ID_REWIND: u32 = 330;

/// An app drawn whole at `w` by `h` pixels and `scale`, with names kept.
fn named_app(size: (i32, i32, i32), layers: usize) -> App {
    let (w, h, scale) = size;
    let mut a = app(6);
    a.ui.record_names();
    a.resized(w, h, scale);
    for _ in 1..layers {
        a.layer_action(0);
    }
    redraw(&mut a, (w, h));
    a
}

fn redraw(a: &mut App, (w, h): (i32, i32)) {
    draw_whole_at(a, w, h);
    draw_whole_at(a, w, h);
}

fn widget(a: &App, id: u32) -> Option<Rect> {
    a.ui.widgets()
        .iter()
        .find(|&&(_, w)| w == id)
        .map(|&(r, _)| r)
}

/// The first rectangle in the layout dump whose name starts with `prefix`.
fn named(a: &App, prefix: &str) -> Option<Rect> {
    named_by(a, |n| n.starts_with(prefix))
}

/// The first rectangle in the layout dump called exactly `name`.
fn named_exactly(a: &App, name: &str) -> Option<Rect> {
    named_by(a, |n| n == name)
}

fn named_by(a: &App, pick: impl Fn(&str) -> bool) -> Option<Rect> {
    let layout = a.ui.layout()?;
    layout.lines().find_map(|l| {
        let f: Vec<&str> = l.split('\t').collect();
        if !pick(f.get(5)?) {
            return None;
        }
        let n = |i: usize| f.get(i).and_then(|v| v.parse::<i32>().ok());
        Some(Rect::new(n(1)?, n(2)?, n(3)?, n(4)?))
    })
}

fn apart_across(a: Rect, b: Rect) -> bool {
    a.right() <= b.x || b.right() <= a.x
}

// ---- 1: the Rewind bar and the status message never share room

#[test]
fn the_rewind_bar_and_a_status_message_never_overlap() {
    for face in [Face::Text, Face::Mono] {
        for size in [
            (1366, 705, 1),
            (1920, 1003, 2),
            (1200, 700, 1),
            (800, 600, 1),
        ] {
            font::set_face(face);
            let mut a = named_app(size, 3);
            a.notify("New layer. Right click for a tilemap layer".to_string());
            redraw(&mut a, (size.0, size.1));
            let bar = widget(&a, ID_REWIND).expect("the bar is drawn");
            let status = named(&a, "Status ").expect("the message is drawn");
            assert!(
                apart_across(bar, status),
                "{face:?} {size:?}: bar {bar:?} status {status:?}"
            );
            assert!(
                bar.w >= 48 * size.2,
                "{face:?} {size:?}: the bar keeps room"
            );
        }
    }
    font::set_face(Face::Text);
}

/// The bar stays put while what the status says fits beside it (R1 changed the rule: a message
/// that would be cut takes room from the bar, see r1_tests).
#[test]
fn the_rewind_bar_keeps_its_size_while_the_status_fits() {
    let size = (1366, 705, 1);
    let mut a = named_app(size, 1);
    let quiet = widget(&a, ID_REWIND).expect("drawn");
    a.notify("Saved sprite.piks".into());
    redraw(&mut a, (size.0, size.1));
    assert_eq!(widget(&a, ID_REWIND), Some(quiet));
}

/// Draws the whole interface and returns its pixels.
fn pixels(a: &mut App, (w, h): (i32, i32)) -> Vec<u32> {
    let mut px = vec![0u32; (w * h) as usize];
    let mut surface = pikslar_render::Surface { px: &mut px, w, h };
    a.draw_interface(
        &mut surface,
        pikslar_platform::Window::detached(),
        &mut Vec::new(),
    );
    px
}

fn high_contrast(a: &mut App, black: bool) {
    let sys = if black {
        pikslar_ui::theme::SystemColors::BLACK
    } else {
        pikslar_ui::theme::SystemColors::WHITE
    };
    a.forced_look = Some(Some(sys));
    a.follow_look();
}

fn plain_look(a: &mut App) {
    a.forced_look = Some(None);
    a.follow_look();
}

// ---- 4: in high contrast the cel grid keeps its cells

#[test]
fn in_high_contrast_every_cel_has_an_edge_and_the_current_column_shows() {
    use crate::workspace::model::Preset;
    for black in [true, false] {
        let size = (1366, 705, 1);
        let mut a = named_app(size, 3);
        a.choose_preset(Preset::Animate);
        high_contrast(&mut a, black);
        redraw(&mut a, (size.0, size.1));
        // Layer 1's cel at frame 1 is neither the active layer's nor (after this) current.
        a.set_frame(2);
        redraw(&mut a, (size.0, size.1));
        let px_now = pixels(&mut a, (size.0, size.1));
        let at_now = |x: i32, y: i32| px_now[(y * size.0 + x) as usize];
        let cel = named(&a, "Cel 1 1").expect("the cel is drawn");
        let mid_y = cel.y + cel.h / 2;
        let edged = (cel.x..cel.x + 3).any(|x| at_now(x, mid_y) == pikslar_ui::theme::rule());
        assert!(edged, "black {black}: the cel's left edge is a line");
        let current = named(&a, "Cel 1 3").expect("current cel");
        let window = pikslar_ui::theme::panel();
        let inside = (current.x + 3..current.right() - 3)
            .filter(|&x| at_now(x, current.y + 1) != window)
            .count();
        let other = (cel.x + 3..cel.right() - 3)
            .filter(|&x| at_now(x, cel.y + 1) != window)
            .count();
        assert!(
            inside > other,
            "black {black}: the current column is marked"
        );
    }
    let mut a = app(1);
    plain_look(&mut a);
}

// ---- 2: at 2x on a 1080p screen, Draw and Tiles show their palette

fn slot_of(a: &App, panel: crate::workspace::model::Panel) -> crate::workspace::solve::SlotRect {
    let solved = &a.layout.solved;
    let (side, i, _) = solved.effective.find(panel).expect("placed");
    solved.area(side).slots()[i]
}

#[test]
fn draw_and_tiles_at_2x_show_the_palette_and_a_compact_color_wastes_no_room() {
    use crate::workspace::model::{Panel, Preset};
    let size = (1920, 1003, 2);
    for preset in [Preset::Draw, Preset::Tiles] {
        let mut a = named_app(size, 3);
        a.choose_preset(preset);
        redraw(&mut a, (size.0, size.1));
        assert!(slot_of(&a, Panel::Palette).open, "{preset:?}: Palette open");
        let compact = crate::layout::today().color_compact * size.2;
        if a.layout.solved.effective.color_compact && preset == Preset::Draw {
            let color = slot_of(&a, Panel::Color);
            assert!(color.rect.h <= compact, "{preset:?}: {:?}", color.rect);
        }
    }
}

// ---- 5: the Palette workspace gives the canvas its room

#[test]
fn the_palette_workspace_keeps_pixel_lint_on_a_rail_and_the_canvas_its_share() {
    use crate::workspace::model::{Preset, Side};
    for (size, least) in [((1366, 705, 1), 55), ((1920, 1003, 2), 40)] {
        let mut a = named_app(size, 3);
        a.choose_preset(Preset::Palette);
        redraw(&mut a, (size.0, size.1));
        assert!(
            a.layout.solved.area(Side::Left).rail,
            "{size:?}: Pixel Lint on its rail"
        );
        let c = a.layout.canvas;
        let share = (c.w * c.h * 100) / (size.0 * size.1);
        assert!(share >= least, "{size:?}: canvas share {share} %");
    }
}

// ---- 11: a frame's number sits in the middle of its cell, above the cel mark

#[test]
fn a_frame_number_is_centred_above_the_cel_mark() {
    use crate::panel_timeline::number_y;
    for (s, face) in [
        (1, Face::Text),
        (2, Face::Text),
        (1, Face::Mono),
        (2, Face::Mono),
    ] {
        font::set_face(face);
        let ts = pikslar_ui::font::text_scale(s);
        let r = Rect::new(0, 100, 32 * s, 28 * s);
        let top = number_y(r, s, ts) - r.y;
        let below = r.h - 6 * s - (top + pikslar_ui::cap_height() * ts);
        assert!(top >= s && below >= 0, "{s} {face:?}: {top} {below}");
        assert!(
            (top - below).abs() <= s,
            "{s} {face:?}: {top} above, {below} below"
        );
    }
    font::set_face(Face::Text);
}

// ---- 14: the Brush button says it opens a menu

#[test]
fn the_brush_button_carries_a_menu_caret_right_of_its_picture() {
    let size = (1366, 705, 1);
    let mut a = named_app(size, 1);
    redraw(&mut a, (size.0, size.1));
    let r = named_exactly(&a, "Brush").expect("the Brush button");
    let px = pixels(&mut a, (size.0, size.1));
    let ink = pikslar_ui::theme::text();
    let right = (r.right() - 12..r.right() - 2)
        .filter(|&x| px[((r.y + r.h / 2) * size.0 + x) as usize] == ink)
        .count();
    assert!(right >= 3, "a caret at the right: {right}");
}

// ---- 13: a menu opens under the bar, clear of its title's focus ring

#[test]
fn a_menu_bar_menu_opens_under_the_bar() {
    for size in [(1366, 705, 1), (1920, 1003, 2)] {
        let mut a = named_app(size, 1);
        a.open_bar_menu(1, true);
        redraw(&mut a, (size.0, size.1));
        let at = a.menu.map(|m| m.at).expect("the Edit menu is open");
        assert!(at.1 >= a.layout.top.bottom(), "{size:?}: {at:?}");
    }
}

// ---- 12: the Export dialog's steppers are the icon steppers used everywhere else

#[test]
fn the_export_scale_stepper_uses_icons_and_is_off_at_its_limit() {
    use crate::export_panel::ID_EXPORT;
    let size = (1366, 705, 1);
    let mut a = named_app(size, 1);
    a.open_export_dialog(true);
    redraw(&mut a, (size.0, size.1));
    let less = a.ui.kind_of(ID_EXPORT + 22).expect("Scale's less button");
    let more = a.ui.kind_of(ID_EXPORT + 23).expect("Scale's more button");
    assert!(less.state.disabled, "scale 1 cannot go lower");
    assert!(!more.state.disabled);
    let names = a.ui.layout().expect("names");
    assert!(
        !names.lines().any(|l| l.split('\t').nth(5) == Some("-")),
        "no text minus"
    );
}

// ---- the workspace workflow: an arrangement changed by a key is written by that same frame

#[test]
fn a_timeline_resized_by_a_key_is_saved_without_waiting_for_another_frame() {
    use crate::settings::SettingsFile;
    use crate::workspace::model::Side;
    use pikslar_platform::{Handler, Key, Mods, Window};
    let dir = std::env::temp_dir().join(format!("pikslar-p9-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let (w, h) = (1400, 900);
    let mut a = app(1);
    a.use_settings(SettingsFile::in_dir(&dir));
    a.resized(w, h, 1);
    redraw(&mut a, (w, h));
    a.focus_on(crate::dock_keys::ID_SPLIT + Side::Bottom.index() as u32);
    redraw(&mut a, (w, h));
    for _ in 0..2 {
        a.key_down(Key::Up, Mods::default(), false, Window::detached());
        // One window frame, as the window runs it after the key, and no other.
        let mut px = vec![0u32; (w * h) as usize];
        let mut surface = pikslar_render::Surface { px: &mut px, w, h };
        a.frame(&mut surface, Window::detached(), &mut Vec::new());
    }
    let stored = a.workspaces.active().area(Side::Bottom).size;
    let mut b = app(1);
    b.use_settings(SettingsFile::in_dir(&dir));
    assert_eq!(b.workspaces.active().area(Side::Bottom).size, stored);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- a splitter dragged by the pointer writes settings once, on release, not on every move

/// One window frame, as the window runs it after each event.
fn window_frame(a: &mut App, (w, h): (i32, i32)) {
    use pikslar_platform::{Handler, Window};
    let mut px = vec![0u32; (w * h) as usize];
    let mut surface = pikslar_render::Surface { px: &mut px, w, h };
    a.frame(&mut surface, Window::detached(), &mut Vec::new());
}

#[test]
fn a_splitter_drag_writes_settings_on_release_and_not_on_every_move() {
    use crate::settings::SettingsFile;
    use crate::workspace::model::Side;
    use pikslar_platform::{Button, Event, Handler, Mods, Window};
    let dir = std::env::temp_dir().join(format!("pikslar-p9-drag-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("settings.txt");
    let size = (1400, 900);
    let mut a = app(1);
    a.use_settings(SettingsFile::in_dir(&dir));
    a.resized(size.0, size.1, 1);
    redraw(&mut a, size);
    window_frame(&mut a, size);
    let before = a.workspaces.active().area(Side::Bottom).size;
    let (x, y) =
        crate::test_app::centre_of(&a, crate::dock_keys::ID_SPLIT + Side::Bottom.index() as u32);
    let _ = std::fs::remove_file(&file);
    let press = |a: &mut App, button, y| {
        let mods = Mods::default();
        a.event(Event::Down { x, y, button, mods }, Window::detached());
    };
    press(&mut a, Button::Left, y);
    window_frame(&mut a, size);
    for step in 1..=12 {
        a.event(Event::Move { x, y: y - step * 4 }, Window::detached());
        window_frame(&mut a, size);
        assert!(!file.exists(), "move {step} wrote the settings while held");
    }
    let (button, y) = (Button::Left, y - 48);
    a.event(Event::Up { x, y, button }, Window::detached());
    window_frame(&mut a, size);
    let stored = a.workspaces.active().area(Side::Bottom).size;
    assert_ne!(stored, before, "the drag resized the timeline");
    assert!(file.exists(), "the release frame writes the settings");
    let mut b = app(1);
    b.use_settings(SettingsFile::in_dir(&dir));
    assert_eq!(b.workspaces.active().area(Side::Bottom).size, stored);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- the Rewind row keeps its bar, counter and status inside its area, however narrow

#[test]
fn the_rewind_row_keeps_every_part_inside_a_narrow_area() {
    use crate::panel_bottom::RewindRow;
    for w in [0, 20, 60, 120, 200, 400] {
        for s in [1, 2] {
            let area = Rect::new(10, 5, w * s, 16 * s);
            let row = RewindRow::of(area, s, s);
            for (what, r) in [
                ("bar", row.track),
                ("counter", row.counter),
                ("status", row.status),
            ] {
                assert!(
                    r.x >= area.x && r.right() <= area.right() && r.w >= 0,
                    "{what} {r:?} leaves {area:?}"
                );
            }
        }
    }
}
