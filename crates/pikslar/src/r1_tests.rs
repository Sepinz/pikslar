//! Package R1's refinements (docs/ui-refinement/issue-list.md), each checked through the same
//! entry points the window uses, at 1x and 2x, in both faces and, where it matters, in high
//! contrast.

use crate::app::App;
use crate::test_app::*;
use pikslar_render::Rect;
use pikslar_ui::font::{self, text_width, Face};

pub(crate) const ID_REWIND: u32 = 330;

/// The sizes R1 is judged at: 1366 by 705 at 1x, a 1080p screen at 150 % (2x), a small window.
pub(crate) const SIZES: [(i32, i32, i32); 3] = [(1366, 705, 1), (1920, 1003, 2), (800, 600, 1)];

/// An app drawn whole at `size`, with names kept, `layers` layers and six frames.
pub(crate) fn named_app(size: (i32, i32, i32), layers: usize) -> App {
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

pub(crate) fn redraw(a: &mut App, (w, h): (i32, i32)) {
    draw_whole_at(a, w, h);
    draw_whole_at(a, w, h);
}

pub(crate) fn widget(a: &App, id: u32) -> Option<Rect> {
    a.ui.widgets()
        .iter()
        .find(|&&(_, w)| w == id)
        .map(|&(r, _)| r)
}

/// Every named rectangle in the layout dump, with its name.
pub(crate) fn names(a: &App) -> Vec<(String, Rect)> {
    let Some(layout) = a.ui.layout() else {
        return Vec::new();
    };
    layout
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let n = |i: usize| f.get(i).and_then(|v| v.parse::<i32>().ok());
            Some((
                (*f.get(5)?).to_string(),
                Rect::new(n(1)?, n(2)?, n(3)?, n(4)?),
            ))
        })
        .collect()
}

pub(crate) fn named(a: &App, prefix: &str) -> Option<Rect> {
    names(a)
        .into_iter()
        .find(|(n, _)| n.starts_with(prefix))
        .map(|(_, r)| r)
}

/// Runs `f` in each face, and leaves the plain face on, also when `f` fails.
pub(crate) fn in_both_faces(mut f: impl FnMut(Face)) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            font::set_face(Face::Text);
        }
    }
    let _reset = Reset;
    for face in [Face::Text, Face::Mono] {
        font::set_face(face);
        f(face);
    }
}

// ---- A1: a status message is not cut while the Rewind bar has room to give

const HINT: &str = "New layer. Right click for a tilemap layer";

#[test]
fn a_status_message_takes_room_from_the_rewind_bar_before_it_is_cut() {
    in_both_faces(|face| {
        for size in [(1366, 705, 1), (1920, 1003, 2)] {
            let mut a = named_app(size, 3);
            a.notify(HINT.to_string());
            redraw(&mut a, (size.0, size.1));
            let status = named(&a, "Status ").expect("the message is drawn");
            let whole = text_width(HINT, a.ui.text_scale());
            assert_eq!(
                status.w, whole,
                "{face:?} {size:?}: the message is cut ({} of {whole})",
                status.w
            );
            let bar = widget(&a, ID_REWIND).expect("the bar is drawn");
            assert!(bar.w >= 48 * size.2, "{face:?} {size:?}: bar {bar:?}");
            assert!(
                bar.right() <= status.x,
                "{face:?} {size:?}: bar {bar:?} meets status {status:?}"
            );
        }
    });
}

#[test]
fn a_message_too_long_for_any_room_is_still_cut_and_the_bar_keeps_its_least_length() {
    in_both_faces(|face| {
        let size = (800, 600, 1);
        let mut a = named_app(size, 1);
        let long = "Saved C:/a/very/long/path/to/some/sprite/that/goes/on/and/on/and/on.piks";
        a.notify(long.to_string());
        redraw(&mut a, (size.0, size.1));
        let bar = widget(&a, ID_REWIND).expect("the bar is drawn");
        assert!(bar.w >= 48, "{face:?}: bar {bar:?}");
        let status = named(&a, "Status ").expect("drawn");
        assert!(status.right() <= size.0, "{face:?}: {status:?}");
    });
}

