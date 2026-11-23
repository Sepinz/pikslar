//! The true-size preview as an artist sees it: P shows the frame one pixel for each pixel.

use crate::app::App;
use crate::test_app::{app, dot, WINDOW};
use crate::true_preview::PREVIEW_CAP;
use pikslar_platform::{Button, Event, Handler, Window};
use pikslar_render::{blit_true_size, Rect, Surface, View};

/// The window as the app leaves it, kept between passes as a real window keeps it.
struct Screen(Vec<u32>);

impl Screen {
    fn new() -> Self {
        Screen(vec![0u32; (WINDOW.0 * WINDOW.1) as usize])
    }

    /// Lets the app draw, and returns what it says it drew.
    fn draw(&mut self, a: &mut App) -> Vec<Rect> {
        let mut surface = Surface {
            px: &mut self.0,
            w: WINDOW.0,
            h: WINDOW.1,
        };
        let mut dirty = Vec::new();
        a.frame(&mut surface, Window::detached(), &mut dirty);
        dirty
    }

    fn area(&self, r: Rect) -> Vec<u32> {
        (r.y..r.bottom())
            .flat_map(|y| (r.x..r.right()).map(move |x| (y * WINDOW.0 + x) as usize))
            .map(|i| self.0[i])
            .collect()
    }
}

/// Where the document pixels of the preview are.
fn inner(a: &App) -> Rect {
    a.true_preview.rect.inset(a.ui.scale)
}

/// The frame as it should look at true size, without onion skin, the whole sprite shown.
fn wanted(a: &mut App, plain: &[u32]) -> Vec<u32> {
    let r = inner(a);
    let (w, h) = (r.w as usize, r.h as usize);
    let mut px = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
    let mut surface = Surface {
        px: &mut px,
        w: WINDOW.0,
        h: WINDOW.1,
    };
    blit_true_size(plain, w, (w, h), &mut surface, (r.x, r.y));
    Screen(px).area(r)
}

fn sprite(a: &App) -> (usize, usize) {
    (a.editor.doc().width, a.editor.doc().height)
}

#[test]
fn p_shows_the_frame_at_true_size_and_p_again_hides_it() {
    let mut a = app(1);
    dot(&mut a, 3, 5);
    let mut screen = Screen::new();
    screen.draw(&mut a);
    let bare = Screen(screen.0.clone());
    assert!(a.timeline_action(crate::shortcuts::Action::TruePreview));
    screen.draw(&mut a);
    let r = inner(&a);
    assert_eq!((r.w as usize, r.h as usize), sprite(&a), "one pixel each");
    assert!(
        a.layout.canvas.contains(r.x, r.y)
            && a.layout.canvas.contains(r.right() - 1, r.bottom() - 1)
    );
    assert!(
        r.x > a.layout.canvas.x + a.layout.canvas.w / 2,
        "in the right half"
    );
    let frame = a.comp.pixels().to_vec();
    assert_eq!(screen.area(r), wanted(&mut a, &frame));
    let framed = a.true_preview.rect;
    assert_ne!(screen.area(framed), bare.area(framed), "it shows");
    assert!(a.timeline_action(crate::shortcuts::Action::TruePreview));
    screen.draw(&mut a);
    assert_eq!(
        screen.area(framed),
        bare.area(framed),
        "hidden, the canvas shows again"
    );
    assert!(a.true_preview.rect.is_empty());
}

#[test]
fn it_follows_drawing() {
    let mut a = app(1);
    let mut screen = Screen::new();
    a.toggle_true_preview();
    screen.draw(&mut a);
    let before = screen.area(inner(&a));
    dot(&mut a, 10, 2);
    screen.draw(&mut a);
    let after = screen.area(inner(&a));
    assert_ne!(before, after, "the new pixel shows");
    let frame = a.comp.pixels().to_vec();
    assert_eq!(after, wanted(&mut a, &frame));
}

#[test]
fn it_stays_over_the_canvas_when_the_canvas_is_drawn_again() {
    let mut a = app(1);
    dot(&mut a, 1, 1);
    let mut screen = Screen::new();
    a.toggle_true_preview();
    screen.draw(&mut a);
    let shown = screen.area(a.true_preview.rect);
    for _ in 0..2 {
        a.full_canvas = true;
        screen.draw(&mut a);
        assert_eq!(screen.area(a.true_preview.rect), shown);
    }
    // Nothing changed: the next pass leaves it alone, and it is still there.
    screen.draw(&mut a);
    assert_eq!(screen.area(a.true_preview.rect), shown);
}

