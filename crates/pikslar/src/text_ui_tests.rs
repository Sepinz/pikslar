//! The text tool: a session of typing over a floating paste, ended by Enter or Esc.

use crate::app::{App, ToolChoice};
use crate::panel_side::ID_TOOL;
use crate::test_app::*;
use pikslar_core::color::rgba;
use pikslar_core::glyphs::{text_width, Align, Font};
use pikslar_core::{LayerCmd, LimitError, TileCmd};
use pikslar_platform::{Key, Mods};

const RED: u32 = rgba(255, 0, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);
const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};
const SHIFT_ALT: Mods = Mods {
    ctrl: false,
    shift: true,
    alt: true,
};

fn text_app() -> App {
    let mut a = app(1);
    a.editor.color = RED;
    a.set_tool(ToolChoice::Text);
    draw_whole(&mut a);
    a
}

fn type_text(a: &mut App, text: &str) {
    text.chars().for_each(|c| a.text_typed(c));
    draw_whole(a);
}

fn open_at(a: &mut App, x: i32, y: i32) {
    let at = screen(a, x, y);
    click_whole(a, at, PLAIN);
    assert!(a.text.is_some(), "a click on the canvas opens a session");
}

fn depth(a: &App) -> usize {
    a.editor.history().undo_depth()
}

fn bounds(a: &App) -> Option<(i32, i32, i32, i32)> {
    a.editor.selection().map(|s| s.bounds())
}

fn lit(a: &App) -> usize {
    let (w, h) = (a.editor.doc().width as i32, a.editor.doc().height as i32);
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| a.editor.color_at(x, y) != 0)
        .count()
}

#[test]
fn the_strip_has_a_text_tool_after_rectangle_and_x_chooses_it() {
    let mut a = app(1);
    draw_whole(&mut a);
    let at = centre_of(&a, ID_TOOL + 6);
    click_whole(&mut a, at, PLAIN);
    assert_eq!(a.tool, ToolChoice::Text);
    a.set_tool(ToolChoice::Paint(pikslar_core::Tool::Pencil));
    key(&mut a, Key::Char(b'X'), PLAIN);
    assert_eq!(a.tool, ToolChoice::Text);
    assert_eq!(
        a.keymap
            .binding(crate::shortcuts::Action::Tag)
            .map(|b| b.letter),
        Some(b'T')
    );
}

#[test]
fn a_click_opens_a_session_without_a_step_and_typing_previews_live() {
    let mut a = text_app();
    let before = depth(&a);
    open_at(&mut a, 4, 5);
    assert!(a.editor.is_pasting());
    assert_eq!(depth(&a), before);
    type_text(&mut a, "Hi");
    assert_eq!(depth(&a), before, "typing is not a history step");
    assert_eq!(
        bounds(&a).map(|b| (b.0, b.1, b.2)),
        Some((4, 5, text_width(Font::Pikslar5x7, "Hi", 1, 1)))
    );
    assert_eq!(a.text.as_ref().map(|t| t.edit.text()), Some("Hi"));
}

#[test]
fn letters_type_instead_of_choosing_tools_and_space_types_too() {
    let mut a = text_app();
    open_at(&mut a, 2, 2);
    for k in [
        Key::Char(b'B'),
        Key::Char(b'E'),
        Key::Space,
        Key::Char(b'X'),
    ] {
        key(&mut a, k, PLAIN);
    }
    assert_eq!(a.tool, ToolChoice::Text, "b is not the pencil");
    assert_eq!(a.text.as_ref().map(|t| t.edit.text()), Some("be x"));
}

#[test]
fn enter_commits_as_one_step_and_the_tool_stays() {
    let mut a = text_app();
    let before = depth(&a);
    open_at(&mut a, 3, 3);
    type_text(&mut a, "ab");
    key(&mut a, Key::Enter, PLAIN);
    assert!(a.text.is_none() && !a.editor.is_floating());
    assert_eq!(depth(&a), before + 1);
    assert_eq!(a.tool, ToolChoice::Text);
    assert!(lit(&a) > 10);
    assert_eq!(a.editor.color_at(0, 0), 0);
    a.undo();
    assert_eq!(lit(&a), 0);
}