#[test]
fn the_rewind_row_needing_room_keeps_every_part_inside_its_area() {
    use crate::panel_bottom::RewindRow;
    for w in [0, 20, 60, 120, 200, 400, 900] {
        for s in [1, 2] {
            for need in [0, 50, 300, 5000] {
                let area = Rect::new(10, 5, w * s, 16 * s);
                let row = RewindRow::needing(area, s, s, need * s, 40 * s);
                for (what, r) in [
                    ("bar", row.track),
                    ("counter", row.counter),
                    ("status", row.status),
                ] {
                    assert!(
                        r.x >= area.x && r.right() <= area.right() && r.w >= 0,
                        "w {w} s {s} need {need}: {what} {r:?} leaves {area:?}"
                    );
                }
                assert!(row.track.right() <= row.counter.x);
                assert!(row.counter.right() <= row.status.x);
            }
        }
    }
}

// ---- A2: the Color section says which color it shows, also in Large text at 2x

#[test]
fn the_color_section_names_the_foreground_in_every_face_and_size() {
    in_both_faces(|face| {
        for size in SIZES {
            let a = named_app(size, 3);
            let label = named(&a, "Color label ")
                .unwrap_or_else(|| panic!("{face:?} {size:?}: no Foreground label"));
            assert!(label.w > 0 && label.right() <= size.0, "{face:?} {size:?}");
        }
    });
}

// ---- A3: a section title is shown wherever its header has room, and named where it has not

const SECTIONS: [&str; 3] = ["Layers", "Palette", "Pixel Lint"];

#[test]
fn a_section_title_is_shown_wherever_it_fits_and_never_leaves_its_header() {
    use crate::workspace::model::Preset;
    in_both_faces(|face| {
        for size in SIZES {
            for preset in [
                Preset::Draw,
                Preset::Animate,
                Preset::Tiles,
                Preset::Palette,
            ] {
                let mut a = named_app(size, 3);
                a.choose_preset(preset);
                redraw(&mut a, (size.0, size.1));
                let ts = a.ui.text_scale();
                let all = names(&a);
                for (name, header) in all.iter().filter(|(n, _)| SECTIONS.contains(&n.as_str())) {
                    let room = header.right() - pikslar_ui::fold::title_x(header.x, size.2, ts);
                    let title = all.iter().find(|(n, _)| *n == format!("Title {name}"));
                    let at = format!("{face:?} {size:?} {preset:?} {name}");
                    if text_width(name, ts) <= room {
                        let (_, r) = title.unwrap_or_else(|| panic!("{at}: room {room}, no title"));
                        assert!(r.right() <= header.right(), "{at}: {r:?} leaves {header:?}");
                    } else {
                        assert!(title.is_none(), "{at}: a title with no room");
                    }
                    let hint = all
                        .iter()
                        .any(|(n, r)| r == header && n.contains(name.as_str()));
                    assert!(hint, "{at}: the header's hint does not name the section");
                }
            }
        }
    });
}

#[test]
fn the_palette_title_shows_in_large_text_where_it_was_three_units_short() {
    use crate::workspace::model::Preset;
    in_both_faces(|face| {
        for size in [(1366, 705, 1), (1920, 1003, 2)] {
            let mut a = named_app(size, 3);
            a.choose_preset(Preset::Animate);
            redraw(&mut a, (size.0, size.1));
            assert!(named(&a, "Title Palette").is_some(), "{face:?} {size:?}");
        }
    });
}

/// A left click at `at`, drawn whole at `size` around it.
pub(crate) fn click_at(a: &mut App, size: (i32, i32, i32), at: (i32, i32)) {
    use pikslar_platform::{Button, Mods};
    a.pointer_down(at.0, at.1, Button::Left, Mods::default());
    draw_whole_at(a, size.0, size.1);
    a.pointer_up(at.0, at.1, Button::Left);
    redraw(a, (size.0, size.1));
}

// ---- A4: the empty Tiles panel offers the way to a tileset

#[test]
fn the_empty_tiles_panel_has_a_button_that_opens_the_new_tilemap_menu() {
    use crate::frame_menu::MenuKind;
    use crate::panel_tiles::ID_ADD_TILEMAP;
    in_both_faces(|face| {
        for size in SIZES {
            let mut a = named_app(size, 1);
            a.choose_preset(crate::workspace::model::Preset::Tiles);
            redraw(&mut a, (size.0, size.1));
            assert!(a.active_tileset().is_none());
            let button = widget(&a, ID_ADD_TILEMAP)
                .unwrap_or_else(|| panic!("{face:?} {size:?}: no Add tilemap button"));
            assert!(button.right() <= size.0 && button.bottom() <= size.1);
            click_at(
                &mut a,
                size,
                (button.x + button.w / 2, button.y + button.h / 2),
            );
            assert_eq!(
                a.menu.map(|m| m.kind),
                Some(MenuKind::NewTilemap),
                "{face:?} {size:?}"
            );
        }
    });
}

