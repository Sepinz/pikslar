//! `info`: what a project holds, for a person or, with --json, for a script.

use pikslar_core::{BlendMode, Blending, ColorMode, Document, LayerKind, SliceRect};
use pikslar_export::{direction_name, json};
use std::fmt::Write;

/// A name as text a terminal shows rather than obeys: control characters are written as escapes.
fn printable(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_control() {
                c.escape_debug().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

fn mode_name(mode: ColorMode) -> &'static str {
    match mode {
        ColorMode::Palette => "palette",
        ColorMode::Rgba => "rgba",
    }
}

fn blend_name(blend: BlendMode) -> &'static str {
    match blend {
        BlendMode::Normal => "normal",
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
        BlendMode::Darken => "darken",
        BlendMode::Lighten => "lighten",
        BlendMode::Addition => "addition",
        BlendMode::Subtract => "subtract",
        BlendMode::Difference => "difference",
        BlendMode::ColorDodge => "color_dodge",
        BlendMode::ColorBurn => "color_burn",
        BlendMode::HardLight => "hard_light",
        BlendMode::SoftLight => "soft_light",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Color => "color",
        BlendMode::Luminosity => "luminosity",
        BlendMode::Divide => "divide",
    }
}

fn blending_name(blending: Blending) -> &'static str {
    match blending {
        Blending::Pikslar => "pikslar",
        Blending::Aseprite => "aseprite",
    }
}

/// The whole running time in milliseconds. By frame rate it is the exact total of the ticks,
/// rounded once, as every export shows it, not the sum of frames each rounded alone.
fn total_ms(doc: &Document) -> u64 {
    match doc.frame_rate {
        Some(fps) => {
            let ticks: u64 = (0..doc.frames.len())
                .map(|f| u64::from(doc.frame_units(f)))
                .sum();
            let fps = u64::from(fps).max(1);
            (ticks * 2000 + fps) / (2 * fps)
        }
        None => doc.frames.iter().map(|f| u64::from(f.duration_ms)).sum(),
    }
}

/// Frames are counted from 1, as `--frame` takes them.
pub fn text(doc: &Document) -> String {
    let total = total_ms(doc);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} x {}, {} layers, {} frames, {}.{:03} s{}{}",
        doc.width,
        doc.height,
        doc.layers().len(),
        doc.frames.len(),
        total / 1000,
        total % 1000,
        doc.frame_rate
            .map_or_else(String::new, |fps| format!(", {fps} fps")),
        // Only a project that is not drawn the usual way says how it is drawn.
        match doc.blending {
            Blending::Pikslar => "",
            Blending::Aseprite => ", Aseprite-compatible blending",
        }
    );
    out.push_str("Layers, top first:\n");
    for layer in doc.layers().iter().rev() {
        let hidden = if layer.visible { "" } else { ", hidden" };
        let kind = match layer.kind {
            LayerKind::Reference => ", reference, not exported",
            LayerKind::Group => ", group",
            LayerKind::Tilemap(_) => ", tilemap",
            LayerKind::Paint => "",
        };
        // Each group a layer is in indents it two more places.
        let indent = "  ".repeat(usize::from(layer.depth) + 1);
        let _ = writeln!(
            out,
            "{indent}{}: {}, {}, opacity {}{hidden}{kind}",
            printable(&layer.name),
            mode_name(layer.mode),
            blend_name(layer.blend),
            layer.opacity
        );
    }
    push_tag_lines(&mut out, doc);
    push_slice_lines(&mut out, doc);
    out
}

/// The tags, one a line, frames counted from 1.
fn push_tag_lines(out: &mut String, doc: &Document) {
    out.push_str("Tags:");
    if doc.tags.is_empty() {
        out.push_str(" none");
    }
    out.push('\n');
    for tag in &doc.tags {
        let (from, to, way) = (tag.from + 1, tag.to + 1, direction_name(tag.direction));
        let plays = match tag.repeat {
            0 => String::new(),
            n => format!(", plays {n} times"),
        };
        let _ = writeln!(
            out,
            "  {}: frames {from} to {to}, {way}{plays}",
            printable(&tag.name)
        );
    }
}

