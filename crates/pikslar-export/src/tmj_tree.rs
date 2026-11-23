//! The TMJ's layer tree: tilemap layers inside the Tiled group layers that stand for Pikslar's
//! groups. A group with no tilemap layer in it, at any depth, is left out.

use crate::tmj::{TmjLayer, TmjNode};
use pikslar_core::{Document, LayerKind};

/// The layers of the map, bottom first, from the tilemap layers `built`.
pub(crate) fn tree(doc: &Document, built: Vec<TmjLayer>) -> Vec<TmjNode> {
    let mut slots: Vec<Option<TmjLayer>> = vec![None; doc.layers().len()];
    for layer in built {
        let index = layer.index;
        if let Some(slot) = slots.get_mut(index) {
            *slot = Some(layer);
        }
    }
    children(doc, &mut slots, (0, doc.layers().len()))
}

/// The nodes of the layers from `lo` up to but not including `hi`, bottom first. Groups take
/// the layers inside them.
fn children(
    doc: &Document,
    slots: &mut [Option<TmjLayer>],
    (lo, hi): (usize, usize),
) -> Vec<TmjNode> {
    let mut top_first = Vec::new();
    let mut next = hi;
    while next > lo {
        let at = next - 1;
        let layer = &doc.layers()[at];
        if layer.kind == LayerKind::Group {
            let start = doc.block(at).0.max(lo);
            let layers = children(doc, slots, (start, at));
            if !layers.is_empty() {
                top_first.push(TmjNode::Group {
                    name: layer.name.clone(),
                    visible: layer.visible,
                    layers,
                });
            }
            next = start;
        } else {
            if let Some(tiles) = slots[at].take() {
                top_first.push(TmjNode::Tiles(tiles));
            }
            next = at;
        }
    }
    top_first.reverse();
    top_first
}