// ---- A5, A6, A7, C-2: icon buttons say what they do, to the pointer and to the keyboard

/// The pointer moves to `at`, and the interface is drawn whole around it.
pub(crate) fn hover(a: &mut App, size: (i32, i32, i32), at: (i32, i32)) {
    use pikslar_platform::{Event, Handler, Window};
    a.event(Event::Move { x: at.0, y: at.1 }, Window::detached());
    redraw(a, (size.0, size.1));
}

fn centre(r: Rect) -> (i32, i32) {
    (r.x + r.w / 2, r.y + r.h / 2)
}

fn hint(a: &App) -> String {
    a.hint.as_deref().unwrap_or_default().to_string()
}

#[test]
fn the_dock_strip_buttons_say_panel_menu_and_fold_this_panel() {
    in_both_faces(|face| {
        for size in SIZES {
            let mut a = named_app(size, 3);
            let all = names(&a);
            let menu = all
                .iter()
                .find(|(n, _)| n.ends_with(" menu") && n != "Scripts menu");
            let (_, menu) = menu.unwrap_or_else(|| panic!("{face:?} {size:?}: no panel menu"));
            let fold = all
                .iter()
                .find(|(n, _)| n.starts_with("Fold ") && !n.contains("section"));
            let (_, fold) = fold.unwrap_or_else(|| panic!("{face:?} {size:?}: no fold button"));
            let (menu, fold) = (*menu, *fold);
            hover(&mut a, size, centre(menu));
            assert_eq!(hint(&a), "Panel menu", "{face:?} {size:?}");
            hover(&mut a, size, centre(fold));
            assert_eq!(hint(&a), "Fold this panel", "{face:?} {size:?}");
        }
    });
}

#[test]
fn the_export_scale_buttons_say_smaller_and_larger_scale() {
    in_both_faces(|face| {
        for size in SIZES {
            let mut a = named_app(size, 1);
            a.open_export_dialog(false);
            redraw(&mut a, (size.0, size.1));
            for (name, said) in [
                ("Less Scale", "Smaller scale"),
                ("More Scale", "Larger scale"),
            ] {
                let r = named(&a, name).unwrap_or_else(|| panic!("{face:?} {size:?}: no {name}"));
                hover(&mut a, size, centre(r));
                assert_eq!(hint(&a), said, "{face:?} {size:?}");
            }
        }
    });
}

#[test]
fn a_focused_icon_button_says_its_hint_without_a_tooltip_at_the_pointer() {
    use crate::panel_right::ID_LAYER_TOOLS;
    for size in SIZES {
        let mut a = named_app(size, 3);
        hover(&mut a, size, (2, 2));
        assert_eq!(hint(&a), "", "{size:?}: nothing under the pointer");
        let id = ID_LAYER_TOOLS + 1;
        let r = widget(&a, id).expect("the second layer tool");
        a.ui.focus.set(id);
        redraw(&mut a, (size.0, size.1));
        let said = names(&a)
            .into_iter()
            .find(|(n, at)| *at == r && n != "New layer")
            .map(|(n, _)| n)
            .expect("the tool is named by its hint");
        assert_eq!(hint(&a), said, "{size:?}");
        assert!(a.hint_from_focus, "{size:?}");
        a.rest_moved(
            2,
            2,
            std::time::Instant::now() - 2 * crate::tooltip::TOOLTIP_DELAY,
        );
        a.tick_tooltip(std::time::Instant::now());
        assert!(
            a.tooltip_area().is_none(),
            "{size:?}: no tooltip at the pointer"
        );
        // The pointer on another control speaks for that one.
        let eye = named(&a, "Show or hide this layer").expect("an eye");
        hover(&mut a, size, centre(eye));
        assert!(hint(&a).starts_with("Hide "), "{size:?}: {}", hint(&a));
        assert!(!a.hint_from_focus);
    }
}

