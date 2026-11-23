//! GIF exports against the RGBA PNG exports of the same frames: every corpus project, choice
//! of layers and scale. A GIF must show exactly those pixels, frame after frame, with the
//! palette, timing and loop asked for, or be refused because a pixel is translucent or there
//! are too many colours. Read back with the test reader in `pikslar-format/src/gif_read.rs`.

use crate::gif_read::{played, read, shown};
use crate::{
    frame_output, gif_delays, gif_output, play_order, resolve, ExportError, Frames, Layers, Output,
    Picture,
};
use pikslar_core::{BlendMode, ColorMode, Direction, Document, Grid};
use pikslar_format::png;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn corpus() -> Vec<Document> {
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

pub(crate) fn written(mut output: Output<'_>) -> Vec<u8> {
    let mut bytes = Vec::new();
    (output.write)(&mut bytes).expect("written");
    bytes
}

/// A frame as the RGBA PNG export shows it, a clear pixel as (0, 0, 0, 0).
pub(crate) fn as_png(picture: &Picture<'_>, frame: usize) -> Vec<u32> {
    let bytes = written(frame_output(picture, frame, "f.png".into()).expect("rgba"));
    let px = png::decode(&bytes).expect("decodes").px;
    px.into_iter()
        .map(|c| if c >> 24 == 0 { 0 } else { c })
        .collect()
}

/// What a GIF of `frames` must be: refused for a translucent pixel or too many colours, or
/// written.
fn verdict(frames: &[Vec<u32>]) -> Result<(), &'static str> {
    let all = frames.iter().flatten();
    if all.clone().any(|&c| !matches!(c >> 24, 0 | 255)) {
        return Err("translucent");
    }
    let colours: BTreeSet<u32> = all.copied().collect();
    match colours.len() > 256 {
        true => Err("colours"),
        false => Ok(()),
    }
}

/// Exports `order` of `picture` as a GIF playing `plays` times and checks every frame shown,
/// its delay and the loop against the RGBA exports. True if it was written, false if refused,
/// and only if it had to be.
pub(crate) fn check(picture: &Picture<'_>, order: &[usize], plays: u32) -> bool {
    let frames: Vec<Vec<u32>> = order.iter().map(|&f| as_png(picture, f)).collect();
    let result = gif_output(picture, order.to_vec(), plays, "a.gif".into());
    match (verdict(&frames), result) {
        (Ok(()), Ok(output)) => {
            let bytes = written(output);
            let r = read(&bytes);
            assert_eq!(r.size, (picture.width(), picture.height()));
            assert_eq!(shown(&r), frames, "every frame shown as the PNG shows it");
            let twice: Vec<Vec<u32>> = frames.iter().chain(&frames).cloned().collect();
            if order.len() > 1 && plays != 1 {
                assert_eq!(
                    played(&r, 2),
                    twice,
                    "played again over what the last frame left"
                );
            }
            assert!(r.frames.iter().all(|f| matches!(f.disposal, 1 | 2)));
            let durations = order.iter().map(|&f| picture.doc().frames[f].duration_ms);
            let delays: Vec<u16> = r.frames.iter().map(|f| f.delay).collect();
            let still = order.len() == 1;
            assert_eq!(
                delays,
                if still {
                    vec![0]
                } else {
                    gif_delays(durations).0
                }
            );
            let repeats = match (still, plays) {
                (true, _) | (_, 1) => None,
                (false, n) => Some(n.saturating_sub(1) as u16),
            };
            assert_eq!(r.repeats, repeats);
            true
        }
        (Err("translucent"), Err(ExportError::Translucent { .. })) => false,
        (Err("colours"), Err(ExportError::GifColours { .. })) => false,
        (expected, got) => panic!("expected {expected:?}, got {:?}", got.err()),
    }
}

#[test]
fn every_corpus_animation_is_its_rgba_frames_or_refused_for_a_reason_it_has() {
    let (mut checked, mut refused) = (0, 0);
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
                match check(&picture, &order, 0) {
                    true => checked += 1,
                    false => refused += 1,
                }
            }
        }
    }
    assert!(checked > 20, "{checked} written, {refused} refused");
}

