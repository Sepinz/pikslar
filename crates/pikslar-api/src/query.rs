//! Reading the document. Nothing here changes it.

use crate::{input, ApiError, DocumentInfo, LayerInfo, LayerRef, Mode, Reply};
use pikslar_core::{ColorMode, Document, Layer, LayerKind};

pub fn document(doc: &Document) -> Reply {
    Reply::Document(DocumentInfo {
        width: doc.width,
        height: doc.height,
        durations: doc.frames.iter().map(|f| f.duration_ms).collect(),
        frames: doc.frames.iter().map(|f| f.key().get()).collect(),
        layers: doc
            .layers()
            .iter()
            .map(|l| l.persisted_id().get())
            .collect(),
    })
}

fn kind(layer: &Layer) -> &'static str {
    match layer.kind {
        LayerKind::Paint => "paint",
        LayerKind::Reference => "reference",
        LayerKind::Group => "group",
        LayerKind::Tilemap(_) => "tilemap",
    }
}

pub fn mode(mode: ColorMode) -> Mode {
    match mode {
        ColorMode::Rgba => Mode::Rgba,
        ColorMode::Palette => Mode::Palette,
    }
}

pub fn layer(doc: &Document, layer: LayerRef) -> Result<Reply, ApiError> {
    let index = input::layer_index(doc, layer)?;
    let l = &doc.layers()[index];
    Ok(Reply::Layer(LayerInfo {
        id: l.persisted_id().get(),
        index,
        name: l.name.clone(),
        kind: kind(l),
        mode: mode(l.mode),
        visible: l.visible,
        locked: l.locked,
        opacity: l.opacity,
        blend: l.blend.name(),
    }))
}

pub fn swatches(doc: &Document) -> Reply {
    let colors = &doc.colors;
    let list = colors.swatches().iter();
    Reply::Swatches(list.map(|&id| (id, colors.color(id))).collect())
}