#[test]
fn the_eye_names_the_layer_it_shows_or_hides() {
    let size = (1366, 705, 1);
    let mut a = named_app(size, 2);
    let eyes: Vec<Rect> = names(&a)
        .into_iter()
        .filter(|(n, _)| n == "Show or hide this layer")
        .map(|(_, r)| r)
        .collect();
    assert_eq!(eyes.len(), 2);
    let layers: Vec<String> = a
        .editor
        .doc()
        .layers()
        .iter()
        .map(|l| l.name.clone())
        .collect();
    let mut said = Vec::new();
    for &eye in &eyes {
        hover(&mut a, size, centre(eye));
        said.push(hint(&a));
    }
    for name in &layers {
        assert!(
            said.contains(&format!("Hide {name}")),
            "{said:?} {layers:?}"
        );
    }
    // Hidden, the same eye offers to show it.
    click_at(&mut a, size, centre(eyes[0]));
    hover(&mut a, size, centre(eyes[0]));
    let hidden = a
        .editor
        .doc()
        .layers()
        .iter()
        .find(|l| !l.visible)
        .expect("one hidden");
    assert_eq!(hint(&a), format!("Show {}", hidden.name));
}

// ---- A10: frame numbers of one to four digits stay inside their chips, in both faces

/// What `a` draws at `size`, logged: the texts and where each went.
fn drawn_texts(a: &mut App, size: (i32, i32, i32)) -> Vec<(Rect, String)> {
    font::log_text(true);
    redraw(a, (size.0, size.1));
    let log = font::take_text_log();
    font::log_text(false);
    log.texts
}

/// The frame numbers that leave their chip, or that meet another number.
fn frame_number_faults(a: &mut App, size: (i32, i32, i32)) -> Vec<String> {
    let s = size.2;
    let texts = drawn_texts(a, size);
    let cells: Vec<(String, Rect)> = names(a)
        .into_iter()
        .filter(|(n, _)| n.starts_with("Frame ") && n[6..].parse::<usize>().is_ok())
        .collect();
    assert!(!cells.is_empty(), "{size:?}: no frame chips drawn");
    let mut faults = Vec::new();
    let mut shown = Vec::new();
    for (name, chip) in &cells {
        let number = &name[6..];
        let found = texts
            .iter()
            .find(|(t, w)| w == number && chip.contains(t.x + t.w / 2, t.y + t.h / 2));
        let Some(&(t, _)) = found else {
            faults.push(format!("{name}: no number drawn in {chip:?}"));
            continue;
        };
        if t.x < chip.x + s || t.right() > chip.right() - s {
            faults.push(format!("{name}: {t:?} leaves {chip:?}"));
        }
        shown.push((name.clone(), t));
    }
    for (i, (a_name, a_r)) in shown.iter().enumerate() {
        for (b_name, b_r) in &shown[i + 1..] {
            let meet = a_r.x < b_r.right()
                && b_r.x < a_r.right()
                && a_r.y < b_r.bottom()
                && b_r.y < a_r.bottom();
            if meet {
                faults.push(format!("{a_name} meets {b_name}"));
            }
        }
    }
    faults
}

#[test]
fn frame_numbers_of_one_to_four_digits_fit_their_chips() {
    use crate::workspace::model::Preset;
    let mut faults = Vec::new();
    in_both_faces(|face| {
        for size in [(1366, 705, 1), (1920, 1003, 2)] {
            for matrix in [false, true] {
                let mut a = app(1200);
                a.ui.record_names();
                a.resized(size.0, size.1, size.2);
                a.choose_preset(Preset::Animate);
                if matrix != a.matrix.open {
                    a.toggle_matrix();
                }
                for at in [0, 95, 995, 1190] {
                    a.set_frame(at);
                    for f in frame_number_faults(&mut a, size) {
                        faults.push(format!("{face:?} {size:?} matrix {matrix} at {at}: {f}"));
                    }
                }
            }
        }
    });
    faults.dedup();
    assert!(
        faults.is_empty(),
        "{} faults:\n{}",
        faults.len(),
        faults.join("\n")
    );
}

// ---- A9: Large text spacing

#[test]
fn context_bar_controls_are_two_units_taller_in_large_text_and_tall_enough_for_the_words() {
    use crate::context_bar::{control, CONTROL};
    use pikslar_ui::font::{self, Face};
    in_both_faces(|face| {
        let want = match face {
            Face::Text => CONTROL,
            Face::Mono => CONTROL + 2,
        };
        assert_eq!(control(), want, "{face:?}");
        // The capitals, with three units above and below, fit inside the control at either size.
        for s in [1, 2] {
            let ink = font::cap_height() * font::text_scale(s);
            assert!(ink + 6 * s <= control() * s, "{face:?} {s}x: {ink}");
        }
    });
}

