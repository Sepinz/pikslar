//! Stepping frames on a sprite larger than the view: the window composites only the changed
//! tiles it shows, and what it shows has to be what drawing everything again would show.

use crate::app::App;
use crate::test_app::WINDOW;
use pikslar_core::{rgba, Damage, DrawCmd, Tool};
use pikslar_platform::{Handler, Window};
use pikslar_render::{Compositor, Rect, Surface, View};

const SIZE: usize = 300;
const FRAMES: usize = 4;

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as usize % n.max(1)
    }
}

/// A sprite of four frames with strokes all over each, zoomed in so a part of it shows.
fn big_app(seed: u64) -> (App, Rng) {
    let mut a = App::launch(None, Some(SIZE), None);
    a.resized(WINDOW.0, WINDOW.1, 1);
    for _ in 1..FRAMES {
        a.new_frame(false);
    }
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    for frame in 0..FRAMES {
        a.set_frame(frame);
        for _ in 0..12 {
            stroke(&mut a, &mut rng);
        }
    }
    a.set_frame(0);
    a.view = View {
        num: 4,
        den: 1,
        ..a.view
    };
    a.full_canvas = true;
    (a, rng)
}

fn stroke(a: &mut App, rng: &mut Rng) {
    a.editor.color = rgba(rng.below(256) as u8, 90, 160, 255);
    let mut point = || (rng.below(SIZE) as i32, rng.below(SIZE) as i32);
    let path = vec![point(), point()];
    a.editor
        .apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path,
        })
        .expect("a stroke");
}

fn draw(a: &mut App, screen: &mut [u32]) {
    let mut surface = Surface {
        px: screen,
        w: WINDOW.0,
        h: WINDOW.1,
    };
    a.frame(&mut surface, Window::detached(), &mut Vec::new());
}

fn area(screen: &[u32], r: Rect) -> Vec<u32> {
    (r.y..r.bottom())
        .flat_map(|y| (r.x..r.right()).map(move |x| screen[(y * WINDOW.0 + x) as usize]))
        .collect()
}

/// The whole frame as a compositor made just now composites it.
fn fresh(a: &App) -> Vec<u32> {
    let mut comp = Compositor::new(SIZE, SIZE);
    comp.update(&a.editor, Damage::All, &mut Vec::new());
    comp.pixels().to_vec()
}

/// What the screen shows now equals what it shows once the canvas is drawn whole again, and
/// the compositor holds a fresh composite wherever the view shows the sprite.
fn check(a: &mut App, screen: &mut [u32], what: &str) {
    let canvas = a.layout.canvas;
    let shown = area(screen, canvas);
    a.full_canvas = true;
    draw(a, screen);
    assert!(shown == area(screen, canvas), "{what}: the screen");
    let shown = pikslar_render::shown_region(a.view, canvas, (SIZE, SIZE));
    let wanted = fresh(a);
    for y in shown.y..shown.bottom() {
        let row = y as usize * SIZE;
        let range = row + shown.x as usize..row + shown.right() as usize;
        assert!(
            a.comp.pixels()[range.clone()] == wanted[range],
            "{what}: row {y}"
        );
    }
}

#[test]
fn stepping_frames_shows_each_frame_as_drawing_it_whole_would() {
    for seed in 1..=4 {
        let (mut a, mut rng) = big_app(seed);
        let mut screen = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
        draw(&mut a, &mut screen);
        for step in 0..24 {
            match rng.below(4) {
                0 => {
                    // Pans to another part of the sprite, which may have changed off screen.
                    a.view.x = -(rng.below(SIZE * 3) as i32);
                    a.view.y = -(rng.below(SIZE * 3) as i32);
                    a.full_canvas = true;
                }
                1 => stroke(&mut a, &mut rng),
                _ => a.step_frame(rng.below(2) == 0),
            }
            draw(&mut a, &mut screen);
            check(&mut a, &mut screen, &format!("seed {seed} step {step}"));
        }
    }
}

#[test]
fn the_true_size_preview_shows_a_part_the_view_does_not() {
    let (mut a, _) = big_app(9);
    // The view shows the top left corner; the preview shows the middle.
    a.view.x = a.layout.canvas.x;
    a.view.y = a.layout.canvas.y;
    a.toggle_true_preview();
    let mut screen = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
    draw(&mut a, &mut screen);
    for _ in 0..FRAMES {
        a.step_frame(true);
        draw(&mut a, &mut screen);
        let inner = a.true_preview.rect.inset(a.ui.scale);
        let room = (inner.w as usize, inner.h as usize);
        let (x0, y0, w, h) = crate::true_preview::shown_part((SIZE, SIZE), room);
        assert!(x0 > 0 && y0 > 0, "the middle of the sprite");
        let wanted = fresh(&a);
        let mut px = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
        let mut surface = Surface {
            px: &mut px,
            w: WINDOW.0,
            h: WINDOW.1,
        };
        let from = &wanted[y0 * SIZE + x0..];
        pikslar_render::blit_true_size(from, SIZE, (w, h), &mut surface, (inner.x, inner.y));
        assert!(
            area(&screen, inner) == area(&px, inner),
            "frame {}",
            a.editor.frame
        );
    }
    assert!(
        a.comp.stale_tiles() > 0,
        "the rest of the sprite still waits"
    );
}
