use super::*;

const BOX: Rect = Rect::new(10, 10, 20, 20);

fn pass(ui: &mut Ui) -> Interaction {
    ui.begin();
    let out = ui.interact(1, BOX);
    ui.end();
    out
}

fn pointer(ui: &mut Ui, x: i32, y: i32) {
    ui.mouse = (x, y);
}

#[test]
fn click_requires_press_and_release_inside() {
    let mut ui = Ui::new();
    pointer(&mut ui, 15, 15);
    ui.pressed = true;
    ui.down = true;
    assert!(pass(&mut ui).started);
    ui.down = false;
    ui.released = true;
    assert!(pass(&mut ui).clicked);
    assert_eq!(ui.active, 0);
}

#[test]
fn releasing_outside_does_not_click() {
    let mut ui = Ui::new();
    pointer(&mut ui, 15, 15);
    ui.pressed = true;
    ui.down = true;
    pass(&mut ui);
    pointer(&mut ui, 90, 90);
    ui.down = false;
    ui.released = true;
    assert!(!pass(&mut ui).clicked);
}

#[test]
fn a_held_widget_blocks_hover_on_others() {
    let mut ui = Ui::new();
    ui.active = 2;
    pointer(&mut ui, 15, 15);
    assert!(!pass(&mut ui).hover);
}

#[test]
fn quick_click_within_one_pass_counts() {
    let mut ui = Ui::new();
    pointer(&mut ui, 15, 15);
    ui.pressed = true;
    ui.released = true;
    assert!(pass(&mut ui).clicked);
}

#[test]
fn hover_changes_request_a_redraw_only_once() {
    let mut ui = Ui::new();
    pass(&mut ui);
    assert!(ui.track(15, 15));
    assert!(!ui.track(16, 16));
    assert!(ui.track(90, 90));
}

#[test]
fn a_selected_control_still_answers_the_pointer() {
    let plain = Interaction::default();
    let hover = Interaction {
        hover: true,
        ..plain
    };
    let held = Interaction {
        held: true,
        ..hover
    };
    let colors = [
        crate::chrome::Look::of(plain, true, false).surface(),
        crate::chrome::Look::of(hover, true, false).surface(),
        crate::chrome::Look::of(held, true, false).surface(),
    ];
    assert_ne!(colors[0], colors[1]);
    assert_ne!(colors[1], colors[2]);
    assert_ne!(colors[0], colors[2]);
}

#[test]
fn a_switched_off_button_ignores_a_click() {
    let mut ui = Ui::new();
    let mut px = vec![0u32; 100 * 40];
    let mut sf = Surface {
        px: &mut px,
        w: 100,
        h: 40,
    };
    pointer(&mut ui, 15, 15);
    ui.pressed = true;
    ui.released = true;
    assert!(!icon_button_if(
        false,
        &mut ui,
        &mut sf,
        1,
        BOX,
        Icon::Minus
    ));
    assert_eq!(
        ui.widget_at(15, 15),
        1,
        "but it is still a control, not canvas"
    );
    ui.begin();
    assert!(icon_button_if(true, &mut ui, &mut sf, 1, BOX, Icon::Minus));
}

#[test]
fn buttons_carry_a_one_unit_edge() {
    let mut ui = Ui::new();
    let mut px = vec![0u32; 100 * 40];
    let mut sf = Surface {
        px: &mut px,
        w: 100,
        h: 40,
    };
    ui.begin();
    button(&mut ui, &mut sf, 1, BOX, "Go", false);
    icon_button(
        &mut ui,
        &mut sf,
        2,
        Rect::new(50, 10, 20, 20),
        Icon::Minus,
        false,
    );
    let at = |x: usize, y: usize| px[y * 100 + x];
    for (x, y) in [(11, 10), (28, 29), (19, 10), (10, 25), (51, 10), (68, 29)] {
        assert_eq!(at(x, y), crate::theme::edge(), "edge at {x},{y}");
    }
    for (x, y) in [(10, 10), (29, 29), (50, 10), (69, 29)] {
        assert_eq!(at(x, y), 0, "corner notched at {x},{y}");
    }
    assert_eq!(at(11, 11), theme::lip(), "the lip under the top edge");
    assert_eq!(at(15, 15), theme::raised(), "inside the edge is the face");
}

#[test]
fn slider_follows_the_pointer_and_clamps() {
    let mut ui = Ui::new();
    let mut px = vec![0u32; 100 * 40];
    let mut sf = Surface {
        px: &mut px,
        w: 100,
        h: 40,
    };
    let area = Rect::new(0, 0, 101, 10);
    pointer(&mut ui, 50, 5);
    ui.pressed = true;
    ui.down = true;
    let first = slider(&mut ui, &mut sf, 1, area, 0.0, |_, _| {}).expect("dragging");
    assert_eq!(
        first,
        Slide {
            value: 0.5,
            started: true
        }
    );
    ui.end();
    pointer(&mut ui, 500, 90);
    let second = slider(&mut ui, &mut sf, 1, area, 0.5, |_, _| {}).expect("dragging");
    assert_eq!(
        second,
        Slide {
            value: 1.0,
            started: false
        }
    );
}