// ---- A11: at 2x the canvas keeps its share, lists are drawn in whole rows, Color compacts first

/// The canvas's share of the window, and the right dock's slots, after a full draw.
fn canvas_share(a: &App, (w, h): (i32, i32)) -> f64 {
    let c = a.layout.canvas;
    f64::from(c.w * c.h) / f64::from(w * h)
}

#[test]
fn the_canvas_keeps_two_fifths_of_the_window_at_1x_and_2x_in_both_faces() {
    use crate::workspace::model::Preset;
    let mut seen = Vec::new();
    in_both_faces(|face| {
        for size in [(1366, 705, 1), (1920, 1003, 2), (2560, 1377, 2)] {
            for p in [Preset::Draw, Preset::Tiles, Preset::Animate] {
                let mut a = named_app(size, 3);
                a.choose_preset(p);
                redraw(&mut a, (size.0, size.1));
                let share = canvas_share(&a, (size.0, size.1));
                seen.push(format!("{face:?} {size:?} {p:?} {:.3}", share));
                assert!(share >= 0.40, "{face:?} {size:?} {p:?}: canvas {share:.3}");
            }
        }
    });
    eprintln!("canvas shares:\n{}", seen.join("\n"));
}

#[test]
fn a_sized_list_slot_is_drawn_in_whole_rows_and_its_rest_goes_to_the_taker() {
    use crate::workspace::model::{Panel, Preset, Side};
    let m = crate::layout::today();
    in_both_faces(|face| {
        for size in [(1366, 705, 1), (1920, 1003, 2), (1920, 1017, 1)] {
            for p in [Preset::Draw, Preset::Tiles, Preset::Animate] {
                let mut a = named_app(size, 3);
                a.choose_preset(p);
                redraw(&mut a, (size.0, size.1));
                let s = size.2;
                let solved = &a.layout.solved;
                let model = solved.effective.area(Side::Right).slots().to_vec();
                let drawn = solved.area(Side::Right).slots().to_vec();
                let used: i32 = drawn.iter().map(|d| d.rect.h).sum();
                assert_eq!(used, solved.area(Side::Right).rect.h, "{size:?} {p:?}");
                for (sl, d) in model.iter().zip(&drawn) {
                    let Some(panel) = d.panel.filter(|_| d.open && sl.size > 0) else {
                        continue;
                    };
                    let row = (m.row)(panel) * s;
                    if row == 0 || !matches!(panel, Panel::Palette | Panel::Layers) {
                        continue;
                    }
                    let least = (m.least)(panel) * s;
                    assert!(
                        d.rect.h < least || (d.rect.h - least) % row == 0,
                        "{face:?} {size:?} {p:?}: {panel:?} is {} high, least {least}, rows of {row}",
                        d.rect.h
                    );
                }
            }
        }
    });
}

#[test]
fn at_2x_the_color_is_compact_while_both_lists_stay_open() {
    use crate::workspace::model::{Panel, Preset, Side};
    in_both_faces(|face| {
        let size = (1920, 1003, 2);
        let mut a = named_app(size, 3);
        a.choose_preset(Preset::Draw);
        redraw(&mut a, (size.0, size.1));
        let solved = &a.layout.solved;
        assert!(
            solved.effective.color_compact,
            "{face:?}: Color compacts first"
        );
        let open: Vec<Panel> = solved
            .area(Side::Right)
            .slots()
            .iter()
            .filter(|d| d.open)
            .filter_map(|d| d.panel)
            .collect();
        assert!(open.contains(&Panel::Layers), "{face:?}: {open:?}");
        if face == pikslar_ui::font::Face::Text {
            assert!(open.contains(&Panel::Palette), "{face:?}: {open:?}");
        }
    });
}

// ---- A14 and C-5: one name for each thing

/// Every text the Export dialog draws, with a tilemap layer when `map`.
fn export_texts(face: Face, size: (i32, i32, i32), map: bool) -> Vec<(Rect, String)> {
    let mut a = if map {
        crate::test_app::map_app().0
    } else {
        app(3)
    };
    a.resized(size.0, size.1, size.2);
    a.open_export_dialog(false);
    redraw(&mut a, (size.0, size.1));
    let texts = drawn_texts(&mut a, size);
    assert!(!texts.is_empty(), "{face:?} {size:?}: nothing drawn");
    texts
}

