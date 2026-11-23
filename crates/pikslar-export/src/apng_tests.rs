//! APNG exports against the RGBA PNG exports of the same frames, cell for cell, the colour
//! behind clear pixels included: every corpus project, choice of layers and scale, and
//! projects made to test what GIF cannot show. Read back with the strict test reader in
//! `pikslar-format/src/apng_read.rs`.

use crate::apng_read::{played, png_cells, read};
use crate::gif_tests::{frames_of, written};
use crate::{apng_output, frame_output, play_order, ExportError, Frames, Layers, Picture};
use pikslar_core::{BlendMode, ColorMode, Direction, Document, Grid};
use std::path::PathBuf;

pub(crate) fn corpus() -> Vec<Document> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../pikslar-safety/corpus");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .expect("corpus")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "piks"))
        .collect();
    names.sort();
    let load = |p: &PathBuf| pikslar_format::piks::load(&std::fs::read(p).expect("read"));
    names.iter().map(|p| load(p).expect("loads")).collect()
}

/// A frame exactly as the RGBA PNG export writes it.
pub(crate) fn png_of(picture: &Picture<'_>, frame: usize) -> Vec<u32> {
    let bytes = written(frame_output(picture, frame, "f.png".into()).expect("rgba"));
    png_cells(&bytes).1
}

/// Exports `order` of `picture` as an APNG playing `plays` times and checks every frame
/// shown, played twice, its delay, the loop and the frame controls. Gives the bytes.
pub(crate) fn check(picture: &Picture<'_>, order: &[usize], plays: u32) -> Vec<u8> {
    let frames: Vec<Vec<u32>> = order.iter().map(|&f| png_of(picture, f)).collect();
    let output = apng_output(picture, order.to_vec(), plays, "a.png".into()).expect("written");
    let bytes = written(output);
    let r = read(&bytes);
    assert_eq!(r.size, (picture.width(), picture.height()));
    let twice: Vec<Vec<u32>> = frames.iter().chain(&frames).cloned().collect();
    assert_eq!(
        played(&r, 2),
        twice,
        "every frame exactly as the PNG has it"
    );
    let delays: Vec<(u16, u16)> = r.frames.iter().map(|f| f.delay).collect();
    let durations: Vec<(u16, u16)> = order
        .iter()
        .map(|&f| match picture.doc().frame_rate {
            Some(fps) => (picture.doc().frame_units(f) as u16, u16::from(fps)),
            None => (picture.doc().frames[f].duration_ms as u16, 1000),
        })
        .collect();
    assert_eq!(delays, durations, "ticks or milliseconds, exactly");
    let plays = if order.len() == 1 { 1 } else { plays };
    assert_eq!(r.plays, plays);
    assert!(
        r.frames.iter().all(|f| f.blend == 0),
        "a frame replaces what it covers"
    );
    bytes
}

#[test]
fn every_corpus_animation_is_exactly_its_rgba_frames() {
    let mut checked = 0;
    for doc in corpus() {
        let count = doc.layers().len();
        let mut choices = vec![Layers::Visible];
        choices.extend((0..count).map(|l| Layers::Only(vec![l])));
        for layers in choices {
            for scale in [1, 2] {
                let Ok(picture) = Picture::new(&doc, &layers, scale) else {
                    continue;
                };
                let order: Vec<usize> = (0..doc.frames.len()).collect();
                check(&picture, &order, 0);
                checked += 1;
            }
        }
    }
    assert!(checked > 40, "{checked} checked");
}

#[test]
fn partial_alpha_opacity_and_blending_are_kept_exactly() {
    let mut doc = frames_of(&[0x8020_4060, 0x0160_4020, 0xfe00_0000], (8, 8));
    doc.add_layer(1, ColorMode::Rgba).expect("layer");
    let ink: Vec<u32> = (0..64u32)
        .map(|i| match i % 4 {
            0 => 0xff00_ff80,
            1 => 0x4000_ff80,
            2 => 0x0012_3456,
            _ => 0,
        })
        .collect();
    for frame in 0..3 {
        doc.set_cel(1, frame, Grid::from_cells(8, 8, &ink))
            .expect("cel");
    }
    for (blend, opacity) in [
        (BlendMode::Multiply, 128),
        (BlendMode::Normal, 255),
        (BlendMode::Multiply, 7),
    ] {
        doc.layers_mut()[1].blend = blend;
        doc.layers_mut()[1].opacity = opacity;
        for layers in [
            Layers::Visible,
            Layers::Only(vec![1]),
            Layers::Only(vec![0]),
        ] {
            let picture = Picture::new(&doc, &layers, 1).expect("drawn");
            check(&picture, &[0, 1, 2, 1], 0);
        }
    }
}

