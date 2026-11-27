//! The docks as drawn: slots that fold by hand and for room, their tab strips and rails.
//! These replace the tests of the right panel's own folding, which the slots took over.

use super::*;
use crate::panel_right::{Section, ID_SECTION};
use crate::test_app::*;
use pikslar_platform::Button;

fn widget(a: &App, id: u32) -> Option<Rect> {
    a.ui.widgets()
        .iter()
        .find(|(_, w)| *w == id)
        .map(|(r, _)| *r)
}

fn at(w: i32, h: i32) -> App {
    let mut a = app(1);
    a.ui.record_names();
    a.resized(w, h, 1);
    draw_whole_at(&mut a, w, h);
    a
}

/// The right dock's slots as shown: their heights and whether each is open.
fn right(a: &App) -> Vec<(i32, bool)> {
    let area = a.layout.solved.area(Side::Right);
    area.slots().iter().map(|r| (r.rect.h, r.open)).collect()
}

fn click(a: &mut App, id: u32, (w, h): (i32, i32)) {
    let r = widget(a, id).unwrap_or_else(|| panic!("{id} is shown"));
    let (x, y) = (r.x + r.w / 2, r.y + r.h / 2);
    a.ui.mouse = (x, y);
    a.pointer_down(x, y, Button::Left, PLAIN);
    draw_whole_at(a, w, h);
    a.pointer_up(x, y, Button::Left);
    draw_whole_at(a, w, h);
    draw_whole_at(a, w, h);
}

#[test]
fn draw_shows_color_palette_and_layers_under_tab_strips() {
    let a = at(1366, 705);
    let slots = right(&a);
    assert_eq!(slots.len(), 3);
    assert!(slots.iter().all(|&(_, open)| open), "{slots:?}");
    for (i, name) in [(0, "Color"), (1, "Palette"), (2, "Layers")] {
        let names = a.ui.layout().unwrap_or_default();
        let tab = strip_id(Side::Right, i) + 1;
        assert!(widget(&a, tab).is_some(), "{name} tab");
        assert!(
            names.contains(&format!("\t{name} tab\ttab\tchosen\n")),
            "{names}"
        );
    }
    for id in [ID_SECTION, ID_SECTION + 1, ID_SECTION + 2] {
        assert!(widget(&a, id).is_some(), "{id}");
    }
    assert!(
        widget(&a, ID_SECTION + 3).is_none(),
        "Pixel Lint is a tab behind Layers"
    );
}

#[test]
fn folded_slots_give_their_room_to_the_rest() {
    let mut a = at(1366, 705);
    let open = right(&a);
    a.fold_section(Section::Palette, true);
    draw_whole_at(&mut a, 1366, 705);
    let folded = right(&a);
    assert!(!folded[1].1 && folded[1].0 < open[1].0);
    assert!(
        folded[2].0 > open[2].0,
        "the layers take the room: {open:?} {folded:?}"
    );
    assert!(!a.workspaces.active().area(Side::Right).slots()[1].open);
    a.fold_section(Section::Palette, false);
    draw_whole_at(&mut a, 1366, 705);
    assert_eq!(right(&a), open);
}

#[test]
fn a_short_dock_compacts_the_color_before_it_folds_the_palette_and_keeps_both() {
    let (mut compact_seen, mut folded_seen) = (false, false);
    for h in (560..=1100).step_by(10) {
        let a = at(1366, h);
        let slots = right(&a);
        let compact = a.layout.solved.effective.color_compact;
        assert!(
            slots[0].1 && slots[2].1,
            "{h}: color and layers stay open: {slots:?}"
        );
        // Compacting comes before hiding (R1, A11): the palette folds only once the color is
        // compact, and the color compacts as soon as the lists would lose a row beside it.
        assert!(
            compact || slots[1].1,
            "{h}: the palette folds only after the color compacts"
        );
        compact_seen |= compact && slots[1].1;
        folded_seen |= !slots[1].1;
        let used: i32 = slots.iter().map(|s| s.0).sum();
        assert_eq!(used, a.layout.right.h, "{h}");
    }
    assert!(
        compact_seen,
        "some height compacts the color and keeps the palette open"
    );
    assert!(folded_seen, "some height still has to fold the palette");
}

#[test]
fn a_roomy_dock_gives_everything_its_full_size() {
    let a = at(1920, 1017);
    let slots = right(&a);
    assert!(slots.iter().all(|s| s.1));
    assert!(!a.layout.solved.effective.color_compact);
    let body = slots[0].0 - crate::layout::strip() - 2 * BODY_Y;
    assert!(body >= crate::panel_fit::FULL_COLOR, "{slots:?}");
}

#[test]
fn two_layer_rows_always_fit_at_the_smallest_window() {
    let mut a = app(1);
    for _ in 0..30 {
        a.layer_action(0);
    }
    a.resized(624, 561, 1);
    draw_whole_at(&mut a, 624, 561);
    let rows =
        a.ui.widgets()
            .iter()
            .filter(|(_, id)| (80_000..81_000).contains(id))
            .count();
    assert!(rows >= 2, "{rows}");
}

#[test]
fn a_tab_shows_its_panel_and_the_chevron_folds_the_slot() {
    let size = (1366, 705);
    let mut a = at(size.0, size.1);
    let lint_tab = strip_id(Side::Right, 2) + 3;
    click(&mut a, lint_tab, size);
    assert!(widget(&a, ID_SECTION + 3).is_some(), "Pixel Lint shown");
    assert!(widget(&a, ID_SECTION + 2).is_none(), "Layers behind it");
    assert_eq!(a.workspaces.active().area(Side::Right).slots()[2].active, 2);
    let chevron = strip_id(Side::Right, 0) + 18;
    click(&mut a, chevron, size);
    assert!(!right(&a)[0].1, "Color folded");
    assert!(widget(&a, crate::panel_right::ID_COLOR).is_none());
    click(&mut a, chevron, size);
    assert!(right(&a)[0].1, "Color open again");
}

