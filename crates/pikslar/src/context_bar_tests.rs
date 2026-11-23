//! The context bar: what each tool shows, how each control acts, how the bar gives way when it
//! is narrow, and how the keyboard reaches it.

use super::*;
use crate::app::ToolChoice;
use crate::frame_menu::MenuKind;
use crate::panel_side::ID_BRUSH;
use crate::shortcuts::Action;
use crate::test_app::*;
use pikslar_core::Tool;
use pikslar_platform::{Key, Mods};

fn shown(w: i32, h: i32, tool: ToolChoice) -> App {
    let mut a = app(1);
    a.ui.record_names();
    a.resized(w, h, 1);
    a.set_tool(tool);
    draw_whole_at(&mut a, w, h);
    draw_whole_at(&mut a, w, h);
    a
}

fn pencil() -> App {
    shown(1200, 700, ToolChoice::Paint(Tool::Pencil))
}

fn has(a: &App, id: u32) -> bool {
    a.ui.widgets().iter().any(|&(_, w)| w == id)
}

fn dump(a: &App) -> String {
    a.ui.layout().unwrap_or_default()
}

fn press(a: &mut App, k: Key, mods: Mods) {
    a.key_down(k, mods, false, pikslar_platform::Window::detached());
    draw_whole_at(a, 1200, 700);
    draw_whole_at(a, 1200, 700);
}

#[test]
fn plans_keep_everything_wide_then_short_then_give_way_from_the_right() {
    let widths = [(68, 68), (28, 28), (90, 30), (80, 30)];
    let gap = (4, 28);
    assert_eq!(
        plan(400, &widths, gap),
        Plan {
            shown: 4,
            narrow: false,
            more: false
        }
    );
    assert_eq!(
        plan(180, &widths, gap),
        Plan {
            shown: 4,
            narrow: true,
            more: false
        }
    );
    let p = plan(150, &widths, gap);
    assert_eq!((p.shown, p.narrow, p.more), (2, true, true));
    // Whatever the room, the first setting (the size) stays.
    let p = plan(166, &widths, gap);
    assert_eq!((p.shown, p.narrow, p.more), (3, true, true));
    let p = plan(10, &widths, gap);
    assert_eq!((p.shown, p.more), (1, true));
    assert_eq!(plan(10, &[], gap).shown, 0);
}

#[test]
fn the_pencil_shows_size_brush_pixel_perfect_and_rhythm_with_roles() {
    let a = pencil();
    for id in [ID_STEPPER, ID_STEPPER + 1, ID_STEPPER + 2] {
        assert!(has(&a, id), "{id}");
    }
    for id in [ID_BRUSH + 2, ID_BRUSH + 3, ID_BRUSH + 4] {
        assert!(has(&a, id), "{id}");
    }
    let text = dump(&a);
    assert!(
        text.contains("\tBrush size\tspinbutton\tvalue=1\n"),
        "{text}"
    );
    assert!(text.contains("\tPixel perfect\ttoggle\t"), "{text}");
    assert!(text.contains("\tLine rhythm\ttoggle\t"), "{text}");
    assert!(text.contains("\tBrush\tbutton\t"), "{text}");
    assert!(text.contains("\tTool settings\t"), "{text}");
    assert!(text.contains("\tActive tool: Pencil\t"), "{text}");
}

#[test]
fn the_tool_strip_no_longer_holds_the_brush_controls() {
    let a = pencil();
    let side = a.layout.side;
    for id in [ID_STEPPER, ID_BRUSH + 2, ID_BRUSH + 3, ID_BRUSH + 4] {
        let r = a.ui.widgets().iter().find(|&&(_, w)| w == id).map(|w| w.0);
        assert!(r.is_some_and(|r| !side.contains(r.x, r.y)), "{id}");
    }
}

#[test]
fn the_size_stepper_buttons_and_keys_change_the_brush_size() {
    let mut a = pencil();
    let more = centre_of(&a, ID_STEPPER + 2);
    click_whole(&mut a, more, PLAIN);
    assert_eq!(a.editor.brush_size, 2);
    let less = centre_of(&a, ID_STEPPER + 1);
    click_whole(&mut a, less, PLAIN);
    assert_eq!(a.editor.brush_size, 1);
    a.focus_on(ID_STEPPER);
    draw_whole_at(&mut a, 1200, 700);
    press(&mut a, Key::Right, PLAIN);
    press(&mut a, Key::Right, PLAIN);
    assert_eq!(a.editor.brush_size, 3);
    press(&mut a, Key::End, PLAIN);
    assert_eq!(a.editor.brush_size, 64);
    press(&mut a, Key::Home, PLAIN);
    assert_eq!(a.editor.brush_size, 1);
}