fn rect_text(r: SliceRect) -> String {
    format!("{}, {}, {} x {}", r.x, r.y, r.width, r.height)
}

/// A count, then a line for each slice: where it is, and its pivot and center if it has them.
fn push_slice_lines(out: &mut String, doc: &Document) {
    if doc.slices.is_empty() {
        return;
    }
    let _ = writeln!(out, "Slices: {}", doc.slices.len());
    for slice in &doc.slices {
        let _ = write!(
            out,
            "  {}: at {}",
            printable(&slice.name),
            rect_text(slice.bounds)
        );
        if let Some((x, y)) = slice.pivot {
            let _ = write!(out, ", pivot {x}, {y}");
        }
        if let Some(center) = slice.center {
            let _ = write!(out, ", center {}", rect_text(center));
        }
        out.push('\n');
    }
}

fn push_rect(out: &mut String, key: &str, r: SliceRect) {
    let _ = write!(
        out,
        ",\"{key}\":{{\"x\":{},\"y\":{},\"w\":{},\"h\":{}}}",
        r.x, r.y, r.width, r.height
    );
}

fn push_slices(out: &mut String, doc: &Document) {
    for (index, slice) in doc.slices.iter().enumerate() {
        out.push_str(if index > 0 {
            ",{\"name\":"
        } else {
            "{\"name\":"
        });
        json::push_string(out, &slice.name);
        let _ = write!(out, ",\"color\":\"#{:08x}\"", slice.color);
        push_rect(out, "bounds", slice.bounds);
        if let Some((x, y)) = slice.pivot {
            let _ = write!(out, ",\"pivot\":{{\"x\":{x},\"y\":{y}}}");
        }
        if let Some(center) = slice.center {
            push_rect(out, "center", center);
        }
        out.push('}');
    }
}

fn push_layers(out: &mut String, doc: &Document) {
    for (index, layer) in doc.layers().iter().enumerate() {
        out.push_str(if index > 0 {
            ",{\"name\":"
        } else {
            "{\"name\":"
        });
        json::push_string(out, &layer.name);
        let _ = write!(
            out,
            ",\"visible\":{},\"opacity\":{},\"blend\":\"{}\",\"mode\":\"{}\",\"kind\":\"{}\",\"depth\":{}}}",
            layer.visible,
            layer.opacity,
            blend_name(layer.blend),
            mode_name(layer.mode),
            match layer.kind {
                LayerKind::Reference => "reference",
                LayerKind::Group => "group",
                LayerKind::Tilemap(_) => "tilemap",
                LayerKind::Paint => "paint",
            },
            layer.depth
        );
    }
}

fn push_tags(out: &mut String, doc: &Document) {
    for (index, tag) in doc.tags.iter().enumerate() {
        out.push_str(if index > 0 {
            ",{\"name\":"
        } else {
            "{\"name\":"
        });
        json::push_string(out, &tag.name);
        let _ = write!(
            out,
            ",\"first\":{},\"last\":{},\"direction\":\"{}\",\"repeat\":{}}}",
            tag.from + 1,
            tag.to + 1,
            direction_name(tag.direction),
            tag.repeat
        );
    }
}

/// Layers bottom first, as they are stacked. Tag frames are counted from 1.
pub fn json(doc: &Document) -> String {
    let durations: Vec<String> = doc
        .frames
        .iter()
        .map(|f| f.duration_ms.to_string())
        .collect();
    let mut out = format!(
        "{{\"width\":{},\"height\":{},\"fps\":{},\"blending\":\"{}\",\"durations\":[{}],\"layers\":[",
        doc.width,
        doc.height,
        doc.frame_rate
            .map_or_else(|| "null".to_string(), |fps| fps.to_string()),
        blending_name(doc.blending),
        durations.join(",")
    );
    push_layers(&mut out, doc);
    out.push_str("],\"tags\":[");
    push_tags(&mut out, doc);
    out.push_str("],\"slices\":[");
    push_slices(&mut out, doc);
    out.push_str("]}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::printable;

    #[test]
    fn control_characters_in_names_are_shown_not_obeyed() {
        assert_eq!(printable("Shade é"), "Shade é");
        assert_eq!(printable("a\u{1b}[2Jb\n"), "a\\u{1b}[2Jb\\n");
    }
}