#[test]
fn it_leaves_the_onion_skin_out() {
    let mut a = app(2);
    dot(&mut a, 4, 4);
    a.set_frame(1);
    dot(&mut a, 8, 8);
    let mut screen = Screen::new();
    a.toggle_true_preview();
    screen.draw(&mut a);
    let plain = a.comp.pixels().to_vec();
    assert!(
        a.true_preview.plain.is_empty(),
        "no copy while the canvas has no skin"
    );
    a.toggle_onion();
    screen.draw(&mut a);
    assert_ne!(a.comp.pixels(), &plain[..], "the canvas shows the skin");
    assert_eq!(
        screen.area(inner(&a)),
        wanted(&mut a, &plain),
        "the preview does not"
    );
    let (w, h) = sprite(&a);
    assert_eq!(a.true_preview.plain.len(), w * h);
    // Playback hides the skin, so the frame the canvas holds is used again.
    a.playing = true;
    screen.draw(&mut a);
    assert_eq!(
        a.true_preview.plain.capacity(),
        0,
        "no copy during playback"
    );
    a.playing = false;
    screen.draw(&mut a);
    assert_eq!(a.true_preview.plain.len(), w * h);
    a.toggle_onion();
    screen.draw(&mut a);
    assert_eq!(
        a.true_preview.plain.capacity(),
        0,
        "nor once the skin is off"
    );
    a.toggle_onion();
    screen.draw(&mut a);
    a.toggle_true_preview();
    assert_eq!(
        a.true_preview.plain.capacity(),
        0,
        "the copy goes with the preview"
    );
}

#[test]
fn a_large_sprite_shows_its_middle_and_the_copy_stays_small() {
    let mut a = App::launch(None, Some(512), None);
    a.resized(WINDOW.0, WINDOW.1, 1);
    a.new_frame(false);
    a.set_frame(1);
    dot(&mut a, 256, 256);
    a.toggle_onion();
    a.toggle_true_preview();
    let mut screen = Screen::new();
    screen.draw(&mut a);
    let r = inner(&a);
    assert!(r.w as usize <= PREVIEW_CAP && r.h as usize <= PREVIEW_CAP);
    assert!(a.true_preview.plain.capacity() <= PREVIEW_CAP * PREVIEW_CAP);
    let (x0, y0) = ((512 - r.w as usize) / 2, (512 - r.h as usize) / 2);
    let mut middle = Vec::new();
    a.comp.onion = false;
    a.editor.damage_all();
    screen.draw(&mut a);
    let frame = a.comp.pixels();
    for y in y0..y0 + r.h as usize {
        middle.extend_from_slice(&frame[y * 512 + x0..][..r.w as usize]);
    }
    assert_eq!(screen.area(r), wanted(&mut a, &middle));
}

/// An app zoomed in so the sprite covers the whole canvas, with the preview showing.
fn covered_canvas(screen: &mut Screen) -> App {
    let mut a = app(1);
    let c = a.layout.canvas;
    a.view = View {
        num: 40,
        den: 1,
        x: c.x - 40,
        y: c.y - 40,
    };
    a.full_canvas = true;
    a.toggle_true_preview();
    screen.draw(&mut a);
    a
}

fn hover(a: &mut App, screen: &mut Screen, at: (i32, i32)) {
    a.event(Event::Move { x: at.0, y: at.1 }, Window::detached());
    screen.draw(a);
}

#[test]
fn the_preview_is_not_canvas_to_draw_on() {
    let mut screen = Screen::new();
    let mut a = covered_canvas(&mut screen);
    let r = a.true_preview.rect;
    let middle = (r.x + r.w / 2, r.y + r.h / 2);
    assert!(a.layout.canvas.contains(middle.0, middle.1));
    hover(&mut a, &mut screen, middle);
    assert!(a.footprint.shown.is_empty(), "no brush outline over it");
    let revision = a.editor.revision();
    a.pointer_down(middle.0, middle.1, Button::Left, crate::test_app::PLAIN);
    screen.draw(&mut a);
    a.pointer_up(middle.0, middle.1, Button::Left);
    screen.draw(&mut a);
    assert_eq!(a.editor.revision(), revision, "a click on it draws nothing");
    hover(&mut a, &mut screen, (r.x - 60, r.y - 60));
    assert!(
        !a.footprint.shown.is_empty(),
        "the canvas around it still is"
    );
}