#[test]
fn escape_cancels_and_leaves_document_and_history_alone() {
    let mut a = text_app();
    let (doc, before) = (a.editor.doc().clone(), depth(&a));
    open_at(&mut a, 3, 3);
    type_text(&mut a, "gone");
    key(&mut a, Key::Escape, PLAIN);
    assert!(a.text.is_none() && !a.editor.is_floating());
    assert_eq!(*a.editor.doc(), doc);
    assert_eq!(depth(&a), before);
}

#[test]
fn enter_on_empty_text_reverts_and_leaves_no_step() {
    let mut a = text_app();
    let before = depth(&a);
    open_at(&mut a, 3, 3);
    key(&mut a, Key::Enter, PLAIN);
    assert!(a.text.is_none() && !a.editor.is_floating());
    assert_eq!(depth(&a), before);
    open_at(&mut a, 3, 3);
    type_text(&mut a, "x");
    key(&mut a, Key::Backspace, PLAIN);
    key(&mut a, Key::Enter, PLAIN);
    assert_eq!(depth(&a), before, "text deleted again is empty text");
}

#[test]
fn alt_arrows_move_the_text_one_pixel_and_with_shift_eight() {
    let mut a = text_app();
    open_at(&mut a, 8, 8);
    type_text(&mut a, "go");
    key(&mut a, Key::Right, ALT);
    key(&mut a, Key::Down, ALT);
    assert_eq!(bounds(&a).map(|b| (b.0, b.1)), Some((9, 9)));
    key(&mut a, Key::Left, SHIFT_ALT);
    assert_eq!(bounds(&a).map(|b| (b.0, b.1)), Some((1, 9)));
    assert_eq!(
        a.text.as_ref().map(|t| t.edit.caret()),
        Some(2),
        "the caret did not move"
    );
    type_text(&mut a, "!");
    assert_eq!(
        bounds(&a).map(|b| (b.0, b.1)),
        Some((1, 9)),
        "typing keeps the place"
    );
    key(&mut a, Key::Enter, PLAIN);
    assert_eq!(depth(&a), 1, "moves and edits are one step");
}

#[test]
fn dragging_the_text_moves_it_before_commit() {
    let mut a = text_app();
    open_at(&mut a, 4, 4);
    type_text(&mut a, "mv");
    let from = screen(&a, 5, 6);
    let to = screen(&a, 9, 10);
    a.pointer_down(from.0, from.1, pikslar_platform::Button::Left, PLAIN);
    a.canvas_move(to.0, to.1);
    draw_whole(&mut a);
    a.pointer_up(to.0, to.1, pikslar_platform::Button::Left);
    draw_whole(&mut a);
    assert!(a.text.is_some(), "the session goes on");
    assert_eq!(bounds(&a).map(|b| (b.0, b.1)), Some((8, 8)));
    key(&mut a, Key::Enter, PLAIN);
    assert_eq!(depth(&a), 1);
}

#[test]
fn a_click_elsewhere_commits_and_opens_a_new_session_there() {
    let mut a = text_app();
    open_at(&mut a, 2, 2);
    type_text(&mut a, "one");
    let at = screen(&a, 20, 24);
    click_whole(&mut a, at, PLAIN);
    assert_eq!(depth(&a), 1);
    assert_eq!(a.text.as_ref().map(|t| t.edit.text()), Some(""));
    assert_eq!(bounds(&a).map(|b| (b.0, b.1)), Some((20, 24)));
}

#[test]
fn any_other_command_commits_the_text_and_ends_the_session() {
    let mut a = text_app();
    open_at(&mut a, 2, 2);
    type_text(&mut a, "x");
    dot(&mut a, 25, 25);
    draw_whole(&mut a);
    assert!(a.text.is_none());
    assert!(!a.editor.is_floating());
    assert_eq!(depth(&a), 1, "the text joins the change that took it");
    a.set_tool(ToolChoice::Text);
    open_at(&mut a, 2, 12);
    type_text(&mut a, "y");
    a.set_tool(ToolChoice::Paint(pikslar_core::Tool::Pencil));
    assert!(a.text.is_none() && !a.editor.is_floating());
    assert_eq!(depth(&a), 2);
}

