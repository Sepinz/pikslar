//! What each frame of a GIF draws: only what changed from the frame shown before it, and what
//! it leaves for the next. Every case is also checked whole, frame for frame, by `check`.

use crate::gif_read::{read, Read};
use crate::gif_tests::{check, frames_of, written};
use crate::{gif_output, Layers, Picture};
use pikslar_core::{ColorMode, Document, Grid};

const SKY: u32 = 0xffc0_9050;
const INK: u32 = 0xff10_1010;
const RED: u32 = 0xff00_00ff;

/// `cells` of a `w` wide document set as frame `frame` of layer `layer`.
fn paint(doc: &mut Document, (layer, frame): (usize, usize), cells: &[u32]) {
    let (w, h) = (doc.width, doc.height);
    doc.set_cel(layer, frame, Grid::from_cells(w, h, cells))
        .expect("cel");
}

/// A `w` by `h` document of `frames` frames, each with `dots` (x, y, colour) on `under`.
fn dotted(w: usize, h: usize, under: u32, frames: &[&[(usize, usize, u32)]]) -> Document {
    let mut doc = frames_of(&vec![under; frames.len()], (w, h));
    for (f, dots) in frames.iter().enumerate() {
        let mut cells = vec![under; w * h];
        for &(x, y, c) in dots.iter() {
            cells[y * w + x] = c;
        }
        paint(&mut doc, (0, f), &cells);
    }
    doc
}

/// Each frame's area as (x, y, width, height) and its disposal, 1 kept or 2 cleared.
fn steps(
    picture: &Picture<'_>,
    order: &[usize],
    plays: u32,
) -> Vec<((usize, usize, usize, usize), u8)> {
    assert!(
        check(picture, order, plays),
        "written, and every frame exact"
    );
    let r: Read = read(&written(
        gif_output(picture, order.to_vec(), plays, "a.gif".into()).expect("fits"),
    ));
    r.frames.iter().map(|f| (f.bounds, f.disposal)).collect()
}

#[test]
fn only_what_changed_is_drawn_and_the_rest_is_kept() {
    let blink: &[(usize, usize, u32)] = &[(3, 2, INK), (5, 2, INK)];
    let doc = dotted(9, 6, SKY, &[&[], &[], blink, &[]]);
    let picture = Picture::visible(&doc);
    let got = steps(&picture, &[0, 1, 2, 3], 0);
    assert_eq!(
        got,
        [
            ((0, 0, 9, 6), 1),
            ((0, 0, 1, 1), 1),
            ((3, 2, 3, 1), 1),
            ((3, 2, 3, 1), 1)
        ],
        "the whole first frame, one pixel for no change, then the blink and back"
    );
    let big = Picture::new(&doc, &Layers::Visible, 3).expect("scaled");
    let got = steps(&big, &[0, 1, 2, 3], 0);
    assert_eq!(got[2], ((9, 6, 9, 3), 1), "areas grow with the scale");
}

#[test]
fn a_sprite_on_clear_is_cut_to_its_own_pixels() {
    let doc = dotted(8, 6, 0, &[&[(2, 1, RED), (4, 3, INK)]]);
    let still = steps(&Picture::visible(&doc), &[0], 0);
    assert_eq!(still, [((2, 1, 3, 3), 1)]);
    let empty = dotted(8, 6, 0, &[&[]]);
    assert_eq!(
        steps(&Picture::visible(&empty), &[0], 0),
        [((0, 0, 1, 1), 1)]
    );
}

#[test]
fn pixels_that_vanish_are_cleared_by_the_frame_before_and_drawn_again_if_needed() {
    let doc = dotted(
        8,
        6,
        0,
        &[&[(1, 1, RED), (6, 4, INK)], &[(6, 4, INK)], &[(6, 4, RED)]],
    );
    let picture = Picture::visible(&doc);
    let got = steps(&picture, &[0, 1, 2], 1);
    assert_eq!(
        got,
        [((1, 1, 6, 4), 2), ((6, 4, 1, 1), 1), ((6, 4, 1, 1), 1)],
        "frame 1 clears its whole area, so frame 2 draws the pixel it kept"
    );
}