#[test]
fn a_frame_that_becomes_clear_shows_nothing_of_the_frame_before() {
    let mut doc = frames_of(&[0xff00_00ff, 0, 0x8000_ff00, 0], (6, 4));
    let mut dots = vec![0u32; 24];
    dots[5] = 0xff00_00ff;
    dots[6] = 0x0100_0000;
    doc.set_cel(0, 1, Grid::from_cells(6, 4, &dots))
        .expect("cel");
    let picture = Picture::visible(&doc);
    check(&picture, &[0, 1, 2, 3, 0], 0);
    let r = read(&check(&picture, &[0, 1], 2));
    assert_eq!(
        played(&r, 1)[1],
        dots,
        "a dot, a faint pixel and clear, nothing of red"
    );
}

#[test]
fn a_tag_plays_in_its_direction_with_exact_durations() {
    let colours: Vec<u32> = (1..=6).map(|i| 0xff00_0000 | (i * 0x10)).collect();
    let mut doc = frames_of(&colours, (3, 3));
    for (i, ms) in [100, 50, 10, 40, 60_000, 33].iter().enumerate() {
        doc.frames[i].duration_ms = *ms;
    }
    doc.add_tag("walk", 1, 4).expect("tag");
    doc.add_tag("back", 0, 2).expect("tag");
    doc.tags[0].direction = Direction::PingPong;
    doc.tags[1].direction = Direction::Reverse;
    let order = |which| play_order(&doc, &which).expect("frames");
    let picture = Picture::visible(&doc);
    for which in [
        Frames::Tag("walk".into()),
        Frames::Tag("back".into()),
        Frames::All,
        Frames::Range(2, 3),
    ] {
        check(&picture, &order(which), 3);
    }
}

#[test]
fn held_cels_empty_frames_offsets_and_scale_draw_as_the_canvas_draws_them() {
    let mut doc = frames_of(&[0xff00_00ff, 0, 0, 0x7f00_ff00], (7, 5));
    doc.add_layer(1, ColorMode::Rgba).expect("layer");
    let mut dot = vec![0u32; 35];
    dot[8] = 0xc0ff_ffff;
    doc.set_cel(1, 0, Grid::from_cells(7, 5, &dot))
        .expect("cel");
    doc.hold_previous(1, 1).expect("held");
    doc.hold_previous(1, 2).expect("held");
    doc.hold_previous(0, 2).expect("held");
    for scale in [1, 3] {
        let picture = Picture::new(&doc, &Layers::Visible, scale).expect("scaled");
        check(&picture, &[0, 1, 2, 3], 0);
    }
}

#[test]
fn still_wide_long_and_many_frame_animations_are_written_whole() {
    let doc = frames_of(&[0x8000_0001], (3, 2));
    check(&Picture::visible(&doc), &[0], 0);
    let wide = frames_of(&[0xff00_0001, 0x0000_0002], (8192, 2));
    check(&Picture::visible(&wide), &[0, 1], 0);
    let long = frames_of(&[0x4400_0001, 0], (1, 8192));
    check(&Picture::visible(&long), &[1, 0], 0);
    let colours: Vec<u32> = (0..1000u32).map(|i| i.wrapping_mul(0x0103_0507)).collect();
    let many = frames_of(&colours, (2, 2));
    let order: Vec<usize> = (0..1000).collect();
    check(&Picture::visible(&many), &order, 0);
}