#[test]
fn pixel_perfect_and_rhythm_toggle_and_say_so() {
    let mut a = pencil();
    let before = a.editor.pixel_perfect;
    let at = centre_of(&a, ID_BRUSH + 2);
    click_whole(&mut a, at, PLAIN);
    assert_eq!(a.editor.pixel_perfect, !before);
    draw_whole_at(&mut a, 1200, 700);
    assert!(dump(&a).contains("\tPixel perfect\ttoggle\tchosen") == a.editor.pixel_perfect);
    let at = centre_of(&a, ID_BRUSH + 3);
    click_whole(&mut a, at, PLAIN);
    assert!(a.rhythm);
    draw_whole_at(&mut a, 1200, 700);
    assert!(dump(&a).contains("\tLine rhythm\ttoggle\tchosen\n"));
}

#[test]
fn the_brush_button_opens_the_brush_menu_and_shift_f10_does_too() {
    let mut a = pencil();
    let at = centre_of(&a, ID_BRUSH + 4);
    click_whole(&mut a, at, PLAIN);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::Brush));
    a.close_menu();
    draw_whole_at(&mut a, 1200, 700);
    a.focus_on(ID_BRUSH + 4);
    draw_whole_at(&mut a, 1200, 700);
    press(&mut a, Key::F(10), SHIFT);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::Brush));
}

#[test]
fn each_tool_shows_its_own_settings() {
    let cases = [
        (Tool::Eraser, true, true),
        (Tool::Line, true, false),
        (Tool::Rect, true, false),
        (Tool::Fill, false, false),
        (Tool::Picker, false, false),
    ];
    for (tool, size, brush) in cases {
        let a = shown(1200, 700, ToolChoice::Paint(tool));
        assert_eq!(has(&a, ID_STEPPER), size, "{tool:?}");
        assert_eq!(has(&a, ID_BRUSH + 4), brush, "{tool:?}");
        assert_eq!(has(&a, ID_BRUSH + 2), size, "{tool:?}");
    }
    let a = shown(1200, 700, ToolChoice::Marquee);
    assert!(has(&a, ID_MODE + 1) && !has(&a, ID_STEPPER));
    let a = shown(1200, 700, ToolChoice::Wand);
    assert!(has(&a, ID_MODE + 1) && has(&a, ID_WAND + 1));
    let a = shown(1200, 700, ToolChoice::Move);
    assert!(has(&a, ID_ONLY_CEL) && !has(&a, ID_MODE + 1));
}

#[test]
fn the_selection_mode_applies_without_shift_or_alt_and_they_still_win() {
    let mut a = shown(1200, 700, ToolChoice::Marquee);
    let add = centre_of(&a, ID_MODE + 2);
    click_whole(&mut a, add, PLAIN);
    assert_eq!(a.bar.select, SelectOp::Add);
    draw_whole_at(&mut a, 1200, 700);
    assert!(dump(&a).contains("\tSelection Add\ttoggle\tchosen\n"));
    let drag = |a: &mut App, from: (i32, i32), to: (i32, i32), mods| {
        let (p, q) = (screen(a, from.0, from.1), screen(a, to.0, to.1));
        a.pointer_down(p.0, p.1, pikslar_platform::Button::Left, mods);
        a.canvas_move(q.0, q.1);
        a.pointer_up(q.0, q.1, pikslar_platform::Button::Left);
    };
    drag(&mut a, (1, 1), (3, 3), PLAIN);
    drag(&mut a, (10, 10), (12, 12), PLAIN);
    let mask = a
        .editor
        .selection()
        .map(|m| m.contains(2, 2) && m.contains(11, 11));
    assert_eq!(mask, Some(true), "the second box was added");
    drag(&mut a, (10, 10), (12, 12), ALT);
    let mask = a.editor.selection().map(|m| m.contains(11, 11));
    assert_eq!(mask, Some(false), "Alt subtracts whatever the mode");
}