#[test]
fn starting_without_the_pointer_uses_the_selection_corner_or_the_view_centre() {
    let mut a = text_app();
    key(&mut a, Key::Enter, PLAIN);
    assert!(a.text.is_some());
    let centre = a.view.to_doc(
        a.layout.canvas.x + a.layout.canvas.w / 2,
        a.layout.canvas.y + a.layout.canvas.h / 2,
    );
    assert_eq!(bounds(&a).map(|b| (b.0, b.1)), Some(centre));
    key(&mut a, Key::Escape, PLAIN);
    select(&mut a);
    key(&mut a, Key::Enter, PLAIN);
    assert_eq!(bounds(&a).map(|b| (b.0, b.1)), Some((2, 2)));
    key(&mut a, Key::Escape, PLAIN);
    assert_eq!(bounds(&a), Some((2, 2, 4, 4)), "the selection comes back");
}

#[test]
fn pasted_text_keeps_its_lines_and_is_never_the_image() {
    let mut a = text_app();
    open_at(&mut a, 2, 2);
    a.insert_pasted_text("first\r\nsecond");
    assert_eq!(
        a.text.as_ref().map(|t| t.edit.text()),
        Some("first\nsecond")
    );
    key(&mut a, Key::Char(b'V'), CTRL);
    assert_eq!(
        said(&a),
        "The clipboard was busy",
        "not the image's message"
    );
    assert!(a.clip.is_none() && a.text.is_some());
}

#[test]
fn control_z_steps_back_through_the_text_edits_inside_the_session() {
    let mut a = text_app();
    open_at(&mut a, 2, 2);
    type_text(&mut a, "abc");
    key(&mut a, Key::Char(b'Z'), CTRL);
    assert_eq!(a.text.as_ref().map(|t| t.edit.text()), Some("ab"));
    key(&mut a, Key::Char(b'Z'), CTRL);
    key(&mut a, Key::Char(b'Z'), CTRL);
    key(&mut a, Key::Char(b'Z'), CTRL);
    assert_eq!(a.text.as_ref().map(|t| t.edit.text()), Some(""));
    assert!(a.editor.is_pasting(), "still in the session");
    assert_eq!(depth(&a), 0);
}

#[test]
fn the_preview_follows_the_current_colour() {
    let mut a = text_app();
    open_at(&mut a, 2, 2);
    type_text(&mut a, "c");
    a.editor.color = BLUE;
    draw_whole(&mut a);
    key(&mut a, Key::Enter, PLAIN);
    let (x, y) = (0..32)
        .flat_map(|y| (0..32).map(move |x| (x, y)))
        .find(|&(x, y)| a.editor.color_at(x, y) != 0)
        .expect("ink");
    assert_eq!(a.editor.color_at(x, y), BLUE);
}

#[test]
fn characters_the_font_lacks_are_listed_and_the_text_keeps_them() {
    let mut a = text_app();
    open_at(&mut a, 2, 2);
    type_text(&mut a, "aЖ");
    assert_eq!(a.text.as_ref().map(|t| t.missing.clone()), Some(vec!['Ж']));
    assert_eq!(a.text.as_ref().map(|t| t.edit.text()), Some("aЖ"));
}

#[test]
fn text_is_cut_at_256_characters() {
    let mut a = text_app();
    open_at(&mut a, 0, 0);
    type_text(&mut a, &"w".repeat(300));
    assert_eq!(
        a.text.as_ref().map(|t| t.edit.text().chars().count()),
        Some(256)
    );
}

