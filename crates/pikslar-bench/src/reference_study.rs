//! `pikslar-bench reference-study`: what the largest reference picture costs to place, keep and
//! show. Photographs compress badly, so the picture is noise: every tile distinct and full.
//! These stay out of the gate's baseline, which stays comparable with earlier builds.

use pikslar_core::reference::MAX_REFERENCE_SIDE;
use pikslar_core::{rgba, Cel, ColorMode, Damage, Document, Editor, Grid, LayerCmd};
use pikslar_format::png;
use pikslar_render::{flatten_document, Compositor};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

const ROUNDS: usize = 31;

fn report(name: &str, value: f64, unit: &str) {
    println!("{name:<44} {value:>14.1}  {unit}");
}

/// The median of `ROUNDS` runs of `work`, in milliseconds.
fn median_ms(mut work: impl FnMut()) -> f64 {
    let mut times: Vec<f64> = (0..ROUNDS)
        .map(|_| {
            let start = Instant::now();
            work();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[ROUNDS / 2]
}

fn noise(side: usize) -> Vec<u32> {
    let mut state = 0x9e37_79b9u32;
    (0..side * side)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state | 0xff00_0000
        })
        .collect()
}

/// A canvas of `side` with one drawn paint layer, and the same with the reference placed.
fn editors(side: usize, picture: &Arc<pikslar_core::Image>) -> (Editor, Editor) {
    let mut doc = Document::new(side, side, ColorMode::Rgba).expect("valid size");
    let ink = vec![rgba(200, 40, 40, 255); side * side];
    doc.set_cel(0, 0, Grid::from_cells(side, side, &ink))
        .expect("fits");
    let plain = Editor::new(doc.clone());
    let mut with = Editor::new(doc);
    let place = LayerCmd::PlaceReference {
        below: 0,
        image: Arc::clone(picture),
        name: "Photo".into(),
    };
    with.apply(place).expect("within the limits");
    (plain, with)
}

fn compositing(side: usize, picture: &Arc<pikslar_core::Image>) {
    let (plain, with) = editors(side, picture);
    for (name, editor) in [("without", &plain), ("with", &with)] {
        let mut comp = Compositor::new(side, side);
        let ms = median_ms(|| comp.update(editor, Damage::All, &mut Vec::new()));
        report(&format!("screen_{side}_{name}_reference_ms"), ms, "ms");
    }
    let mut comp = Compositor::new(side, side);
    let ms = median_ms(|| {
        black_box(flatten_document(with.doc(), 0, &mut comp));
    });
    report(&format!("export_{side}_with_reference_ms"), ms, "ms");
}

pub fn run() {
    let side = MAX_REFERENCE_SIDE;
    let px = noise(side);
    let encoded = png::encode(&png::Image {
        width: side,
        height: side,
        px: px.clone(),
    })
    .expect("encodes");
    report("png_file_bytes", encoded.len() as f64, "bytes");
    let ms = median_ms(|| {
        black_box(png::dimensions(&encoded).expect("a header"));
    });
    report("png_size_check_ms", ms, "ms");
    let start = Instant::now();
    let decoded = png::decode(&encoded).expect("decodes");
    report(
        "png_decode_ms",
        start.elapsed().as_secs_f64() * 1000.0,
        "ms",
    );
    report(
        "decoded_pixels_bytes",
        (decoded.px.len() * 4) as f64,
        "bytes",
    );
    let start = Instant::now();
    let cel = Cel::new(Grid::from_cells(side, side, &decoded.px));
    report(
        "tiles_from_pixels_ms",
        start.elapsed().as_secs_f64() * 1000.0,
        "ms",
    );
    report("reference_kept_bytes", cel.heap_size() as f64, "bytes");
    drop((decoded, px));
    let picture = Arc::new(cel.image().clone());
    for canvas in [64, 256, 1024] {
        compositing(canvas, &picture);
    }
}