/// A document of `w` by `h` with one frame for each of `frames`, every cell that colour.
pub(crate) fn frames_of(frames: &[u32], (w, h): (usize, usize)) -> Document {
    let mut doc = Document::new(w, h, ColorMode::Rgba).expect("valid");
    for (at, &c) in frames.iter().enumerate() {
        if at > 0 {
            doc.add_frame(at, false).expect("room");
        }
        doc.set_cel(0, at, Grid::from_cells(w, h, &vec![c; w * h]))
            .expect("cel");
    }
    doc
}

#[test]
fn a_clear_pixel_never_shows_the_frame_before_it() {
    let mut doc = frames_of(&[0xff00_00ff, 0, 0xff00_ff00], (6, 4));
    let mut dots = vec![0u32; 24];
    dots[5] = 0xff00_00ff;
    doc.set_cel(0, 1, Grid::from_cells(6, 4, &dots))
        .expect("cel");
    let picture = Picture::visible(&doc);
    assert!(check(&picture, &[0, 1, 2, 1, 0], 0));
    let r = read(&written(
        gif_output(&picture, vec![0, 1], 0, "a.gif".into()).expect("fits"),
    ));
    assert_eq!(
        shown(&r)[1],
        dots,
        "frame 2 is its own clear pixels and one dot"
    );
}

#[test]
fn a_tag_plays_in_its_direction_and_the_rest_play_forward() {
    let colours: Vec<u32> = (1..=6).map(|i| 0xff00_0000 | (i * 0x10)).collect();
    let mut doc = frames_of(&colours, (3, 3));
    for (i, ms) in [100, 50, 15, 40, 1000, 30].iter().enumerate() {
        doc.frames[i].duration_ms = *ms;
    }
    doc.add_tag("walk", 1, 4).expect("tag");
    doc.add_tag("back", 0, 2).expect("tag");
    doc.add_tag("two", 4, 5).expect("tag");
    doc.tags[0].direction = Direction::PingPong;
    doc.tags[1].direction = Direction::Reverse;
    doc.tags[2].direction = Direction::PingPong;
    let order = |which| play_order(&doc, &which).expect("frames");
    assert_eq!(order(Frames::Tag("walk".into())), [1, 2, 3, 4, 3, 2]);
    assert_eq!(order(Frames::Tag("back".into())), [2, 1, 0]);
    assert_eq!(order(Frames::Tag("two".into())), [4, 5]);
    assert_eq!(order(Frames::All), [0, 1, 2, 3, 4, 5]);
    assert_eq!(order(Frames::Range(2, 3)), [2, 3]);
    let picture = Picture::visible(&doc);
    for which in ["walk", "back", "two"] {
        assert!(check(&picture, &order(Frames::Tag(which.into())), 3));
    }
    assert!(check(&picture, &order(Frames::All), 1));
}

#[test]
fn delays_round_to_hundredths_carry_the_error_and_never_go_under_two() {
    assert_eq!(gif_delays([100, 100]), (vec![10, 10], 0));
    assert_eq!(
        gif_delays([33, 33, 34]),
        (vec![3, 4, 3], 0),
        "100 ms in all"
    );
    assert_eq!(gif_delays([25, 25]), (vec![3, 2], 0));
    assert_eq!(gif_delays([15, 15, 15, 15]), (vec![2, 2, 2, 2], 2));
    assert_eq!(
        gif_delays([10, 100]),
        (vec![2, 10], 1),
        "a raised delay takes no time from the next"
    );
    assert_eq!(gif_delays([60_000]), (vec![6000], 0));
    let (delays, raised) = gif_delays([26u32; 1000]);
    let total: u32 = delays.iter().map(|&d| u32::from(d) * 10).sum();
    assert!(total.abs_diff(26_000) <= 5, "{total} ms for 26,000");
    assert_eq!(raised, 0);
    let (delays, raised) = gif_delays([16u32; 10]);
    assert_eq!(
        delays, [2; 10],
        "1.6 hundredths rounds to 2 or 1, and 1 is raised"
    );
    assert_eq!(raised, 4);
}

