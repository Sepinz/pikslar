//! Contract: what each operation of API version 1 does.

mod common;

use common::*;
use pikslar_api::{run, ApiError, Mode, Op, Reply, API_VERSION};
use pikslar_core::{rgba, BlendMode};

#[test]
fn the_version_is_one_and_is_reported() {
    assert_eq!(API_VERSION, 1);
    let mut e = project();
    assert_eq!(op(&mut e, Op::Version), Reply::Version(1));
}

#[test]
fn another_version_is_refused_before_anything_runs() {
    let mut e = project();
    let before = save(&e);
    for version in [0, 2, u32::MAX] {
        let result = run(&mut e, version, |_| -> Result<(), ApiError> {
            panic!("the body must not run")
        });
        assert_eq!(
            result,
            Err(ApiError::Version {
                wanted: version,
                supported: 1
            })
        );
    }
    assert_eq!(save(&e), before);
}

#[test]
fn the_document_is_described_by_its_ids() {
    let mut e = project();
    let ids = layer_ids(&e);
    let Reply::Document(info) = op(&mut e, Op::Document) else {
        panic!("a document")
    };
    assert_eq!((info.width, info.height), (16, 12));
    assert_eq!(info.durations, vec![100, 100]);
    let handles: Vec<i64> = info.frames.iter().map(|&h| h as i64).collect();
    assert_eq!(handles, frames(&e));
    let listed: Vec<i64> = info.layers.iter().map(|&id| i64::from(id)).collect();
    assert_eq!(listed, ids);
}

#[test]
fn a_layer_is_described() {
    let mut e = project();
    let top = layer_ids(&e)[1];
    let Reply::Layer(info) = op(&mut e, Op::Layer { layer: top }) else {
        panic!("a layer")
    };
    assert_eq!(i64::from(info.id), top);
    assert_eq!(info.index, 1);
    assert_eq!(info.kind, "paint");
    assert_eq!(info.mode, Mode::Palette);
    assert!(info.visible && !info.locked);
    assert_eq!((info.opacity, info.blend), (255, "Normal"));
}

#[test]
fn pixels_are_read_and_written_on_one_cel() {
    let mut e = project();
    let bottom = layer_ids(&e)[0];
    let f = frames(&e);
    let pixel = |frame: usize, x, y| Op::Pixel {
        layer: bottom,
        frame: f[frame],
        x,
        y,
    };
    assert_eq!(op(&mut e, pixel(0, 2, 3)), Reply::Color(RED));
    let replies = ops(
        &mut e,
        &[
            Op::SetPixel {
                layer: bottom,
                frame: f[1],
                x: 5,
                y: 6,
                color: i64::from(BLUE),
            },
            pixel(1, 5, 6),
            pixel(0, 5, 6),
            Op::ErasePixel {
                layer: bottom,
                frame: f[0],
                x: 2,
                y: 3,
            },
            pixel(0, 2, 3),
        ],
    );
    assert_eq!(replies[1], Reply::Color(BLUE));
    assert_eq!(replies[2], Reply::Color(0), "the other frame is untouched");
    assert_eq!(replies[4], Reply::Color(0), "erased");
}

#[test]
fn a_line_and_a_fill_draw_as_the_tools_do() {
    let mut e = project();
    let bottom = layer_ids(&e)[0];
    let frame = frames(&e)[1];
    let replies = ops(
        &mut e,
        &[
            Op::DrawLine {
                layer: bottom,
                frame,
                from: (0, 0),
                to: (4, 4),
                color: i64::from(RED),
            },
            Op::Fill {
                layer: bottom,
                frame,
                x: 15,
                y: 0,
                color: i64::from(BLUE),
            },
            Op::Pixel {
                layer: bottom,
                frame,
                x: 3,
                y: 3,
            },
            Op::Pixel {
                layer: bottom,
                frame,
                x: 15,
                y: 11,
            },
        ],
    );
    assert_eq!(replies[2], Reply::Color(RED));
    assert_eq!(replies[3], Reply::Color(BLUE));
}