#[test]
fn export_says_sprite_sheet_png_sequence_color_rgba_and_palette_whole_and_on_screen() {
    in_both_faces(|face| {
        for size in SIZES {
            for map in [false, true] {
                let texts = export_texts(face, size, map);
                let words: Vec<&str> = texts.iter().map(|(_, t)| t.as_str()).collect();
                for want in ["Sprite sheet", "PNG sequence", "Color", "RGBA", "Palette"] {
                    let Some((r, _)) = texts.iter().find(|(_, t)| t == want) else {
                        panic!("{face:?} {size:?} map {map}: no whole {want:?} in {words:?}");
                    };
                    assert!(
                        r.x >= 0 && r.right() <= size.0 && r.bottom() <= size.1,
                        "{face:?} {size:?} map {map}: {want} at {r:?} leaves the window"
                    );
                }
                for old in ["Sheet", "Frames ", "Colour", "Full colour", "Indexed"] {
                    assert!(
                        !words.iter().any(|w| *w == old || w.contains("colour")),
                        "{face:?} {size:?} map {map}: {old:?} still drawn"
                    );
                }
            }
        }
    });
}

#[test]
fn export_messages_name_the_dialog_options_and_spell_color_as_the_app_does() {
    use pikslar_export::ExportError;
    let all = crate::export_run::app_message(ExportError::TooManyColours {
        frame: None,
        hidden: 0,
    });
    assert!(
        all.contains("PNG sequence") && all.contains("or RGBA"),
        "{all}"
    );
    let one = crate::export_run::app_message(ExportError::TooManyColours {
        frame: Some(2),
        hidden: 1,
    });
    assert!(one.contains("Export it as RGBA"), "{one}");
    let gif = crate::export_run::app_message(ExportError::GifColours { clear: false });
    for m in [&all, &one, &gif] {
        assert!(!m.contains("--") && !m.contains("colour"), "{m}");
        assert!(m.contains("colors"), "{m}");
    }
}

#[test]
fn the_layer_mode_button_and_badge_say_palette_and_rgba_in_both_faces() {
    in_both_faces(|face| {
        for size in SIZES {
            // Each mode: the badge names it, and the mode button offers the other by its name,
            // or the name alone where "To" does not fit beside it.
            use pikslar_core::ColorMode;
            for (mode, badge, offer) in [
                (ColorMode::Rgba, "RGBA", "Palette"),
                (ColorMode::Palette, "PAL", "RGBA"),
            ] {
                let mut a = named_app(size, 1);
                a.set_layer_mode(0, mode);
                let texts = drawn_texts(&mut a, size);
                let words: Vec<&str> = texts.iter().map(|(_, t)| t.as_str()).collect();
                let to = format!("To {offer}");
                assert!(words.contains(&badge), "{face:?} {size:?}: {words:?}");
                assert!(
                    words.contains(&to.as_str())
                        || words.iter().filter(|w| **w == offer).count() > 0,
                    "{face:?} {size:?} {mode:?}: {words:?}"
                );
                assert!(!words.contains(&"To Pal.") && !words.contains(&"RGB"));
            }
        }
    });
}

#[test]
fn the_rgba_badge_never_meets_a_long_layer_name() {
    use pikslar_core::ColorMode;
    in_both_faces(|face| {
        for size in SIZES {
            let mut a = named_app(size, 1);
            a.set_layer_mode(0, ColorMode::Rgba);
            let layer = a.editor.layer_id(0).expect("a layer");
            let name = "A very long layer name indeed".to_string();
            a.run(pikslar_core::LayerCmd::Rename { layer, name });
            let texts = drawn_texts(&mut a, size);
            let name_r = texts
                .iter()
                .find(|(_, t)| t.starts_with("A very") && t.ends_with("..."))
                .map(|&(r, _)| r)
                .unwrap_or_else(|| panic!("{face:?} {size:?}: no cut name"));
            let badge = texts
                .iter()
                .filter(|(r, t)| t == "RGBA" && r.y.abs_diff(name_r.y) < 4)
                .map(|&(r, _)| r)
                .next()
                .unwrap_or_else(|| panic!("{face:?} {size:?}: no badge beside the name"));
            assert!(
                name_r.right() < badge.x,
                "{face:?} {size:?}: {name_r:?} meets {badge:?}"
            );
        }
    });
}

// ---- A15: the New sprite dialog names its rows, with units, and its choices have hints