fn focus_pass(ui: &mut Ui, sf: &mut Surface<'_>) -> bool {
    ui.begin();
    ui.focus.collect(true);
    let clicked = button(ui, sf, 1, BOX, "Go", false);
    button(ui, sf, 2, Rect::new(50, 10, 20, 20), "No", false);
    ui.focus.collect(false);
    clicked
}

#[test]
fn the_focused_button_has_a_two_unit_focus_ring_one_unit_out_and_others_none() {
    let mut ui = Ui::new();
    let mut px = vec![0u32; 100 * 40];
    let mut sf = Surface {
        px: &mut px,
        w: 100,
        h: 40,
    };
    focus_pass(&mut ui, &mut sf);
    assert_eq!(ui.focus.step(false), Some(1));
    focus_pass(&mut ui, &mut sf);
    let at = |x: usize, y: usize| px[y * 100 + x];
    for (x, y) in [
        (7, 20),
        (8, 20),
        (31, 20),
        (32, 20),
        (20, 7),
        (20, 8),
        (20, 31),
        (20, 32),
    ] {
        assert_eq!(at(x, y), theme::focus(), "ring at {x},{y}");
    }
    assert_ne!(at(9, 20), theme::focus(), "a unit of gap");
    assert_ne!(at(48, 20), theme::focus(), "the other button has none");
    assert_ne!(at(6, 20), theme::focus(), "two units, no more");
}

#[test]
fn space_on_the_focused_button_clicks_it_once() {
    let mut ui = Ui::new();
    let mut px = vec![0u32; 100 * 40];
    let mut sf = Surface {
        px: &mut px,
        w: 100,
        h: 40,
    };
    focus_pass(&mut ui, &mut sf);
    ui.focus.step(false);
    ui.focus.step(false);
    assert!(ui.focus.request_activate());
    assert!(
        !focus_pass(&mut ui, &mut sf),
        "the first button was not the one"
    );
    ui.focus.request_activate();
    ui.focus.set(1);
    ui.focus.request_activate();
    assert!(focus_pass(&mut ui, &mut sf));
    assert!(!focus_pass(&mut ui, &mut sf), "spent");
}

#[test]
fn the_layout_names_each_widget_only_when_asked() {
    let mut px = vec![0u32; 100 * 40];
    let mut sf = Surface {
        px: &mut px,
        w: 100,
        h: 40,
    };
    let mut ui = Ui::new();
    ui.begin();
    button(&mut ui, &mut sf, 7, BOX, "Export...", false);
    assert_eq!(ui.layout(), None, "nothing kept by default");
    ui.record_names();
    ui.begin();
    button(&mut ui, &mut sf, 7, BOX, "Export...", false);
    let strip = Rect::new(0, 30, 90, 8);
    ui.interact(9, strip);
    ui.name_rect(strip, "Drag to scrub");
    ui.interact(11, Rect::new(1, 1, 2, 2));
    ui.name_rect(Rect::new(0, 30, 30, 8), "Frame 1");
    assert_eq!(
        ui.layout().as_deref(),
        Some(concat!(
            "7\t10\t10\t20\t20\tExport...\tbutton\t\n",
            "9\t0\t30\t90\t8\tDrag to scrub\t\t\n",
            "11\t1\t1\t2\t2\t\t\t\n",
            "0\t0\t30\t30\t8\tFrame 1\t\t\n",
        ))
    );
    ui.begin();
    assert_eq!(ui.layout().as_deref(), Some(""), "a new pass starts empty");
}

/// Columns 7 and 8 of the line the layout gives the widget called `name`.
fn role_columns(ui: &Ui, name: &str) -> (String, String) {
    let text = ui.layout().unwrap_or_default();
    let line = text
        .lines()
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .find(|f| f.get(5) == Some(&name))
        .unwrap_or_default();
    let at = |i: usize| line.get(i).copied().unwrap_or("?").to_string();
    (at(6), at(7))
}

#[test]
fn the_toolkit_widgets_say_their_role_and_state_after_the_name() {
    let mut px = vec![0u32; 200 * 60];
    let mut sf = Surface {
        px: &mut px,
        w: 200,
        h: 60,
    };
    let mut ui = Ui::new();
    ui.record_names();
    ui.begin();
    button(&mut ui, &mut sf, 1, BOX, "Go", true);
    icon_button_if(
        false,
        &mut ui,
        &mut sf,
        2,
        Rect::new(40, 10, 20, 20),
        Icon::Plus,
    );
    slider(
        &mut ui,
        &mut sf,
        3,
        Rect::new(0, 40, 101, 10),
        0.25,
        |_, _| {},
    );
    ui.name(3, "Alpha");
    crate::fold::fold_header(
        &mut ui,
        &mut sf,
        4,
        Rect::new(100, 0, 90, 20),
        "Brush",
        false,
    );
    let col = |n: &str| role_columns(&ui, n);
    assert_eq!(col("Go"), ("button".into(), "chosen".into()));
    assert_eq!(col("Plus (off)"), ("button".into(), "disabled".into()));
    assert_eq!(col("Alpha"), ("slider".into(), "value=25".into()));
    assert_eq!(col("Brush"), ("button".into(), "collapsed".into()));
    assert_eq!(
        ui.kind_of(1).map(|k| k.role),
        Some(crate::role::Role::Button),
        "kept beside the widget even without names"
    );
}