#[test]
fn a_brush_outline_beside_it_leaves_no_trace_on_it() {
    let mut screen = Screen::new();
    let mut a = covered_canvas(&mut screen);
    let r = a.true_preview.rect;
    let shown = screen.area(r);
    // A cell 40 pixels wide just left of the preview: its outline runs onto it.
    hover(&mut a, &mut screen, (r.x - 3, r.y + 10));
    assert!(
        !a.footprint.shown.intersect(r).is_empty(),
        "the outline reaches it"
    );
    assert_eq!(screen.area(r), shown, "but is drawn under it");
    hover(&mut a, &mut screen, (r.x - 200, r.y - 100));
    assert_eq!(screen.area(r), shown, "and moving away leaves it whole");
}

#[test]
fn a_change_outside_the_part_shown_does_not_redraw_it() {
    let mut a = App::launch(None, Some(512), None);
    a.resized(WINDOW.0, WINDOW.1, 1);
    a.toggle_true_preview();
    let mut screen = Screen::new();
    screen.draw(&mut a);
    let r = a.true_preview.rect;
    dot(&mut a, 0, 0);
    let dirty = screen.draw(&mut a);
    assert!(
        !dirty.contains(&r),
        "a corner of a large sprite is not shown"
    );
    dot(&mut a, 256, 256);
    let dirty = screen.draw(&mut a);
    assert!(dirty.contains(&r), "its middle is");
}

/// A 512 pixel sprite of `frames` frames, zoomed into its top left corner so its middle, which
/// the preview shows, is off screen, with the preview drawn.
fn zoomed_into_a_corner(frames: usize, screen: &mut Screen) -> App {
    let mut a = App::launch(None, Some(512), None);
    a.resized(WINDOW.0, WINDOW.1, 1);
    for _ in 1..frames {
        a.new_frame(false);
    }
    a.set_frame(0);
    let c = a.layout.canvas;
    a.view = View {
        num: 16,
        den: 1,
        x: c.x,
        y: c.y,
    };
    a.full_canvas = true;
    a.toggle_true_preview();
    screen.draw(&mut a);
    a
}

/// The middle the preview shows, as a full composite gives it.
fn shown_middle(a: &mut App) -> Vec<u32> {
    let r = inner(a);
    let (w, h) = (r.w as usize, r.h as usize);
    let (x0, y0) = ((512 - w) / 2, (512 - h) / 2);
    a.comp.settle(&a.editor, (x0, y0, w, h));
    let frame = a.comp.pixels();
    (y0..y0 + h)
        .flat_map(|y| frame[y * 512 + x0..][..w].iter().copied())
        .collect()
}

#[test]
fn it_follows_a_change_to_its_part_off_screen() {
    let mut screen = Screen::new();
    let mut a = zoomed_into_a_corner(1, &mut screen);
    let r = a.true_preview.rect;
    let before = screen.area(inner(&a));
    dot(&mut a, 256, 256);
    let dirty = screen.draw(&mut a);
    assert!(dirty.contains(&r), "the preview is drawn again");
    let after = screen.area(inner(&a));
    assert_ne!(before, after, "the new pixel shows in it");
    let middle = shown_middle(&mut a);
    assert_eq!(after, wanted(&mut a, &middle));
    dot(&mut a, 500, 500);
    let dirty = screen.draw(&mut a);
    assert!(
        !dirty.contains(&r),
        "a change off screen and off its part is not"
    );
}

#[test]
fn it_follows_a_frame_step_off_screen() {
    let mut screen = Screen::new();
    let mut a = zoomed_into_a_corner(2, &mut screen);
    a.set_frame(1);
    dot(&mut a, 250, 250);
    screen.draw(&mut a);
    let on_second = screen.area(inner(&a));
    a.set_frame(0);
    screen.draw(&mut a);
    let on_first = screen.area(inner(&a));
    assert_ne!(on_first, on_second, "the step shows in it");
    let middle = shown_middle(&mut a);
    assert_eq!(on_first, wanted(&mut a, &middle));
}

#[test]
fn where_it_was_shows_canvas_again_when_it_moves() {
    let mut a = app(1);
    dot(&mut a, 2, 2);
    let mut screen = Screen::new();
    screen.draw(&mut a);
    let bare = Screen(screen.0.clone());
    a.toggle_true_preview();
    screen.draw(&mut a);
    let r = a.true_preview.rect;
    // As if it had last been drawn further left, where the screen still shows it.
    let old = Rect::new(r.x - 100, r.y, r.w, r.h);
    for y in old.y..old.bottom() {
        for x in old.x..old.right() {
            screen.0[(y * WINDOW.0 + x) as usize] = 0xff12_3456;
        }
    }
    a.true_preview.rect = old;
    screen.draw(&mut a);
    assert_eq!(
        screen.area(old),
        bare.area(old),
        "the old place is canvas again"
    );
    assert_eq!(a.true_preview.rect, r);
}