#[test]
fn new_sprite_names_its_rows_above_their_choices_in_both_faces() {
    use crate::panel_side::ID_DIALOG;
    in_both_faces(|face| {
        for size in SIZES {
            let mut a = named_app(size, 1);
            a.open_new_dialog();
            let texts = drawn_texts(&mut a, size);
            let label = |want: &str| {
                texts
                    .iter()
                    .find(|(_, t)| t == want)
                    .map(|&(r, _)| r)
                    .unwrap_or_else(|| panic!("{face:?} {size:?}: no whole {want:?}"))
            };
            let (size_label, mode_label) = (label("Size (px, square)"), label("Color mode"));
            let first_size = widget(&a, ID_DIALOG).expect("a size button");
            let first_mode = widget(&a, ID_DIALOG + 10).expect("a mode button");
            let create = widget(&a, ID_DIALOG + 20).expect("Create");
            // Each name sits above its own choices, and nothing overlaps.
            assert!(size_label.bottom() <= first_size.y, "{face:?} {size:?}");
            assert!(first_size.bottom() <= mode_label.y, "{face:?} {size:?}");
            assert!(mode_label.bottom() <= first_mode.y, "{face:?} {size:?}");
            assert!(first_mode.bottom() < create.y, "{face:?} {size:?}");
            assert!(
                create.bottom() <= size.1,
                "{face:?} {size:?}: Create off screen"
            );
        }
    });
}

#[test]
fn new_sprite_choices_say_what_they_make_and_keep_their_names() {
    let size = (1366, 705, 1);
    let mut a = named_app(size, 1);
    a.open_new_dialog();
    redraw(&mut a, (size.0, size.1));
    for (name, said) in [
        ("16", "16 by 16 pixels"),
        (
            "Palette",
            "Palette: each pixel is a swatch, so changing a swatch recolors the picture",
        ),
        ("RGBA", "RGBA: each pixel keeps its own color and alpha"),
    ] {
        let r = names(&a)
            .into_iter()
            .filter(|(n, _)| n == name)
            .map(|(_, r)| r)
            .next_back()
            .unwrap_or_else(|| panic!("no {name}"));
        hover(&mut a, size, centre(r));
        assert_eq!(hint(&a), said);
    }
}

// ---- A16: what one Export choice changes in another is said

#[test]
fn export_says_when_a_choice_changes_the_format_or_the_frames() {
    use crate::export_panel::ID_EXPORT;
    use pikslar_platform::Key;
    let size = (1366, 705, 1);
    let mut a = named_app(size, 1);
    a.open_export_dialog(false);
    redraw(&mut a, (size.0, size.1));
    let press = |a: &mut App, id: u32| {
        let r = widget(a, ID_EXPORT + id).unwrap_or_else(|| panic!("no button {id}"));
        click_at(a, size, centre(r));
        redraw(a, (size.0, size.1));
    };
    press(&mut a, 11);
    assert_eq!(
        said(&a),
        "All frames: the format is now PNG sequence, as a PNG holds one frame"
    );
    press(&mut a, 10);
    assert_eq!(said(&a), "This frame: the format is PNG again");
    key(&mut a, Key::Char(b'4'), PLAIN);
    assert_eq!(said(&a), "GIF: Frames is now All frames");
    key(&mut a, Key::Char(b'1'), PLAIN);
    assert_eq!(
        said(&a),
        "PNG: Frames is now This frame, as a PNG holds one frame"
    );
    // Choosing PNG itself from an automatic PNG sequence names what changed: the frames.
    redraw(&mut a, (size.0, size.1));
    press(&mut a, 11);
    key(&mut a, Key::Char(b'1'), PLAIN);
    assert_eq!(
        said(&a),
        "PNG: Frames is now This frame, as a PNG holds one frame"
    );
    // Between two animated formats nothing else changes, and nothing new is said.
    key(&mut a, Key::Char(b'4'), PLAIN);
    a.message = None;
    key(&mut a, Key::Char(b'5'), PLAIN);
    assert_eq!(said(&a), "");
}

// ---- A17: the top menus part their groups with a one-unit rule