#[test]
fn a_container_is_listed_and_hit_but_never_a_focus_stop() {
    let mut ui = Ui::new();
    ui.record_names();
    ui.begin();
    ui.focus.collect(true);
    let strip = Rect::new(0, 0, 100, 20);
    ui.container(5, strip, crate::role::Role::TabList, State::NONE);
    ui.name(5, "Tabs");
    ui.focus.collect(false);
    assert_eq!(ui.widget_at(50, 10), 5);
    assert_eq!(ui.focus.step(false), None, "nothing to walk to");
    assert_eq!(
        role_columns(&ui, "Tabs"),
        ("tab-list".into(), String::new())
    );
}

/// The relative luminance of an opaque `0xffRRGGBB` colour, for contrast ratios.
fn luminance(c: u32) -> f64 {
    let part = |shift: u32| {
        let v = f64::from((c >> shift) & 0xff) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * part(16) + 0.7152 * part(8) + 0.0722 * part(0)
}

fn contrast(a: u32, b: u32) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// A slider handle must show on a bar of any shade, in every theme: its two tones (the ink and
/// the ring round it) are never both lost against the bar behind.
#[test]
fn the_slider_handle_shows_on_dark_and_bright_bars_in_every_theme() {
    use crate::theme::{SystemColors, Theme};
    let themes = [
        (Theme::LIGHTBOX, false),
        (Theme::high_contrast(&SystemColors::BLACK), true),
        (Theme::high_contrast(&SystemColors::WHITE), true),
    ];
    for (theme, hc) in themes {
        crate::theme::set(theme);
        crate::icons::set_high_contrast(hc);
        for bar in [0xff14_1519u32, 0xff00_0000, 0xffff_ffff, 0xffff_ff00] {
            let mut ui = Ui::new();
            ui.scale = 1;
            let mut px = vec![bar; 60 * 20];
            let mut sf = Surface {
                px: &mut px,
                w: 60,
                h: 20,
            };
            ui.begin();
            let area = Rect::new(5, 5, 50, 10);
            slider(&mut ui, &mut sf, 1, area, 0.5, |_, _| {});
            // The handle's middle column and the columns beside it, at the bar's middle row.
            let row: Vec<u32> = (26..=34).map(|x| px[10 * 60 + x]).collect();
            let best = row.iter().map(|&c| contrast(c, bar)).fold(0.0, f64::max);
            assert!(
                row.contains(&theme::text()),
                "hc {hc} on {bar:#x}: {row:x?}"
            );
            assert!(best >= 3.0, "hc {hc} on {bar:#x}: best contrast {best:.1}");
        }
    }
    crate::theme::set(crate::theme::Theme::NORMAL);
    crate::icons::set_high_contrast(false);
}

/// At either end the handle stays on its track, so it never reaches the panel or an edge
/// beside it (final review, defect 17).
#[test]
fn the_slider_handle_stays_on_its_track_at_both_ends() {
    for scale in [1, 2, 3] {
        for value in [0.0, 1.0] {
            let mut ui = Ui::new();
            ui.scale = scale;
            let mut px = vec![7u32; 200 * 40];
            let mut sf = Surface {
                px: &mut px,
                w: 200,
                h: 40,
            };
            ui.begin();
            let area = Rect::new(20, 10, 100, 10);
            slider(&mut ui, &mut sf, 1, area, value, |_, _| {});
            for y in 10..20 {
                for x in (0..20).chain(120..200) {
                    assert_eq!(px[y * 200 + x], 7, "scale {scale} value {value} at {x}");
                }
            }
        }
    }
}

/// The button Enter takes looks the same in every dialog: a plain face, not Accent, whose
/// notched outline is Text (final review, defect 8).
#[test]
fn a_default_button_has_a_text_outline_with_notched_corners_and_no_accent() {
    crate::theme::set(crate::theme::Theme::LIGHTBOX);
    for scale in [1, 2] {
        let mut ui = Ui::new();
        ui.scale = scale;
        let mut px = vec![7u32; 100 * 60];
        let mut sf = Surface {
            px: &mut px,
            w: 100,
            h: 60,
        };
        ui.begin();
        let r = Rect::new(10, 10, 60, 24 * scale);
        default_button(&mut ui, &mut sf, 1, r, "Create");
        let at = |x: i32, y: i32| px[(y * 100 + x) as usize];
        assert_eq!(at(40, 10), theme::text(), "top edge");
        assert_eq!(at(10, 20), theme::text(), "left edge");
        assert_eq!(at(10, 10), 7, "the corner is notched");
        assert_ne!(at(40, 10 + 2 * scale + 1), theme::accent(), "no accent");
    }
    crate::theme::set(crate::theme::Theme::NORMAL);
}