#[test]
fn colours_and_alpha_a_gif_cannot_show_are_refused_with_where_they_are() {
    let opaque: Vec<u32> = (0..256).map(|i| 0xff00_0000 | i).collect();
    let fits = |colours: &[u32]| {
        let doc = frames_of(colours, (2, 2));
        let picture = Picture::visible(&doc);
        let order: Vec<usize> = (0..colours.len()).collect();
        gif_output(&picture, order, 0, "a.gif".into()).map(|_| ())
    };
    assert!(fits(&opaque).is_ok(), "256 opaque colours");
    let mut with_clear = opaque[..255].to_vec();
    with_clear.push(0);
    assert!(fits(&with_clear).is_ok(), "255 and clear");
    with_clear.push(opaque[255]);
    assert!(matches!(
        fits(&with_clear),
        Err(ExportError::GifColours { clear: true })
    ));
    let mut more = opaque.clone();
    more.push(0xff00_0100);
    assert!(matches!(
        fits(&more),
        Err(ExportError::GifColours { clear: false })
    ));
    more.push(0x8000_0005);
    assert!(
        matches!(
            fits(&more),
            Err(ExportError::Translucent {
                frame: 257,
                x: 0,
                y: 0,
                alpha: 128
            })
        ),
        "a pixel no GIF can show is named even after too many colours"
    );
    let mut doc = frames_of(&[0xff00_0001, 0xff00_0002], (5, 40));
    let mut cells = vec![0xff00_0002; 200];
    cells[5 * 33 + 3] = 0x7f00_0002;
    doc.set_cel(0, 1, Grid::from_cells(5, 40, &cells))
        .expect("cel");
    let refused = gif_output(&Picture::visible(&doc), vec![0, 1], 0, "a.gif".into())
        .err()
        .expect("refused");
    assert!(matches!(
        refused,
        ExportError::Translucent {
            frame: 1,
            x: 3,
            y: 33,
            alpha: 127
        }
    ));
    assert_eq!(
        refused.to_string(),
        "Frame 2 has a partly clear pixel at x 3, y 33 (counted from 0 at the top left), of \
         alpha 127 out of 255. A GIF pixel is opaque or clear, and no pixel is changed. Make \
         such pixels opaque or clear, or export PNG"
    );
}

#[test]
fn layers_blend_as_the_canvas_shows_them_and_a_translucent_result_is_refused() {
    let mut doc = frames_of(&[0xff20_4060, 0xff60_4020], (8, 8));
    doc.add_layer(1, ColorMode::Rgba).expect("layer");
    let ink: Vec<u32> = (0..64)
        .map(|i| if i % 3 == 0 { 0xff00_ff80 } else { 0 })
        .collect();
    for frame in 0..2 {
        doc.set_cel(1, frame, Grid::from_cells(8, 8, &ink))
            .expect("cel");
    }
    doc.layers_mut()[1].blend = BlendMode::Multiply;
    doc.layers_mut()[1].opacity = 128;
    let picture = Picture::visible(&doc);
    assert!(
        check(&picture, &[0, 1], 0),
        "blended over an opaque layer is opaque"
    );
    let alone = Picture::new(&doc, &Layers::Only(vec![1]), 1).expect("drawn");
    assert!(
        !check(&alone, &[0, 1], 0),
        "half opacity over nothing is translucent"
    );
}

#[test]
fn clear_pixels_are_the_clear_entry_whatever_colour_they_hide() {
    let mut cells: Vec<u32> = (0..255).map(|i| 0xff00_0000 | i).collect();
    cells.extend((0..257u32).map(|i| i << 4));
    let mut doc = Document::new(16, 32, ColorMode::Rgba).expect("valid");
    doc.set_cel(0, 0, Grid::from_cells(16, 32, &cells))
        .expect("cel");
    doc.add_frame(1, true).expect("room");
    let picture = Picture::visible(&doc);
    assert!(
        check(&picture, &[0, 1], 0),
        "255 colours and clear, whatever is behind it"
    );
}