#[test]
fn plays_are_counted_and_the_same_project_gives_the_same_bytes() {
    let doc = frames_of(&[0xff00_0001, 0x8000_0002, 0], (4, 4));
    let picture = Picture::visible(&doc);
    for plays in [0, 1, 2, 65_536] {
        check(&picture, &[0, 1, 2], plays);
    }
    let output = |order: Vec<usize>, plays| apng_output(&picture, order, plays, "a.png".into());
    assert!(matches!(
        output(vec![0, 1], 65_537),
        Err(ExportError::TooManyPlays { plays: 65_537 })
    ));
    assert!(matches!(output(vec![], 0), Err(ExportError::NoFrames)));
    assert!(matches!(
        output(vec![3], 0),
        Err(ExportError::NoSuchFrame {
            frame: 3,
            frames: 3
        })
    ));
    let once = written(output(vec![0, 1, 2], 0).expect("ok"));
    assert_eq!(once, written(output(vec![0, 1, 2], 0).expect("ok")));
}

/// Each frame's area as (x, y, width, height), after `check` has played them all.
fn areas(picture: &Picture<'_>, order: &[usize]) -> Vec<(usize, usize, usize, usize)> {
    read(&check(picture, order, 0))
        .frames
        .iter()
        .map(|f| f.area)
        .collect()
}

/// A `w` by `h` document of one frame for each of `frames`, each `(x, y, colour)` on `under`.
fn dotted(w: usize, h: usize, under: u32, frames: &[&[(usize, usize, u32)]]) -> Document {
    let mut doc = frames_of(&vec![under; frames.len()], (w, h));
    for (f, dots) in frames.iter().enumerate() {
        let mut cells = vec![under; w * h];
        for &(x, y, c) in dots.iter() {
            cells[y * w + x] = c;
        }
        doc.set_cel(0, f, Grid::from_cells(w, h, &cells))
            .expect("cel");
    }
    doc
}

#[test]
fn each_frame_draws_the_rectangle_of_what_changed() {
    let sky = 0xffc0_9050;
    let blink: &[(usize, usize, u32)] = &[(3, 2, 0x8010_1010), (5, 2, 0xff10_1010)];
    let doc = dotted(9, 6, sky, &[&[], &[], blink, &[]]);
    let picture = Picture::visible(&doc);
    assert_eq!(
        areas(&picture, &[0, 1, 2, 3]),
        [(0, 0, 9, 6), (0, 0, 1, 1), (3, 2, 3, 1), (3, 2, 3, 1)],
        "the whole first frame, one pixel for no change, then the blink and back"
    );
    let big = Picture::new(&doc, &Layers::Visible, 3).expect("scaled");
    assert_eq!(
        areas(&big, &[0, 1, 2, 3])[2],
        (9, 6, 9, 3),
        "areas grow with the scale"
    );
}

#[test]
fn a_frame_that_clears_or_only_changes_hidden_colour_draws_that_area() {
    let doc = dotted(
        8,
        6,
        0,
        &[
            &[(1, 1, 0xff00_00ff), (6, 4, 0x4000_ff00)],
            &[],
            &[(6, 4, 0x0012_3456)],
        ],
    );
    let picture = Picture::visible(&doc);
    assert_eq!(
        areas(&picture, &[0, 1, 2, 0]),
        [(0, 0, 8, 6), (1, 1, 6, 4), (6, 4, 1, 1), (1, 1, 6, 4)],
        "what vanished is written clear; a colour behind a clear pixel is kept too"
    );
}

/// A tiny generator, so the cases are the same every run.
fn next(seed: &mut u32) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}

/// Small random animations of every kind of cell, across the 16-row band edge, in orders with
/// repeats: every frame exact, played twice.
#[test]
fn random_animations_show_every_frame_exactly() {
    let mut seed = 0x9e37_79b9;
    let looks = [0, 0x0012_3456, 0x8000_00ff, 0xff10_1010, 0x0100_0000];
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
                    true => looks[next(&mut seed) as usize % looks.len()],
                    false => 0,
                })
                .collect();
            doc.set_cel(0, f, Grid::from_cells(w, h, &cells))
                .expect("cel");
        }
        let order: Vec<usize> = (0..1 + next(&mut seed) as usize % 5)
            .map(|_| next(&mut seed) as usize % frames)
            .collect();
        let picture = Picture::new(&doc, &Layers::Visible, 1 + case % 2).expect("small");
        for plays in [0, 1, 2] {
            check(&picture, &order, plays);
        }
    }
}