#[test]
fn the_top_menus_part_their_groups_and_every_group_word_is_found() {
    use crate::frame_menu::MenuKind;
    let a = named_app((1366, 705, 1), 1);
    let at = |kind: MenuKind| {
        let entries = a.menu_entries(kind);
        let words: Vec<String> = entries
            .iter()
            .map(|(l, _)| pikslar_ui::menu::split_hint(l).0.to_string())
            .collect();
        (a.menu_groups(kind, &entries), words)
    };
    let (edit, words) = at(MenuKind::Edit);
    let starts: Vec<&str> = edit.iter().map(|&n| words[n].as_str()).collect();
    assert_eq!(
        starts,
        ["Cut", "Select all", "Flip across", "Canvas size..."]
    );
    let (file, words) = at(MenuKind::File);
    let starts: Vec<&str> = file.iter().map(|&n| words[n].as_str()).collect();
    assert_eq!(starts, ["Open a file...", "Save", "Export..."]);
    let (view, _) = at(MenuKind::View);
    assert_eq!(view.len(), 5, "every View group is found: {view:?}");
    let (window, words) = at(MenuKind::Window);
    assert_eq!(window.len(), 2);
    assert!(words[window[1]].starts_with("Reset the"), "{words:?}");
    for groups in [&edit, &file, &view, &window] {
        assert!(
            groups.windows(2).all(|w| w[0] < w[1]),
            "in order: {groups:?}"
        );
        assert!(groups.iter().all(|&n| n > 0), "never above the first row");
    }
}

#[test]
fn an_open_edit_menu_draws_its_rules_in_every_look() {
    use crate::frame_menu::MenuKind;
    use pikslar_ui::theme::{self, SystemColors, Theme};
    let looks = [
        Theme::NORMAL,
        Theme::high_contrast(&SystemColors::BLACK),
        Theme::high_contrast(&SystemColors::WHITE),
    ];
    for look in looks {
        for size in [(1366, 705, 1), (1920, 1003, 2)] {
            let mut a = named_app(size, 1);
            a.open_palette_kind((40, 30), MenuKind::Edit);
            theme::set(look);
            let mut px = vec![0u32; (size.0 * size.1) as usize];
            for _ in 0..2 {
                let mut sf = pikslar_render::Surface {
                    px: &mut px,
                    w: size.0,
                    h: size.1,
                };
                a.draw_interface(
                    &mut sf,
                    pikslar_platform::Window::detached(),
                    &mut Vec::new(),
                );
            }
            let open = a.menu.expect("open");
            let rect = a.menu_area(open);
            let s = size.2;
            let entries = a.menu_entries(MenuKind::Edit);
            for n in a.menu_groups(MenuKind::Edit, &entries) {
                let row = pikslar_ui::menu::menu_row(rect, n, s);
                let mid = (row.x + row.w / 2, row.y);
                let ink = px[(mid.1 * size.0 + mid.0) as usize];
                assert_eq!(ink, theme::rule(), "{size:?} row {n}");
                assert_ne!(theme::rule(), theme::header(), "the rule shows on the menu");
            }
            theme::set(Theme::NORMAL);
        }
    }
}

// ---- Found in capture C1: the status bar's words keep a full gap in Large text

#[test]
fn the_status_words_keep_a_gap_that_grows_with_the_text() {
    use crate::workspace::model::Preset;
    let mut faults = Vec::new();
    in_both_faces(|face| {
        for size in [(1366, 705, 1), (1920, 1003, 2)] {
            for preset in [Preset::Draw, Preset::Animate] {
                let mut a = named_app(size, 2);
                a.choose_preset(preset);
                a.notify("Large text on".to_string());
                let texts = drawn_texts(&mut a, size);
                let gap = 12 * a.ui.text_scale();
                let find = |pred: &dyn Fn(&str) -> bool| {
                    texts
                        .iter()
                        .filter(|(r, t)| pred(t) && r.bottom() > size.1 - 40 * size.2)
                        .map(|&(r, _)| r)
                        .last()
                };
                let message = find(&|t| t == "Large text on").expect("the message is drawn");
                let save = find(&|t| t == "Unsaved changes").expect("the save label is drawn");
                if message.right() + gap > save.x {
                    faults.push(format!(
                        "{face:?} {size:?} {preset:?}: {message:?} to {save:?}"
                    ));
                }
                if a.history_placed() {
                    let counter = find(&|t| t.starts_with("Edit ")).expect("the counter is drawn");
                    if save.right() + gap > counter.x {
                        faults.push(format!(
                            "{face:?} {size:?} {preset:?}: {save:?} to {counter:?}"
                        ));
                    }
                }
            }
        }
    });
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}
