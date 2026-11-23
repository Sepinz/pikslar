//! Links on the largest document the format allows stay quick to ask about: what the timeline
//! and the mode button ask once per repaint or per click must not read every cel per cel.

use super::*;
use crate::command::LayerCmd;
use crate::document::{ColorMode, FrameInfo, MAX_FRAMES, MAX_LAYERS};
use crate::editor::Editor;
use crate::layer::Cel;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Cells of the matrix a large window shows at once.
const CELLS_IN_VIEW: usize = 600;

/// Generous for a debug build, which is many times slower; tight enough in release to catch a
/// pass over every cel per cel. A command in a debug build also checks every rule of the links
/// once, sorting every linked cel, so it gets more.
fn bound(command: bool) -> Duration {
    match (cfg!(debug_assertions), command) {
        (true, false) => Duration::from_millis(500),
        (true, true) => Duration::from_millis(2_500),
        (false, _) => Duration::from_millis(20),
    }
}

/// 256 layers of 4,096 frames, every layer holding one drawing on every frame.
fn largest_held() -> Document {
    let cels = vec![Cel::blank(); MAX_FRAMES];
    let layers = (0..MAX_LAYERS)
        .map(|_| {
            let layer = Layer::new("Layer".into(), ColorMode::Rgba).with_cels(cels.clone());
            layer
                .with_holds(&[1; MAX_FRAMES])
                .expect("one id per frame")
        })
        .collect();
    let frames = vec![FrameInfo::new(100); MAX_FRAMES];
    let colors = Arc::new(crate::color::ColorTable::default());
    Document::from_parts(8, 8, colors, layers, frames).expect("within limits")
}

/// The same with one link reaching every cel of every layer.
fn largest_linked() -> Document {
    let mut doc = largest_held();
    let ids = vec![vec![7; MAX_FRAMES]; MAX_LAYERS];
    assert_eq!(doc.set_links(&ids), Ok(0));
    doc
}

/// What the matrix asks while drawing the cells in view: which are shared with another layer.
fn notches(doc: &Document) -> usize {
    let across = doc.links_reaching_across();
    let cells = (0..CELLS_IN_VIEW).map(|i| (i % MAX_LAYERS, i / MAX_LAYERS));
    cells
        .filter(|&(layer, frame)| across.contains(doc.layers[layer].link(frame)))
        .count()
}

fn timed<R>(what: &str, run: impl FnOnce() -> R) -> R {
    timed_within(bound(false), what, run)
}

fn timed_within<R>(bound: Duration, what: &str, run: impl FnOnce() -> R) -> R {
    let start = Instant::now();
    let result = run();
    let took = start.elapsed();
    assert!(took < bound, "{what} took {took:?}");
    result
}

#[test]
fn marking_shared_cels_reads_the_document_once_per_repaint() {
    let held = largest_held();
    assert_eq!(timed("notches of holds", || notches(&held)), 0);
    let linked = largest_linked();
    assert_eq!(
        timed("notches of one link", || notches(&linked)),
        CELLS_IN_VIEW
    );
}

#[test]
fn asking_whether_converting_ends_links_reads_the_document_once() {
    let held = largest_held();
    assert!(!timed("holds", || held.shares_with_other_layers(10)));
    let linked = largest_linked();
    assert!(timed("one link", || linked.shares_with_other_layers(10)));
}

#[test]
fn converting_a_layer_of_the_largest_document_is_quick() {
    let mut e = Editor::new(largest_linked());
    let layer = e.layer_id(10).expect("layer");
    let convert = LayerCmd::SetMode {
        layer,
        mode: ColorMode::Palette,
    };
    timed_within(bound(true), "converting", || e.apply(convert)).expect("converted");
    assert!(!e.doc.shares_with_other_layers(10));
    assert!(e.doc.shares_with_other_layers(11));
    assert!(e.doc.is_held(10, MAX_FRAMES - 1), "its own hold stays");
}