#[test]
fn a_slot_folded_for_room_ignores_its_chevron() {
    let size = (1366, 560);
    let mut a = at(size.0, size.1);
    assert!(!right(&a)[1].1, "the palette folds for room at 560");
    let chevron = strip_id(Side::Right, 1) + 18;
    click(&mut a, chevron, size);
    assert!(a.workspaces.active().area(Side::Right).slots()[1].open);
}

#[test]
fn a_folded_area_is_a_rail_that_opens_on_a_panel() {
    let size = (1366, 705);
    let mut a = at(size.0, size.1);
    a.edit_layout(|ws| {
        let area = ws.area(Side::Right).with_open(false);
        ws.with_area(Side::Right, area)
    });
    draw_whole_at(&mut a, size.0, size.1);
    assert!(a.layout.solved.area(Side::Right).rail);
    assert_eq!(a.layout.right.w, crate::workspace::model::RAIL);
    let names = a.ui.layout().unwrap_or_default();
    assert!(
        names.contains("\tShow Palette\tbutton\tcollapsed\n"),
        "{names}"
    );
    let palette = ID_RAIL + 2;
    click(&mut a, palette, size);
    assert!(!a.layout.solved.area(Side::Right).rail);
    assert!(widget(&a, ID_SECTION + 1).is_some());
}

#[test]
fn a_tilemap_layer_shows_tiles_beside_the_palette_until_another_tab_is_chosen() {
    use pikslar_core::{ColorMode, TileCmd};
    let size = (1366, 705);
    let mut a = at(size.0, size.1);
    let at_top = a.editor.doc().layers().len();
    a.editor
        .apply(TileCmd::NewMapLayer {
            at: at_top,
            name: "Map".into(),
            mode: ColorMode::Rgba,
            tile_w: 16,
            tile_h: 16,
        })
        .expect("room");
    a.editor.layer = at_top;
    a.sync_matrix_layout();
    draw_whole_at(&mut a, size.0, size.1);
    let names = a.ui.layout().unwrap_or_default();
    assert!(names.contains("\tTiles tab\ttab\tchosen\n"), "{names}");
    assert!(widget(&a, crate::panel_tiles::ID_TILES).is_some());
    let palette_tab = strip_id(Side::Right, 1) + 1;
    click(&mut a, palette_tab, size);
    assert!(widget(&a, ID_SECTION + 1).is_some(), "the palette again");
    assert!(
        !a.workspaces.active().placed().contains(Panel::Tiles),
        "never saved"
    );
}

/// Large text crowds the Layers slot's strip: Layers stays whole, Slices and Pixel Lint become
/// icons, and the icon-only tab says its name for a keyboard user too, not only for the pointer.
#[test]
fn a_keyboard_focused_icon_tab_says_its_name() {
    let size = (1366, 705);
    let mut a = at(size.0, size.1);
    a.set_large_text(true);
    draw_whole_at(&mut a, size.0, size.1);
    let strip = strip_id(Side::Right, 2);
    let slices = strip + 2;
    let rect = widget(&a, slices).expect("the Slices tab is shown");
    assert_eq!(rect.w, 24, "an icon tab: 24 units");
    assert_eq!(
        a.hint.as_deref(),
        None,
        "nothing under the pointer or focus yet"
    );
    a.ui.focus.set(slices);
    draw_whole_at(&mut a, size.0, size.1);
    assert_eq!(a.hint.as_deref(), Some("Slices"));
    a.ui.focus.set(strip + 1);
    draw_whole_at(&mut a, size.0, size.1);
    assert_eq!(a.hint.as_deref(), None, "a whole-name tab needs no hint");
    pikslar_ui::font::set_face(pikslar_ui::font::Face::Text);
}

#[test]
fn a_folded_palette_is_its_tab_strip_and_the_open_slots_take_the_room() {
    let mut a = at(1366, 705);
    let open = right(&a);
    assert!(open.iter().all(|s| s.1), "Draw at 1x shows every slot");
    a.fold_section(Section::Palette, true);
    draw_whole_at(&mut a, 1366, 705);
    let folded = right(&a);
    let strip = crate::layout::strip();
    assert_eq!(folded[1], (strip, false), "the strip alone, no header");
    assert!(
        widget(&a, ID_SECTION + 1).is_none(),
        "no header repeats the tab"
    );
    let total = |v: &[(i32, bool)]| v.iter().map(|s| s.0).sum::<i32>();
    assert_eq!(total(&folded), total(&open), "the dock is still full");
    assert!(
        folded[2].0 > open[2].0,
        "Layers took what the Palette gave up: {open:?} then {folded:?}"
    );
}

#[test]
fn a_slot_folded_for_room_says_so_on_its_fold_button_and_the_palette_lets_go() {
    let (mut a, i, h) = (500..900)
        .step_by(20)
        .find_map(|h| {
            let a = at(1280, h);
            let slots = a.layout.solved.area(Side::Right);
            let i = slots
                .slots()
                .iter()
                .position(|s| s.panel == Some(Panel::Palette) && !s.open)?;
            Some((a, i, h))
        })
        .expect("some height folds the Palette");
    let id = strip_id(Side::Right, i) + pikslar_ui::tabs::FOLD;
    let r = widget(&a, id).expect("the fold button");
    a.ui.mouse = (r.x + r.w / 2, r.y + r.h / 2);
    draw_whole_at(&mut a, 1280, h);
    assert_eq!(
        a.hint.as_deref(),
        Some("Folded for room. A taller window shows it")
    );
    assert_eq!(a.selected_swatch, None);
}