#[test]
fn the_wand_and_move_settings_turn_the_modifiers_round() {
    let mut a = shown(1200, 700, ToolChoice::Wand);
    assert!(a.wand_contiguous(false) && !a.wand_contiguous(true));
    let all = centre_of(&a, ID_WAND + 2);
    click_whole(&mut a, all, PLAIN);
    assert!(a.bar.all_matching);
    assert!(!a.wand_contiguous(false) && a.wand_contiguous(true));
    a.set_tool(ToolChoice::Move);
    draw_whole_at(&mut a, 1200, 700);
    let at = centre_of(&a, ID_ONLY_CEL);
    click_whole(&mut a, at, PLAIN);
    assert!(a.bar.only_cel);
}

#[test]
fn the_tile_switch_is_in_the_bar_and_chooses_the_mode() {
    use crate::panel_tiles::{TileMode, ID_TILES};
    let mut a = pencil();
    crate::layout_tests::add_map(&mut a, 3);
    draw_whole_at(&mut a, 1200, 700);
    let pixels = centre_of(&a, ID_TILES + 1);
    assert!(a.layout.context.contains(pixels.0, pixels.1));
    click_whole(&mut a, pixels, PLAIN);
    assert_eq!(a.tile_ui.mode, TileMode::Pixels);
    draw_whole_at(&mut a, 1200, 700);
    assert!(dump(&a).contains("\tPixels\ttoggle\tchosen\n"));
    a.focus_on(ID_TILES + 1);
    draw_whole_at(&mut a, 1200, 700);
    press(&mut a, Key::Left, PLAIN);
    assert_eq!(a.tile_ui.mode, TileMode::Tiles);
}

#[test]
fn a_narrow_bar_keeps_the_size_and_puts_the_rest_under_more() {
    crate::test_app::large_text();
    let mut a = shown(640, 560, ToolChoice::Paint(Tool::Pencil));
    // Make the bar too narrow for the pencil's settings by asking for a long tool name's room.
    a.resized(300, 560, 1);
    draw_whole_at(&mut a, 300, 560);
    draw_whole_at(&mut a, 300, 560);
    assert!(has(&a, ID_STEPPER), "the size stays");
    assert!(has(&a, ID_MORE), "More appears");
    assert!(!has(&a, ID_BRUSH + 3), "rhythm gave way");
    let before = a.rhythm;
    let at = centre_of(&a, ID_MORE);
    click_whole(&mut a, at, PLAIN);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::BarMore));
    let rows = a.bar_more_entries();
    assert!(rows.iter().any(|(l, _)| l.starts_with("Line rhythm")));
    let n = rows
        .iter()
        .position(|(l, _)| l.starts_with("Line rhythm"))
        .unwrap_or(0);
    a.bar_more_action(n);
    assert_eq!(a.rhythm, !before);
}

#[test]
fn focus_goes_to_the_bar_and_tab_walks_its_controls_before_the_tools() {
    let mut a = pencil();
    assert!(a.context_bar_action(Action::ContextBar));
    let first = a.ui.focus.focused().expect("focus");
    let r = a.ui.widgets().iter().find(|w| w.1 == first).map(|w| w.0);
    assert!(r.is_some_and(|r| a.layout.context.contains(r.x, r.y)));
    let order = a.ui.focus.order().to_vec();
    let at = |id: u32| order.iter().position(|&o| o == id);
    assert!(at(ID_STEPPER + 1) < at(ID_BRUSH + 2));
    assert!(at(ID_BRUSH + 3) < at(crate::panel_side::ID_TOOL));
    assert!(!a.context_bar_action(Action::Pencil));
}

#[test]
fn the_text_tool_keeps_and_cancels_from_the_bar() {
    let mut a = shown(1200, 700, ToolChoice::Text);
    let at = screen(&a, 2, 2);
    click_whole(&mut a, at, PLAIN);
    assert!(a.text.is_some());
    let keep = centre_of(&a, crate::text_ui::ID_TEXT + 6);
    click_whole(&mut a, keep, PLAIN);
    assert!(a.text.is_none(), "Cancel ends the session");
    let at = screen(&a, 2, 2);
    click_whole(&mut a, at, PLAIN);
    assert!(a.text.is_some());
    a.text_typed('a');
    draw_whole(&mut a);
    let keep = centre_of(&a, crate::text_ui::ID_TEXT + 5);
    click_whole(&mut a, keep, PLAIN);
    assert!(a.text.is_none());
    assert!(a.editor.history().undo_depth() > 0, "Keep drew it");
}