#[test]
fn held_cels_empty_frames_and_offsets_draw_as_the_canvas_draws_them() {
    let mut doc = frames_of(&[0xff00_00ff, 0, 0, 0xff00_ff00], (7, 5));
    doc.add_layer(1, ColorMode::Rgba).expect("layer");
    let mut dot = vec![0u32; 35];
    dot[8] = 0xffff_ffff;
    doc.set_cel(1, 0, Grid::from_cells(7, 5, &dot))
        .expect("cel");
    doc.hold_previous(1, 1).expect("held");
    doc.hold_previous(1, 2).expect("held");
    doc.hold_previous(0, 2).expect("held");
    let picture = Picture::visible(&doc);
    assert!(check(&picture, &[0, 1, 2, 3], 0));
}

#[test]
fn still_long_wide_and_many_frame_gifs_are_written_whole() {
    let doc = frames_of(&[0xff00_0001], (3, 2));
    assert!(
        check(&Picture::visible(&doc), &[0], 0),
        "a still image: no loop, no delay"
    );
    let wide = frames_of(&[0xff00_0001, 0xff00_0002], (8192, 2));
    assert!(check(&Picture::visible(&wide), &[0, 1], 0));
    let long = frames_of(&[0xff00_0001, 0], (1, 8192));
    assert!(check(&Picture::visible(&long), &[1, 0], 0));
    let colours: Vec<u32> = (0..4096u32).map(|i| 0xff00_0000 | (i % 200)).collect();
    let many = frames_of(&colours, (2, 2));
    let order: Vec<usize> = (0..4096).collect();
    assert!(check(&Picture::visible(&many), &order, 0));
    let pair = frames_of(&[0xff00_0001], (2, 1));
    let scaled = Picture::new(&pair, &Layers::Visible, 4096).expect("8192 by 4096");
    assert!(gif_output(&scaled, vec![0], 0, "a.gif".into()).is_ok());
}

#[test]
fn plays_are_counted_and_the_same_project_gives_the_same_bytes() {
    let doc = frames_of(&[0xff00_0001, 0xff00_0002, 0], (4, 4));
    let picture = Picture::visible(&doc);
    for plays in [0, 1, 2, 65_536] {
        assert!(check(&picture, &[0, 1, 2], plays));
    }
    assert!(matches!(
        gif_output(&picture, vec![0, 1], 65_537, "a.gif".into()),
        Err(ExportError::TooManyPlays { plays: 65_537 })
    ));
    assert!(matches!(
        gif_output(&picture, vec![], 0, "a.gif".into()),
        Err(ExportError::NoFrames)
    ));
    assert!(matches!(
        gif_output(&picture, vec![3], 0, "a.gif".into()),
        Err(ExportError::NoSuchFrame {
            frame: 3,
            frames: 3
        })
    ));
    let once = written(gif_output(&picture, vec![0, 1, 2], 0, "a.gif".into()).expect("ok"));
    let again = written(gif_output(&picture, vec![0, 1, 2], 0, "a.gif".into()).expect("ok"));
    assert_eq!(once, again);
}

#[test]
fn a_tag_chosen_by_its_place_plays_its_own_frames_when_another_shares_its_name() {
    let colours: Vec<u32> = (1..=6).map(|i| 0xff00_0000 | (i * 0x10)).collect();
    let mut doc = frames_of(&colours, (3, 3));
    doc.add_tag("", 0, 1).expect("tag");
    doc.add_tag("", 3, 5).expect("tag");
    doc.tags[1].direction = Direction::Reverse;
    let order = |which| play_order(&doc, &which).expect("frames");
    assert_eq!(order(Frames::TagAt(0)), [0, 1]);
    assert_eq!(order(Frames::TagAt(1)), [5, 4, 3]);
    assert_eq!(resolve(&doc, &Frames::TagAt(1)).expect("frames"), 3..=5);
    assert!(matches!(
        resolve(&doc, &Frames::TagAt(2)),
        Err(ExportError::NoSuchTag(_))
    ));
}
