//! The version 2 writer, kept only so tests can make version 2 files for the reader.

use super::*;
use std::collections::HashMap;

fn write_layer<'a>(
    out: &mut Out,
    layer: &'a Layer,
    ids: &mut HashMap<usize, u32>,
    images: &mut Vec<&'a Cel>,
) {
    let name = layer.name.as_bytes();
    let name = &name[..name.len().min(u16::MAX as usize)];
    out.u16(name.len() as u16);
    out.0.extend_from_slice(name);
    out.u8(layer.visible as u8);
    out.u8(layer.opacity);
    out.u8(matches!(layer.blend, BlendMode::Multiply) as u8);
    out.u8(matches!(layer.mode, ColorMode::Rgba) as u8);
    for cel in layer.cels() {
        let id = *ids.entry(cel.image_id()).or_insert_with(|| {
            images.push(cel);
            images.len() as u32 - 1
        });
        out.u32(id);
    }
}

/// Version 2 stores every drawing at the origin, cut to the canvas. A document with a cel moved
/// or reaching past the canvas would lose pixels, so it is refused.
pub(crate) fn encode_v2(
    doc: &Document,
    sections: &[(&[u8; 4], Vec<u8>)],
) -> Result<Vec<u8>, FormatError> {
    let fits = |cel: &Cel| cel.fits_grid(doc.width, doc.height);
    if !doc.layers().iter().all(|l| l.cels().iter().all(fits)) {
        return Err(FormatError::Unsupported(
            "drawings moved off the canvas, which version 2 cannot hold",
        ));
    }
    let table = doc.link_table();
    if table.links().any(|link| table.reaches_across(link[0].id)) {
        return Err(FormatError::Unsupported(
            "drawings shared across layers, which version 2 cannot hold",
        ));
    }
    let mut out = Out(Vec::with_capacity(4096));
    out.0.extend_from_slice(MAGIC);
    out.u32(2);
    out.u32(doc.width as u32);
    out.u32(doc.height as u32);
    out.u32s(doc.colors.colors());
    out.u32s(doc.colors.swatches());
    let durations: Vec<u32> = doc.frames.iter().map(|f| f.duration_ms).collect();
    out.u32s(&durations);
    out.u32(doc.layers().len() as u32);
    let (mut ids, mut images) = (HashMap::new(), Vec::new());
    for layer in doc.layers() {
        write_layer(&mut out, layer, &mut ids, &mut images);
    }
    out.u32(images.len() as u32);
    let (tiles_wide, tiles_high) = (doc.width.div_ceil(TILE), doc.height.div_ceil(TILE));
    for image in &images {
        for ty in 0..tiles_high {
            for tx in 0..tiles_wide {
                write_tile(&mut out, image.image().tile(tx as i32, ty as i32));
            }
        }
    }
    let mut animation = Vec::new();
    if let Some(tags) = crate::piks_anim::tags_section(doc) {
        animation.push((crate::piks_anim::TAGS, tags));
    }
    if doc.layers().iter().any(|l| l.links().any(|id| id != 0)) {
        animation.push((crate::piks_anim::LINKS, encode_links(doc)));
    }
    out.u32((sections.len() + animation.len() + doc.foreign.len()) as u32);
    let every = sections
        .iter()
        .map(|(tag, bytes)| (*tag, &bytes[..]))
        .chain(animation.iter().map(|(tag, bytes)| (*tag, &bytes[..])))
        .chain(doc.foreign.iter().map(|f| (&f.tag, &f.data[..])));
    for (tag, bytes) in every {
        out.0.extend_from_slice(tag);
        out.u32(bytes.len() as u32);
        out.0.extend_from_slice(bytes);
    }
    let crc = crc32(&[&out.0]);
    out.u32(crc);
    Ok(out.0)
}

fn encode_links(doc: &Document) -> Vec<u8> {
    let mut out = Out(Vec::new());
    for layer in doc.layers() {
        // Editing can leave gaps in the numbers. A saved file always has them in order.
        let mut links: Vec<u32> = layer.links().collect();
        pikslar_core::tidy_links(&mut links);
        out.u32s(&links);
    }
    out.0
}