#[test]
fn layers_are_added_named_moved_duplicated_and_removed() {
    let mut e = project();
    let [bottom, top] = [layer_ids(&e)[0], layer_ids(&e)[1]];
    let added = created(&op(
        &mut e,
        Op::AddLayer {
            above: Some(bottom),
            mode: Mode::Rgba,
            name: Some("Ink".into()),
        },
    ));
    assert_eq!(layer_ids(&e), vec![bottom, added, top]);
    assert_eq!(e.doc().layers()[1].name, "Ink");
    let copy = created(&op(&mut e, Op::DuplicateLayer { layer: added }));
    assert_eq!(layer_ids(&e), vec![bottom, added, copy, top]);
    ops(
        &mut e,
        &[
            Op::RenameLayer {
                layer: top,
                name: "Top".into(),
            },
            Op::MoveLayer { layer: top, to: 0 },
            Op::RemoveLayer { layer: copy },
        ],
    );
    assert_eq!(layer_ids(&e), vec![top, bottom, added]);
    assert_eq!(e.doc().layers()[0].name, "Top");
    let on_top = created(&op(
        &mut e,
        Op::AddLayer {
            above: None,
            mode: Mode::Palette,
            name: None,
        },
    ));
    assert_eq!(layer_ids(&e).last(), Some(&on_top));
}

#[test]
fn layer_settings_are_set() {
    let mut e = project();
    let top = layer_ids(&e)[1];
    ops(
        &mut e,
        &[
            Op::SetOpacity {
                layer: top,
                opacity: 128,
            },
            Op::SetVisible {
                layer: top,
                visible: false,
            },
            Op::SetBlend {
                layer: top,
                blend: "Multiply".into(),
            },
        ],
    );
    let layer = &e.doc().layers()[1];
    assert_eq!(layer.opacity, 128);
    assert!(!layer.visible);
    assert_eq!(layer.blend, BlendMode::Multiply);
}

#[test]
fn frames_are_added_timed_tagged_and_removed() {
    let mut e = project();
    let f = frames(&e);
    let made = made_frame(&op(&mut e, Op::AddFrame { at: 2, copy: true }));
    assert_eq!(frames(&e), vec![f[0], f[1], made]);
    ops(
        &mut e,
        &[
            Op::SetDuration {
                frame: made,
                ms: 250,
            },
            Op::AddTag {
                name: "walk".into(),
                from: f[1],
                to: made,
            },
            Op::RemoveFrame { frame: f[0] },
        ],
    );
    let durations: Vec<u32> = e.doc().frames.iter().map(|f| f.duration_ms).collect();
    assert_eq!(durations, vec![100, 250]);
    assert_eq!(e.doc().tags.len(), 1);
    assert_eq!(e.doc().tags[0].name, "walk");
    assert_eq!((e.doc().tags[0].from, e.doc().tags[0].to), (0, 1));
}

#[test]
fn swatches_are_listed_added_recolored_and_removed() {
    let mut e = project();
    let Reply::Swatches(before) = op(&mut e, Op::Swatches) else {
        panic!("swatches")
    };
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].1, BLUE);
    let green = rgba(0, 200, 0, 255);
    let id = created(&op(
        &mut e,
        Op::AddSwatch {
            color: i64::from(green),
        },
    ));
    let gold = rgba(200, 180, 0, 255);
    let replies = ops(
        &mut e,
        &[
            Op::Recolor {
                swatch: id,
                color: i64::from(gold),
            },
            Op::RemoveSwatch {
                swatch: i64::from(before[0].0),
            },
            Op::Swatches,
        ],
    );
    let id = u32::try_from(id).expect("an id");
    assert_eq!(replies[2], Reply::Swatches(vec![(id, gold)]));
}

#[test]
fn a_palette_layer_takes_colors_through_the_palette() {
    let mut e = project();
    let top = layer_ids(&e)[1];
    let frame = frames(&e)[0];
    let replies = ops(
        &mut e,
        &[
            Op::SetPixel {
                layer: top,
                frame,
                x: 1,
                y: 1,
                color: i64::from(RED),
            },
            Op::Pixel {
                layer: top,
                frame,
                x: 1,
                y: 1,
            },
        ],
    );
    assert_eq!(replies[1], Reply::Color(RED));
}