#[test]
fn font_scale_and_spacing_are_keys_that_change_the_preview() {
    let mut a = text_app();
    open_at(&mut a, 2, 2);
    type_text(&mut a, "ab");
    let one = bounds(&a).map(|b| b.2);
    key(&mut a, Key::Char(b'T'), CTRL);
    assert_eq!(a.text.as_ref().map(|t| t.scale), Some(2));
    assert_eq!(bounds(&a).map(|b| b.2), one.map(|w| w * 2));
    key(
        &mut a,
        Key::Char(b'T'),
        Mods {
            shift: true,
            ..CTRL
        },
    );
    assert_eq!(a.text.as_ref().map(|t| t.scale), Some(1));
    key(&mut a, Key::Char(b'K'), CTRL);
    assert_eq!(a.text.as_ref().map(|t| t.spacing), Some(2));
    assert_eq!(
        bounds(&a).map(|b| b.2),
        Some(text_width(Font::Pikslar5x7, "ab", 1, 2))
    );
    key(&mut a, Key::Char(b'F'), CTRL);
    assert_eq!(a.text.as_ref().map(|t| t.font), Some(Font::Tiny3x5));
    key(&mut a, Key::Enter, PLAIN);
    open_at(&mut a, 2, 20);
    assert_eq!(
        a.text.as_ref().map(|t| t.spacing),
        Some(2),
        "the choice is kept"
    );
}

#[test]
fn layers_a_paste_is_refused_on_open_no_session() {
    let mut a = text_app();
    let layer = a.editor.layer_id(0).expect("layer");
    a.editor
        .apply(LayerCmd::SetLocked {
            layer,
            locked: true,
        })
        .expect("lock");
    let at = screen(&a, 4, 4);
    click_whole(&mut a, at, PLAIN);
    assert!(a.text.is_none() && !a.editor.is_floating());
    assert_eq!(said(&a), format!("Error: {}", LimitError::Locked));
    let (mut m, _) = map_app();
    m.set_tool(ToolChoice::Text);
    m.editor.layer = 1;
    let _ = TileCmd::DeleteSelected;
    let at = screen(&m, 4, 4);
    click_whole(&mut m, at, PLAIN);
    assert!(m.text.is_none());
}

#[test]
fn the_session_has_no_animation_so_idle_stays_idle() {
    let mut a = text_app();
    open_at(&mut a, 2, 2);
    type_text(&mut a, "still");
    assert!(a.ants_wait().is_none());
}

#[test]
fn the_context_bar_shows_the_field_its_controls_and_keep_and_cancel() {
    let mut a = text_app();
    open_at(&mut a, 2, 2);
    draw_whole(&mut a);
    let bar = a.layout.context;
    for id in crate::text_ui::ID_TEXT..crate::text_ui::ID_TEXT + 7 {
        let at = a.ui.widgets().iter().find(|&&(_, w)| w == id);
        let (r, _) = at.unwrap_or_else(|| panic!("{id} is not shown"));
        assert!(bar.contains(r.x, r.y) && r.bottom() <= bar.bottom(), "{id}");
    }
    key(&mut a, Key::Escape, PLAIN);
    draw_whole(&mut a);
    assert!(!a
        .ui
        .widgets()
        .iter()
        .any(|&(_, w)| w == crate::text_ui::ID_TEXT));
}

const SHIFT_ONLY: Mods = Mods {
    ctrl: false,
    shift: true,
    alt: false,
};

#[test]
fn shift_enter_starts_a_new_line_and_enter_still_commits() {
    let mut a = text_app();
    let before = depth(&a);
    open_at(&mut a, 2, 2);
    type_text(&mut a, "ab");
    key(&mut a, Key::Enter, SHIFT_ONLY);
    type_text(&mut a, "c");
    assert_eq!(a.text.as_ref().map(|t| t.edit.text()), Some("ab\nc"));
    assert_eq!(bounds(&a).map(|b| (b.2, b.3)), Some((11, 19)));
    assert!(a.text.is_some(), "Shift+Enter did not commit");
    key(&mut a, Key::Enter, PLAIN);
    assert!(a.text.is_none());
    assert_eq!(depth(&a), before + 1);
}