#[test]
fn a_loop_that_starts_with_clear_pixels_starts_on_a_clear_canvas() {
    let doc = dotted(8, 6, 0, &[&[(1, 1, RED)], &[(1, 1, RED), (5, 5, INK)]]);
    let picture = Picture::visible(&doc);
    let looped = steps(&picture, &[0, 1], 0);
    assert_eq!(
        looped,
        [((1, 1, 1, 1), 1), ((5, 5, 1, 1), 2)],
        "the last frame clears only what the first does not show"
    );
    let once = steps(&picture, &[0, 1], 1);
    assert_eq!(
        once,
        [((1, 1, 1, 1), 1), ((5, 5, 1, 1), 1)],
        "played once, nothing to clear"
    );
    let opaque = dotted(4, 4, SKY, &[&[], &[(1, 1, RED)]]);
    let solid = steps(&Picture::visible(&opaque), &[0, 1], 0);
    assert_eq!(
        solid,
        [((0, 0, 4, 4), 1), ((1, 1, 1, 1), 1)],
        "a whole first frame needs none"
    );
}

#[test]
fn a_layer_left_out_and_a_changing_background_are_compared_as_shown() {
    let mut doc = dotted(6, 4, SKY, &[&[], &[(0, 0, RED)], &[]]);
    doc.add_layer(1, ColorMode::Rgba).expect("layer");
    let mut figure = vec![0u32; 24];
    figure[9] = INK;
    for f in 0..3 {
        paint(&mut doc, (1, f), &figure);
    }
    let all = steps(&Picture::visible(&doc), &[0, 1, 2], 0);
    assert_eq!(all[1], ((0, 0, 1, 1), 1));
    let figure_only = Picture::new(&doc, &Layers::Only(vec![1]), 1).expect("layer");
    let got = steps(&figure_only, &[0, 1, 2], 0);
    assert_eq!(
        got,
        [((3, 1, 1, 1), 1), ((0, 0, 1, 1), 1), ((0, 0, 1, 1), 1)],
        "the background left out changes nothing, and the loop starts on the same figure"
    );
}

#[test]
fn a_blinking_sprite_on_clear_loops_without_clearing_itself() {
    let sprite: Vec<(usize, usize, u32)> = (0..9).map(|i| (2 + i % 3, 2 + i / 3, RED)).collect();
    let mut shut = sprite.clone();
    shut[4].2 = INK;
    let doc = dotted(8, 8, 0, &[&sprite, &shut]);
    let got = steps(&Picture::visible(&doc), &[0, 1, 0, 1], 0);
    assert_eq!(
        got,
        [
            ((2, 2, 3, 3), 1),
            ((3, 3, 1, 1), 1),
            ((3, 3, 1, 1), 1),
            ((3, 3, 1, 1), 1)
        ]
    );
}

/// A tiny generator, so the cases are the same every run.
fn next(seed: &mut u32) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}

/// Small random animations of clear and two colours, across the 16-row band edge, in orders
/// with repeats: every frame exact, played once and played again over what the loop left.
#[test]
fn random_animations_show_every_frame_exactly() {
    let mut seed = 0x9e37_79b9;
    let looks = [0, 0x0012_3456, RED, INK];
    for case in 0..400 {
        let (w, h) = (
            1 + next(&mut seed) as usize % 5,
            [1, 2, 15, 16, 17, 33][case % 6],
        );
        let frames = 1 + next(&mut seed) as usize % 4;
        let mut doc = frames_of(&vec![0; frames], (w, h));
        for f in 0..frames {
            let dense = next(&mut seed) % 4;
            let cells: Vec<u32> = (0..w * h)
                .map(|_| match next(&mut seed) % 4 < dense {
                    true => looks[next(&mut seed) as usize % 4],
                    false => 0,
                })
                .collect();
            paint(&mut doc, (0, f), &cells);
        }
        let order: Vec<usize> = (0..1 + next(&mut seed) as usize % 5)
            .map(|_| next(&mut seed) as usize % frames)
            .collect();
        let scale = 1 + case % 2;
        let picture = Picture::new(&doc, &Layers::Visible, scale).expect("small");
        for plays in [0, 1, 2] {
            assert!(check(&picture, &order, plays), "case {case}");
        }
    }
}
