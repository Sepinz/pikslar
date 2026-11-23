//! The TMJ as text, written by hand: Tiled's JSON map format, with every name escaped.

use crate::json::push_string;
use crate::tmj::{TmjLayer, TmjMap, TmjNode, TmjTileset};
use std::fmt::Write;

/// The version of Tiled's map format the text follows.
pub const FORMAT_VERSION: &str = "1.10";

fn indent(out: &mut String, depth: usize) {
    out.extend(std::iter::repeat_n("  ", depth));
}

/// `"key":` on a new line at `depth`, after a comma unless it is the object's first key.
fn key(out: &mut String, depth: usize, first: bool, name: &str) {
    out.push_str(if first { "\n" } else { ",\n" });
    indent(out, depth);
    push_string(out, name);
    out.push(':');
}

fn number(out: &mut String, depth: usize, name: &str, value: impl std::fmt::Display) {
    key(out, depth, false, name);
    let _ = write!(out, "{value}");
}

fn text(out: &mut String, depth: usize, name: &str, value: &str) {
    key(out, depth, false, name);
    push_string(out, value);
}

/// Opacity from 0 to 255 as Tiled's 0 to 1, in the fewest digits that read back as the same
/// number.
fn opacity(value: u8) -> f64 {
    f64::from(value) / 255.0
}

fn tileset(out: &mut String, set: &TmjTileset, tile: (usize, usize)) {
    indent(out, 2);
    out.push('{');
    key(out, 3, true, "columns");
    let _ = write!(out, "{}", set.layout.columns);
    number(out, 3, "firstgid", set.first_gid);
    text(out, 3, "image", &set.image);
    number(out, 3, "imageheight", set.layout.height);
    number(out, 3, "imagewidth", set.layout.width);
    number(out, 3, "margin", 0);
    text(out, 3, "name", &set.name);
    number(out, 3, "spacing", 0);
    number(out, 3, "tilecount", set.count);
    number(out, 3, "tileheight", tile.1);
    number(out, 3, "tilewidth", tile.0);
    out.push('\n');
    indent(out, 2);
    out.push('}');
}

/// The GIDs of a layer, one map row a line.
fn data(out: &mut String, depth: usize, layer: &TmjLayer, width: usize) {
    key(out, depth, true, "data");
    out.push('[');
    for (row, cells) in layer.data.chunks(width.max(1)).enumerate() {
        out.push_str(if row == 0 { "\n" } else { ",\n" });
        indent(out, depth + 1);
        for (i, gid) in cells.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(out, "{gid}");
        }
    }
    out.push('\n');
    indent(out, depth);
    out.push(']');
}

/// Writes one layer at `depth`, giving it the next layer id.
fn node(out: &mut String, depth: usize, node: &TmjNode, map: &TmjMap, id: &mut u32) {
    indent(out, depth);
    out.push('{');
    let inner = depth + 1;
    match node {
        TmjNode::Tiles(layer) => tile_layer(out, inner, layer, map, id),
        TmjNode::Group {
            name,
            visible,
            layers,
        } => {
            number_first(out, inner, "id", *id);
            *id += 1;
            key(out, inner, false, "layers");
            list(out, inner, layers, map, id);
            text(out, inner, "name", name);
            number(out, inner, "opacity", 1);
            text(out, inner, "type", "group");
            flag(out, inner, "visible", *visible);
        }
    }
    number(out, inner, "x", 0);
    number(out, inner, "y", 0);
    out.push('\n');
    indent(out, depth);
    out.push('}');
}

/// The keys of a tile layer, inside its braces, before `x` and `y`.
fn tile_layer(out: &mut String, depth: usize, layer: &TmjLayer, map: &TmjMap, id: &mut u32) {
    data(out, depth, layer, map.width);
    number(out, depth, "height", map.height);
    number(out, depth, "id", *id);
    *id += 1;
    if let Some(mode) = layer.mode {
        text(out, depth, "mode", mode);
    }
    text(out, depth, "name", &layer.name);
    if layer.offset.0 != 0 {
        number(out, depth, "offsetx", layer.offset.0);
    }
    if layer.offset.1 != 0 {
        number(out, depth, "offsety", layer.offset.1);
    }
    number(out, depth, "opacity", opacity(layer.opacity));
    text(out, depth, "type", "tilelayer");
    flag(out, depth, "visible", layer.visible);
    number(out, depth, "width", map.width);
}

fn flag(out: &mut String, depth: usize, name: &str, value: bool) {
    key(out, depth, false, name);
    out.push_str(if value { "true" } else { "false" });
}

fn number_first(out: &mut String, depth: usize, name: &str, value: impl std::fmt::Display) {
    key(out, depth, true, name);
    let _ = write!(out, "{value}");
}

/// A list of layers, each on its own lines, at `depth`.
fn list(out: &mut String, depth: usize, layers: &[TmjNode], map: &TmjMap, id: &mut u32) {
    out.push('[');
    for (i, layer) in layers.iter().enumerate() {
        out.push_str(if i == 0 { "\n" } else { ",\n" });
        node(out, depth + 1, layer, map, id);
    }
    out.push('\n');
    indent(out, depth);
    out.push(']');
}

/// How many layers the map has, groups included.
fn count(layers: &[TmjNode]) -> u32 {
    let inner = |n: &TmjNode| match n {
        TmjNode::Tiles(_) => 1,
        TmjNode::Group { layers, .. } => 1 + count(layers),
    };
    layers.iter().map(inner).sum()
}

/// The whole map as TMJ text.
#[inline(never)]
pub fn map_text(map: &TmjMap) -> String {
    let mut out = String::from("{");
    number_first(&mut out, 1, "compressionlevel", -1);
    number(&mut out, 1, "height", map.height);
    flag(&mut out, 1, "infinite", false);
    key(&mut out, 1, false, "layers");
    let mut id = 1;
    list(&mut out, 1, &map.layers, map, &mut id);
    number(&mut out, 1, "nextlayerid", count(&map.layers) + 1);
    number(&mut out, 1, "nextobjectid", 1);
    text(&mut out, 1, "orientation", "orthogonal");
    text(&mut out, 1, "renderorder", "right-down");
    number(&mut out, 1, "tileheight", map.tile.1);
    key(&mut out, 1, false, "tilesets");
    out.push('[');
    for (i, set) in map.tilesets.iter().enumerate() {
        out.push_str(if i == 0 { "\n" } else { ",\n" });
        tileset(&mut out, set, map.tile);
    }
    out.push_str(if map.tilesets.is_empty() {
        "]"
    } else {
        "\n  ]"
    });
    number(&mut out, 1, "tilewidth", map.tile.0);
    text(&mut out, 1, "type", "map");
    text(&mut out, 1, "version", FORMAT_VERSION);
    number(&mut out, 1, "width", map.width);
    out.push_str("\n}\n");
    out
}