#[test]
fn line_breaks_count_toward_the_256_character_cap() {
    let mut a = text_app();
    open_at(&mut a, 0, 0);
    type_text(&mut a, &"w".repeat(255));
    key(&mut a, Key::Enter, SHIFT_ONLY);
    key(&mut a, Key::Enter, SHIFT_ONLY);
    assert_eq!(
        a.text.as_ref().map(|t| t.edit.text().chars().count()),
        Some(256)
    );
    assert_eq!(said(&a), "Text stops at 256 letters");
}

#[test]
fn pasted_text_keeps_its_lines_in_a_session() {
    let mut a = text_app();
    open_at(&mut a, 2, 2);
    a.insert_pasted_text("one\r\ntwo\rthree");
    assert_eq!(
        a.text.as_ref().map(|t| t.edit.text()),
        Some("one\ntwo\nthree")
    );
}

#[test]
fn alignment_is_a_key_and_a_button_and_is_kept_for_the_next_session() {
    let mut a = text_app();
    open_at(&mut a, 2, 2);
    type_text(&mut a, "wwww");
    key(&mut a, Key::Enter, SHIFT_ONLY);
    type_text(&mut a, "w");
    let at = |a: &App| a.text.as_ref().map(|t| t.align);
    assert_eq!(at(&a), Some(Align::Left));
    key(&mut a, Key::Char(b'L'), CTRL);
    assert_eq!(at(&a), Some(Align::Center));
    key(
        &mut a,
        Key::Char(b'L'),
        Mods {
            shift: true,
            ..CTRL
        },
    );
    assert_eq!(at(&a), Some(Align::Left));
    draw_whole(&mut a);
    let button = centre_of(&a, crate::text_ui::ID_TEXT + 4);
    click_whole(&mut a, button, PLAIN);
    assert_eq!(at(&a), Some(Align::Center));
    key(&mut a, Key::Enter, PLAIN);
    open_at(&mut a, 2, 22);
    assert_eq!(at(&a), Some(Align::Center), "the choice is kept");
}

#[test]
fn a_centred_short_line_is_drawn_centred_on_the_canvas() {
    let mut a = text_app();
    open_at(&mut a, 0, 0);
    key(&mut a, Key::Char(b'L'), CTRL);
    type_text(&mut a, "wwww");
    key(&mut a, Key::Enter, SHIFT_ONLY);
    type_text(&mut a, "i");
    key(&mut a, Key::Enter, PLAIN);
    let lit_x = |y: i32| {
        (0..32)
            .filter(|&x| a.editor.color_at(x, y) != 0)
            .collect::<Vec<_>>()
    };
    let wide = lit_x(5);
    let short = lit_x(15);
    let (w, s) = (
        wide.iter().max().unwrap_or(&0),
        short.iter().min().unwrap_or(&0),
    );
    assert!(
        *s > 6 && *s < *w - 6,
        "the i sits in the middle of the wide line"
    );
}

#[test]
fn the_tiny_font_draws_a_session_five_rows_high_and_is_kept() {
    let mut a = text_app();
    key_font_to_tiny(&mut a);
    open_at(&mut a, 2, 2);
    type_text(&mut a, "Ab");
    assert_eq!(bounds(&a).map(|b| (b.2, b.3)), Some((7, 5)));
    assert_eq!(a.text.as_ref().map(|t| t.font), Some(Font::Tiny3x5));
    key(&mut a, Key::Enter, PLAIN);
    open_at(&mut a, 2, 20);
    assert_eq!(a.text.as_ref().map(|t| t.font), Some(Font::Tiny3x5), "kept");
}

fn key_font_to_tiny(a: &mut App) {
    key(a, Key::Char(b'F'), CTRL);
    assert_eq!(a.text_style.font, Font::Tiny3x5);
}

fn shift_enter(a: &mut App, times: usize) {
    let shift = Mods {
        shift: true,
        ..PLAIN
    };
    (0..times).for_each(|_| key(a, Key::Enter, shift));
}