#[test]
fn the_bar_holds_in_high_contrast_and_at_2x() {
    for scale in [1, 2] {
        let (w, h) = (1280 * scale, 760 * scale);
        let mut a = app(1);
        a.resized(w, h, scale);
        a.ui.record_names();
        for look in [None, Some(pikslar_ui::theme::Theme::LIGHTBOX)] {
            if let Some(t) = look {
                pikslar_ui::theme::set(t);
            }
            draw_whole_at(&mut a, w, h);
            let bar = a.layout.context;
            let inside = |id: u32| {
                let r = a.ui.widgets().iter().find(|x| x.1 == id).map(|x| x.0);
                r.is_some_and(|r| bar.contains(r.x, r.y) && r.bottom() <= bar.bottom())
            };
            assert!(inside(ID_STEPPER) && inside(ID_BRUSH + 2), "scale {scale}");
        }
        pikslar_ui::theme::set(pikslar_ui::theme::Theme::NORMAL);
    }
}

#[test]
fn move_pressed_inside_a_selection_grabs_whatever_the_marquee_mode() {
    let mut a = shown(1200, 700, ToolChoice::Marquee);
    let drag = |a: &mut App, from: (i32, i32), to: (i32, i32), mods| {
        let (p, q) = (screen(a, from.0, from.1), screen(a, to.0, to.1));
        a.pointer_down(p.0, p.1, pikslar_platform::Button::Left, mods);
        a.canvas_move(q.0, q.1);
        a.pointer_up(q.0, q.1, pikslar_platform::Button::Left);
    };
    drag(&mut a, (1, 1), (6, 6), PLAIN);
    assert!(a.editor.selection().is_some());
    for mode in [SelectOp::Add, SelectOp::Subtract] {
        a.bar.select = mode;
        a.set_tool(ToolChoice::Move);
        draw_whole_at(&mut a, 1200, 700);
        let p = screen(&a, 3, 3);
        a.pointer_down(p.0, p.1, pikslar_platform::Button::Left, PLAIN);
        assert!(a.editor.selection().is_some(), "{mode:?} deselected");
        a.pointer_up(p.0, p.1, pikslar_platform::Button::Left);
    }
}

#[test]
fn a_narrow_bar_with_one_setting_never_opens_an_empty_more_menu() {
    for tool in [ToolChoice::Marquee, ToolChoice::Lasso, ToolChoice::Move] {
        for w in (120..400).step_by(4) {
            let a = shown(w, 700, tool);
            assert!(
                !has(&a, ID_MORE) || !a.bar_more_entries().is_empty(),
                "{tool:?} at {w}"
            );
        }
    }
}

#[test]
fn arrow_nudges_follow_only_this_cel_and_alt_turns_it_round() {
    let nudge = |only: bool, alt: bool| {
        let mut a = crate::test_app::app(3);
        dot(&mut a, 5, 5);
        let layer = a.editor.layer_id(0).expect("layer");
        let hold = pikslar_core::FrameCmd::HoldAcross {
            layer,
            source: 0,
            from: 0,
            to: 2,
        };
        a.editor.apply(hold).expect("hold");
        a.set_frame(1);
        a.bar.only_cel = only;
        a.arrow((1, 0), Mods { alt, ..PLAIN }, false);
        let at = |f: usize| a.editor.doc().layers()[0].cel(f).position();
        [at(0), at(1), at(2)]
    };
    assert_eq!(nudge(false, false), [(1, 0); 3], "the whole hold");
    assert_eq!(
        nudge(false, true),
        [(0, 0), (1, 0), (0, 0)],
        "Alt: this cel"
    );
    assert_eq!(
        nudge(true, false),
        [(0, 0), (1, 0), (0, 0)],
        "setting: this cel"
    );
    assert_eq!(nudge(true, true), [(1, 0); 3], "both turn round");
}
