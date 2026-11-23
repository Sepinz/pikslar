//! Scaled and layer-chosen exports against the compositor: every corpus project, every frame,
//! several choices of layers and scales, as a frame, a sheet and a sequence member. The
//! expected image is made apart from the export: the document with only the chosen layers
//! shown, flattened whole, then each pixel copied into a square.

use crate::{sheet_plan, write_frame_png, write_sheet_png, Layers, Picture};
use pikslar_core::Document;
use pikslar_format::{piks, png};
use pikslar_render::{flatten_document, Compositor};
use std::path::Path;

pub(crate) fn corpus() -> Vec<Document> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../pikslar-safety/corpus");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .expect("corpus")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "piks"))
        .collect();
    names.sort();
    names
        .iter()
        .map(|p| piks::load(&std::fs::read(p).expect("read")).expect("loads"))
        .collect()
}

/// The document's own frame with exactly `layers` shown, or as it is, made `scale` times larger.
/// A chosen layer is drawn shown or hidden, so the groups it is in are shown too: they have
/// no pixels of their own to add.
pub(crate) fn expected(doc: &Document, layers: &Layers, frame: usize, scale: usize) -> Vec<u32> {
    let mut shown = doc.clone();
    if let Layers::Only(chosen) = layers {
        for (i, layer) in shown.layers_mut().iter_mut().enumerate() {
            layer.visible = chosen.contains(&i) || layer.kind == pikslar_core::LayerKind::Group;
        }
    }
    let px = flatten_document(&shown, frame, &mut Compositor::new(1, 1));
    let w = doc.width;
    let mut out = Vec::with_capacity(px.len() * scale * scale);
    for row in px.chunks_exact(w) {
        for _ in 0..scale {
            for &p in row {
                out.extend(std::iter::repeat_n(p, scale));
            }
        }
    }
    out
}

/// Choices that cover the default, each single layer, a hidden one, and pairs.
fn choices(doc: &Document) -> Vec<Layers> {
    let count = doc.layers().len();
    let mut list = vec![Layers::Visible];
    list.extend((0..count).map(|l| Layers::Only(vec![l])));
    if count > 1 {
        list.push(Layers::Only(vec![count - 1, 0]));
        list.push(Layers::Only((0..count).collect()));
    }
    list
}

#[test]
fn every_frame_at_every_scale_and_choice_of_layers_is_the_compositors() {
    let mut checked = 0;
    for doc in corpus() {
        for layers in choices(&doc) {
            for scale in [1, 2, 3, 4] {
                let Ok(picture) = Picture::new(&doc, &layers, scale) else {
                    assert!(doc.width * scale > 8192 || doc.height * scale > 8192);
                    continue;
                };
                for frame in 0..doc.frames.len() {
                    let mut bytes = Vec::new();
                    write_frame_png(&picture, frame, None, &mut bytes).expect("written");
                    let image = png::decode(&bytes).expect("decodes");
                    assert_eq!(
                        (image.width, image.height),
                        (doc.width * scale, doc.height * scale)
                    );
                    assert!(
                        image.px == expected(&doc, &layers, frame, scale),
                        "{layers:?} {scale}"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 200, "{checked}");
}

#[test]
fn a_scaled_sheet_holds_each_scaled_frame_at_its_place_and_says_so() {
    for doc in corpus().into_iter().filter(|d| d.frames.len() > 1) {
        let layers = Layers::Only(vec![doc.layers().len() - 1]);
        let picture = Picture::new(&doc, &layers, 3).expect("drawn");
        let last = doc.frames.len() - 1;
        let plan =
            sheet_plan(&picture, 0..=last, ("s.png", "s"), &Default::default()).expect("planned");
        let mut bytes = Vec::new();
        write_sheet_png(&picture, (&plan.sheet, 0), None, &mut bytes).expect("written");
        let image = png::decode(&bytes).expect("decodes");
        let (w, h) = (doc.width * 3, doc.height * 3);
        let mut covered = vec![false; image.px.len()];
        for (frame, &(x, y)) in plan.sheet.places.iter().enumerate() {
            let want = expected(&doc, &layers, frame, 3);
            for (r, row) in want.chunks_exact(w).enumerate() {
                let at = (y + r) * image.width + x;
                assert_eq!(&image.px[at..at + w], row, "frame {frame} row {r}");
                covered[at..at + w].fill(true);
            }
        }
        let gaps = image.px.iter().zip(&covered).filter(|(_, &c)| !c);
        assert!(
            gaps.into_iter().all(|(&p, _)| p == 0),
            "space between rows is clear"
        );
        let first = format!("\"frame\":{{\"x\":0,\"y\":0,\"w\":{w},\"h\":{h}}}");
        assert!(plan.description.contains(&first), "{}", plan.description);
        assert!(plan.description.contains("\"scale\":\"3\""));
        let size = format!("\"size\":{{\"w\":{},\"h\":{}}}", image.width, image.height);
        assert!(plan.description.contains(&size));
    }
}

#[test]
fn choosing_every_visible_layer_at_scale_one_is_the_default_export_byte_for_byte() {
    for doc in corpus() {
        // What the picture shows: a layer in a hidden group is not in it.
        let picture = doc.picture_layers();
        let visible: Vec<usize> = (0..doc.layers().len()).filter(|&l| picture[l]).collect();
        if visible.is_empty() {
            continue;
        }
        let chosen = Picture::new(&doc, &Layers::Only(visible), 1).expect("drawn");
        for frame in 0..doc.frames.len() {
            let (mut a, mut b) = (Vec::new(), Vec::new());
            write_frame_png(&chosen, frame, None, &mut a).expect("written");
            write_frame_png(&Picture::visible(&doc), frame, None, &mut b).expect("written");
            assert_eq!(a, b);
        }
    }
}
