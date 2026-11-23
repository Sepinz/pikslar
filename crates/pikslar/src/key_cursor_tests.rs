use crate::app::{App, ToolChoice};
use crate::test_app::{app, draw, key, PLAIN, SHIFT, WINDOW};
use pikslar_core::Tool;
use pikslar_platform::{Event, Handler, Key, Window};

fn ink(a: &App, x: i32, y: i32) -> u32 {
    a.editor.doc().layers()[a.editor.layer]
        .cel(a.editor.frame)
        .image()
        .get(x, y)
}

fn space_up(a: &mut App) {
    a.event(Event::KeyUp { key: Key::Space }, Window::detached());
    draw(a);
}

/// The app with the keyboard cursor turned on by its key, on a 32 by 32 sprite.
fn on(frames: usize) -> App {
    let mut a = app(frames);
    key(&mut a, Key::Char(b'C'), PLAIN);
    assert!(a.key_cursor.on, "C turns the cursor on");
    a
}

#[test]
fn off_the_arrows_keep_walking_the_frames() {
    let mut a = app(3);
    key(&mut a, Key::Right, PLAIN);
    assert_eq!(a.editor.frame, 1);
    assert!(!a.key_cursor.on);
}

#[test]
fn on_the_arrows_move_it_a_pixel_or_ten_and_stop_at_the_edge() {
    let mut a = on(3);
    assert_eq!(a.key_cursor.at, (16, 16), "the middle of the sprite");
    key(&mut a, Key::Right, PLAIN);
    key(&mut a, Key::Down, SHIFT);
    assert_eq!(a.key_cursor.at, (17, 26));
    assert_eq!(a.editor.frame, 0, "the frames stay put");
    for _ in 0..3 {
        key(&mut a, Key::Down, SHIFT);
    }
    assert_eq!(a.key_cursor.at, (17, 31), "the last row");
    key(&mut a, Key::Char(b'C'), PLAIN);
    assert!(!a.key_cursor.on, "C again turns it off");
    key(&mut a, Key::Right, PLAIN);
    assert_eq!(a.editor.frame, 1, "and the arrows walk the frames again");
}

#[test]
fn a_tap_of_space_paints_one_pixel_with_the_pencil() {
    let mut a = on(1);
    a.set_tool(ToolChoice::Paint(Tool::Pencil));
    key(&mut a, Key::Left, PLAIN);
    key(&mut a, Key::Space, PLAIN);
    space_up(&mut a);
    assert_ne!(ink(&a, 15, 16), 0, "painted where the cursor is");
    assert_eq!(ink(&a, 16, 16), 0);
    assert!(!a.space_held, "Space did not start panning");
}

#[test]
fn holding_space_while_moving_draws_a_line() {
    let mut a = on(1);
    a.set_tool(ToolChoice::Paint(Tool::Line));
    key(&mut a, Key::Space, PLAIN);
    for _ in 0..5 {
        key(&mut a, Key::Right, PLAIN);
    }
    space_up(&mut a);
    for x in 16..=21 {
        assert_ne!(ink(&a, x, 16), 0, "{x} is on the line");
    }
    assert_eq!(ink(&a, 22, 16), 0);
    assert_eq!(ink(&a, 16, 17), 0);
}

#[test]
fn holding_space_while_moving_selects_a_box() {
    let mut a = on(1);
    a.set_tool(ToolChoice::Marquee);
    key(&mut a, Key::Space, PLAIN);
    key(&mut a, Key::Right, PLAIN);
    key(&mut a, Key::Right, PLAIN);
    key(&mut a, Key::Down, PLAIN);
    space_up(&mut a);
    let bounds = a.editor.selection().map(|m| m.bounds());
    assert_eq!(bounds, Some((16, 16, 3, 2)));
}

#[test]
fn a_ring_shows_where_it_is_and_goes_when_it_is_turned_off() {
    let mut a = on(1);
    let (w, h) = WINDOW;
    let mut px = vec![0u32; (w * h) as usize];
    let mut surface = pikslar_render::Surface { px: &mut px, w, h };
    a.draw_canvas(&mut surface, &mut Vec::new());
    let r = pikslar_render::screen_rect(&a.view, 16, 16, 1, 1);
    let at = |px: &[u32], x: i32, y: i32| px[(y * w + x) as usize];
    let (mid_y, s) = (r.y + r.h / 2, a.ui.scale);
    assert_eq!(at(&px, r.x - 2 * s, mid_y), 0xff00_0000, "the outer ring");
    assert_eq!(at(&px, r.x - s, mid_y), 0xffff_ffff, "the inner ring");
    key(&mut a, Key::Char(b'C'), PLAIN);
    let mut surface = pikslar_render::Surface { px: &mut px, w, h };
    a.draw_canvas(&mut surface, &mut Vec::new());
    assert_ne!(at(&px, r.x - 2 * s, mid_y), 0xff00_0000, "gone");
}

#[test]
fn a_focused_slider_keeps_the_arrows() {
    let mut a = on(1);
    key(&mut a, Key::Tab, PLAIN);
    assert!(a.ui.focus.focused().is_some());
    key(&mut a, Key::Right, PLAIN);
    assert_eq!(a.key_cursor.at, (16, 16));
}

#[test]
fn space_during_a_mouse_stroke_leaves_the_stroke_to_the_mouse() {
    let mut a = on(1);
    a.set_tool(ToolChoice::Paint(Tool::Pencil));
    let (x, y) = crate::test_app::screen(&a, 4, 4);
    a.pointer_down(x, y, pikslar_platform::Button::Left, PLAIN);
    assert!(
        a.gesture != crate::app::Gesture::None,
        "the mouse is drawing"
    );
    key(&mut a, Key::Space, PLAIN);
    space_up(&mut a);
    assert!(a.gesture != crate::app::Gesture::None, "still the mouse's");
    let (x, y) = crate::test_app::screen(&a, 8, 4);
    a.canvas_move(x, y);
    a.pointer_up(x, y, pikslar_platform::Button::Left);
    assert_ne!(
        ink(&a, 8, 4),
        0,
        "the stroke went on to where the mouse let go"
    );
    assert_eq!(ink(&a, 16, 16), 0, "and the cursor painted nothing");
}