/// A long line over many empty ones would make a drawing of millions of pixels for a few
/// characters: the edit that would pass the limit is refused, and the text stays as it was.
#[test]
fn an_edit_that_would_make_the_drawing_too_large_is_refused() {
    let mut a = text_app();
    open_at(&mut a, 0, 0);
    for _ in 0..7 {
        key(&mut a, Key::Char(b'T'), CTRL);
    }
    assert_eq!(a.text.as_ref().map(|t| t.scale), Some(8));
    type_text(&mut a, &"W".repeat(144));
    shift_enter(&mut a, 60);
    let kept = a.text.as_ref().map(|t| t.edit.text().to_owned());
    let lines = kept.as_deref().map_or(0, |t| t.lines().count());
    assert!(
        lines < 60,
        "the breaks stopped before the drawing grew too large: {lines}"
    );
    assert!(said(&a).contains("too large"), "{}", said(&a));
    let (w, h) = bounds(&a)
        .map(|b| (b.2 as usize, b.3 as usize))
        .unwrap_or((0, 0));
    assert!(w * h <= pikslar_core::glyphs::MAX_TEXT_AREA, "{w}x{h}");
}

#[test]
fn a_larger_scale_that_would_make_the_drawing_too_large_is_refused() {
    let mut a = text_app();
    open_at(&mut a, 0, 0);
    type_text(&mut a, &"W".repeat(144));
    shift_enter(&mut a, 30);
    for _ in 0..7 {
        key(&mut a, Key::Char(b'T'), CTRL);
    }
    let scale = a.text.as_ref().map_or(0, |t| t.scale);
    assert!(
        scale < 8,
        "the scale stopped before the drawing grew too large: {scale}"
    );
    assert!(said(&a).contains("too large"), "{}", said(&a));
}

#[test]
fn a_shortcut_that_opens_a_dialog_commits_the_text_session_so_keys_reach_the_dialog() {
    let ctrl_shift = Mods {
        ctrl: true,
        shift: true,
        alt: false,
    };
    for (key_press, mods) in [
        (Key::Char(b'M'), ctrl_shift),
        (Key::Char(b'H'), ctrl_shift),
        (Key::Char(b'N'), CTRL),
        (Key::Char(b'E'), CTRL),
    ] {
        let mut a = text_app();
        open_at(&mut a, 3, 3);
        type_text(&mut a, "Hi");
        a.key_down(key_press, mods, false, pikslar_platform::Window::detached());
        assert!(a.text.is_none(), "{key_press:?}: the session was committed");
        assert!(a.panel_open(), "{key_press:?}: a dialog is open");
        // Escape now reaches the dialog and closes it.
        a.key_down(
            Key::Escape,
            PLAIN,
            false,
            pikslar_platform::Window::detached(),
        );
        assert!(!a.panel_open(), "{key_press:?}: Escape closed it");
        assert!(lit(&a) > 0, "the text was kept");
    }
}

#[test]
fn enter_and_space_on_a_focused_cancel_or_keep_button_press_it() {
    use crate::text_ui::ID_TEXT;
    for (id, space) in [
        (ID_TEXT + 6, false),
        (ID_TEXT + 6, true),
        (ID_TEXT + 5, false),
    ] {
        let mut a = text_app();
        open_at(&mut a, 2, 2);
        type_text(&mut a, "AB");
        let before = depth(&a);
        a.focus_on(id);
        draw_whole(&mut a);
        // As the window does: the key, then the whole interface drawn (the bar is not in the
        // bottom-only draw `key` ends with, which would drop the press).
        let pressed = if space { Key::Space } else { Key::Enter };
        a.key_down(pressed, PLAIN, false, pikslar_platform::Window::detached());
        a.text_typed(if space { ' ' } else { char::from(13) });
        draw_whole(&mut a);
        assert!(a.text.is_none(), "{id} space={space}: the session ended");
        if id == ID_TEXT + 6 {
            assert_eq!(depth(&a), before, "Cancel cancelled");
            assert_eq!(lit(&a), 0);
        } else {
            assert!(lit(&a) > 0, "Keep kept");
        }
    }
}
