//! What the `Nids` section checks its ids against: one print per frame and one check per
//! layer. Neither is a security measure, only a way to notice that a build which carried the
//! section along without knowing it changed the frames or the layers.
//!
//! Every input is something a file stores exactly and every build reads back as stored, so a
//! build that only opens and saves leaves both alone.

use pikslar_core::{ColorMode, Document, Image, Layer, LayerKind, Tile};
use std::collections::HashMap;

/// A word mixer: not for security, only to notice that frames changed.
fn mix(hash: u64, value: u64) -> u64 {
    (hash.rotate_left(5) ^ value).wrapping_mul(0x517c_c1b7_2722_0a95)
}

/// Spreads every bit of `value` over the result, so sums of finished values stay distinct.
fn finish(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn tile_hash(hash: u64, tile: &Tile) -> u64 {
    match tile {
        Tile::Solid(v) => mix(mix(hash, 1), u64::from(*v)),
        Tile::Packed {
            palette,
            bits,
            data,
        } => {
            let mut h = mix(mix(hash, 2), u64::from(*bits));
            h = palette.iter().fold(h, |h, &c| mix(h, u64::from(c)));
            data.chunks(8).fold(h, |h, chunk| {
                let mut word = [0u8; 8];
                word[..chunk.len()].copy_from_slice(chunk);
                mix(h, u64::from_le_bytes(word))
            })
        }
        Tile::Raw(raw) => raw.iter().fold(mix(hash, 3), |h, &c| mix(h, u64::from(c))),
    }
}

/// What an image holds: each tile that draws something, where it is and as it is saved.
/// Tiles that draw nothing, and how far the table reaches past the drawn ones, are left out,
/// since opening a file drops and trims them.
fn image_hash(image: &Image) -> u64 {
    let (x0, y0, tw, th) = image.table();
    let mut hash = 0;
    for ty in y0..y0 + th as i32 {
        for tx in x0..x0 + tw as i32 {
            let Some(tile) = image.tile(tx, ty).filter(|t| !t.draws_nothing()) else {
                continue;
            };
            let at = u64::from(tx as u16) << 16 | u64::from(ty as u16);
            hash = tile_hash(mix(hash, at), tile);
        }
    }
    finish(hash)
}

/// Image hashes by image, so a drawing many cels share is hashed once.
#[derive(Default)]
struct Images(HashMap<usize, u64>);

impl Images {
    /// A cel that is not blank: its drawing and where it is. `None` for a blank cel.
    fn cel(&mut self, layer: &Layer, place: usize) -> Option<u64> {
        let cel = layer.cels().get(place).filter(|c| !c.is_blank())?;
        let image = *self
            .0
            .entry(cel.image_id())
            .or_insert_with(|| image_hash(cel.image()));
        let (x, y) = cel.position();
        let at = u64::from(x as u16) << 16 | u64::from(y as u16);
        Some(finish(mix(image, at)))
    }
}

/// One print per frame, in timeline order: its duration and its cels that are not blank,
/// summed so it does not matter which layer holds them. So adding, removing or moving an empty
/// layer, or changing how a layer shows, leaves every print alone, and drawing on a frame,
/// retiming it or moving a cel in it changes only its own. Frames that look and play exactly
/// alike have the same print.
pub(crate) fn frame_prints(doc: &Document) -> Vec<u64> {
    let mut images = Images::default();
    let prints = (0..doc.frames.len()).map(|place| {
        let cels = doc.layers().iter().filter_map(|l| images.cel(l, place));
        let sum = cels.fold(0u64, u64::wrapping_add);
        let duration = u64::from(doc.frames[place].duration_ms);
        finish(mix(mix(0x9e37_79b9_7f4a_7c15, duration), sum))
    });
    prints.collect()
}

fn kind_words(kind: LayerKind) -> (u64, u64) {
    match kind {
        LayerKind::Paint => (1, 0),
        LayerKind::Reference => (2, 0),
        LayerKind::Group => (3, 0),
        LayerKind::Tilemap(set) => (4, u64::from(set.get())),
    }
}

/// Marks a group's check as of the scheme that covers its contents. A check written before it
/// (a group's was only its kind and colour mode) is never equal to one made with it, so a file
/// of the older scheme has its groups' ids renewed, with the usual note, never matched.
const GROUP_SCHEME: u64 = 0x6772_6f75_7032;

/// What one layer's check is made of, besides the drawings it shows.
fn head(layer: &Layer) -> u64 {
    let (kind, set) = kind_words(layer.kind);
    let mode = match layer.mode {
        ColorMode::Palette => 1,
        ColorMode::Rgba => 2,
    };
    mix(mix(mix(0x2545_f491_4f6c_dd1d, kind), set), mode)
}

/// A group's check: its head, then how many layers it holds and each one's id and own check,
/// from the bottom. So a group an older build emptied, filled or gave other layers is not the
/// group that was saved, wherever it was given its id.
fn group_check(doc: &Document, index: usize, layer: &Layer, checks: &[u64]) -> u64 {
    let (first, _) = doc.block(index);
    let inside = doc.layers().get(first..index).unwrap_or_default();
    let head = mix(mix(head(layer), GROUP_SCHEME), inside.len() as u64);
    let held = inside
        .iter()
        .zip(checks.get(first..index).unwrap_or_default());
    finish(held.fold(head, |h, (held, &check)| {
        mix(mix(h, u64::from(held.persisted_id().get())), check)
    }))
}

/// One check per layer, from the bottom: its kind, colour mode and tileset, and the distinct
/// drawings it shows with where they are; for a group, what it holds instead (see
/// `group_check`). Not its name, settings or place, nor how many frames show a drawing or in
/// what order, so renaming, hiding, moving or retiming, and adding, copying or reordering
/// frames, leave it alone; drawing on the layer, or removing the last frame that shows one of
/// its drawings, changes it, as does putting another layer in its place.
pub(crate) fn layer_checks(doc: &Document) -> Vec<u64> {
    let mut images = Images::default();
    let mut checks: Vec<u64> = Vec::with_capacity(doc.layers().len());
    for (index, layer) in doc.layers().iter().enumerate() {
        let check = match layer.kind {
            LayerKind::Group => group_check(doc, index, layer, &checks),
            _ => {
                let mut drawings: Vec<u64> = (0..layer.frame_count())
                    .filter_map(|place| images.cel(layer, place))
                    .collect();
                drawings.sort_unstable();
                drawings.dedup();
                finish(drawings.into_iter().fold(head(layer), mix))
            }
        };
        checks.push(check);
    }
    checks
}

/// What a group's check was before it covered its contents, for tests of files of that scheme.
#[cfg(test)]
pub(crate) fn group_check_before_contents(mode: ColorMode) -> u64 {
    let mode = match mode {
        ColorMode::Palette => 1,
        ColorMode::Rgba => 2,
    };
    finish(mix(mix(mix(0x2545_f491_4f6c_dd1d, 3), 0), mode))
}
