//! The one place JSON meets the scripting API: operations read from JSON objects, and replies
//! and errors written as JSON. Nothing else in the command line names an `Op` variant, so a
//! change to the API's operations is a change to this file alone.
//!
//! Numbers are passed to the API as they came; it checks every one. Only the JSON shape is
//! checked here: field names, types, and that nothing unknown was given. A layer id or frame
//! handle may instead refer to an earlier operation's result (`crate::results`).

use crate::fields::{Fields, Refusal};
use crate::json::Value;
use crate::results::{handle, opt_handle, Earlier, Gives, Wants};
use pikslar_api::{ApiError, DocumentInfo, LayerInfo, Mode, Op, Reply};

/// An operation object, such as `{"op":"set_pixel","layer":1,"frame":7,"x":2,"y":3,
/// "color":4278190335}`. References to earlier results are looked up in `earlier`.
pub fn op(value: &Value, earlier: &dyn Earlier) -> Result<Op, Refusal> {
    let mut f = Fields::of(value, "an operation")?;
    let name = f.text("op")?;
    let mut f = f.named(&format!("a {name} operation"));
    let e = earlier;
    let op = reading(name, &mut f, e)
        .or_else(|| layers(name, &mut f, e))
        .or_else(|| frames(name, &mut f, e))
        .or_else(|| pixels(name, &mut f, e))
        .or_else(|| blocks(name, &mut f, e))
        .or_else(|| palette(name, &mut f))
        .ok_or_else(|| Refusal::new("unknown_op", format!("there is no operation {name:?}")))??;
    f.finish()?;
    Ok(op)
}

/// What an operation gives back that a later operation in the same list can refer to.
pub fn gives(op: &Op) -> Gives {
    match op {
        Op::AddLayer { .. } | Op::DuplicateLayer { .. } => Gives::Layer,
        Op::AddFrame { .. } => Gives::Frame,
        Op::Document => Gives::Document,
        _ => Gives::Nothing,
    }
}

fn layer_of(f: &mut Fields, e: &dyn Earlier) -> Result<i64, Refusal> {
    handle(f, "layer", Wants::Layer, e)
}

fn frame_of(f: &mut Fields, key: &str, e: &dyn Earlier) -> Result<i64, Refusal> {
    handle(f, key, Wants::Frame, e)
}

fn reading(name: &str, f: &mut Fields, e: &dyn Earlier) -> Option<Result<Op, Refusal>> {
    Some(match name {
        "version" => Ok(Op::Version),
        "document" => Ok(Op::Document),
        "swatches" => Ok(Op::Swatches),
        "layer" => layer_of(f, e).map(|layer| Op::Layer { layer }),
        "pixel" => (|| {
            let (layer, frame) = (layer_of(f, e)?, frame_of(f, "frame", e)?);
            let (x, y) = (f.int("x")?, f.int("y")?);
            Ok(Op::Pixel { layer, frame, x, y })
        })(),
        _ => return None,
    })
}

fn mode(f: &mut Fields) -> Result<Mode, Refusal> {
    match f.opt_text("mode")? {
        None | Some("rgba") => Ok(Mode::Rgba),
        Some("palette") => Ok(Mode::Palette),
        Some(other) => Err(Refusal::bad(format!(
            "\"mode\" is rgba or palette, not {other:?}"
        ))),
    }
}

fn layers(name: &str, f: &mut Fields, e: &dyn Earlier) -> Option<Result<Op, Refusal>> {
    let layer = |f: &mut Fields| layer_of(f, e);
    Some(match name {
        "add_layer" => (|| {
            let above = opt_handle(f, "above", Wants::Layer, e)?;
            let mode = mode(f)?;
            let name = f.opt_text("name")?.map(String::from);
            Ok(Op::AddLayer { above, mode, name })
        })(),
        "remove_layer" => layer(f).map(|layer| Op::RemoveLayer { layer }),
        "duplicate_layer" => layer(f).map(|layer| Op::DuplicateLayer { layer }),
        "rename_layer" => (|| {
            let (layer, name) = (layer(f)?, f.text("name")?.into());
            Ok(Op::RenameLayer { layer, name })
        })(),
        "move_layer" => (|| {
            let (layer, to) = (layer(f)?, f.int("to")?);
            Ok(Op::MoveLayer { layer, to })
        })(),
        "set_opacity" => (|| {
            let (layer, opacity) = (layer(f)?, f.int("opacity")?);
            Ok(Op::SetOpacity { layer, opacity })
        })(),
        "set_visible" => (|| {
            let (layer, visible) = (layer(f)?, f.flag("visible")?);
            Ok(Op::SetVisible { layer, visible })
        })(),
        "set_blend" => (|| {
            let (layer, blend) = (layer(f)?, f.text("blend")?.into());
            Ok(Op::SetBlend { layer, blend })
        })(),
        _ => return None,
    })
}

fn frames(name: &str, f: &mut Fields, e: &dyn Earlier) -> Option<Result<Op, Refusal>> {
    Some(match name {
        "add_frame" => (|| {
            let at = f.int("at")?;
            let copy = f.opt_flag("copy")?.unwrap_or(false);
            Ok(Op::AddFrame { at, copy })
        })(),
        "remove_frame" => frame_of(f, "frame", e).map(|frame| Op::RemoveFrame { frame }),
        "set_duration" => (|| {
            let (frame, ms) = (frame_of(f, "frame", e)?, f.int("ms")?);
            Ok(Op::SetDuration { frame, ms })
        })(),
        "add_tag" => (|| {
            let name = f.text("name")?.into();
            let (from, to) = (frame_of(f, "from", e)?, frame_of(f, "to", e)?);
            Ok(Op::AddTag { name, from, to })
        })(),
        _ => return None,
    })
}

fn pixels(name: &str, f: &mut Fields, e: &dyn Earlier) -> Option<Result<Op, Refusal>> {
    if !["set_pixel", "erase_pixel", "draw_line", "fill"].contains(&name) {
        return None;
    }
    let op = |f: &mut Fields| -> Result<Op, Refusal> {
        let (layer, frame) = (layer_of(f, e)?, frame_of(f, "frame", e)?);
        if name == "draw_line" {
            let (from, to) = (f.point("from")?, f.point("to")?);
            let color = f.int("color")?;
            return Ok(Op::DrawLine {
                layer,
                frame,
                from,
                to,
                color,
            });
        }
        let (x, y) = (f.int("x")?, f.int("y")?);
        Ok(match name {
            "erase_pixel" => Op::ErasePixel { layer, frame, x, y },
            "fill" => Op::Fill {
                layer,
                frame,
                x,
                y,
                color: f.int("color")?,
            },
            _ => Op::SetPixel {
                layer,
                frame,
                x,
                y,
                color: f.int("color")?,
            },
        })
    };
    Some(op(f))
}

/// Blocks of pixels: `put_pixels` with a `colors` array, `fill_rect`, `pixels` and `draw_text`.
fn blocks(name: &str, f: &mut Fields, e: &dyn Earlier) -> Option<Result<Op, Refusal>> {
    if !["put_pixels", "fill_rect", "pixels", "draw_text"].contains(&name) {
        return None;
    }
    let op = |f: &mut Fields| -> Result<Op, Refusal> {
        let (layer, frame) = (layer_of(f, e)?, frame_of(f, "frame", e)?);
        if name == "draw_text" {
            let (x, y) = (f.int("x")?, f.int("y")?);
            let text = f.text("text")?.to_string();
            let (color, scale) = (f.int("color")?, f.int("scale")?);
            return Ok(Op::DrawText {
                layer,
                frame,
                x,
                y,
                text,
                color,
                scale,
            });
        }
        let (x, y, width) = (f.int("x")?, f.int("y")?, f.int("width")?);
        if name == "put_pixels" {
            let colors = f.list("colors")?.iter().map(|v| match v {
                Value::Int(c) => Ok(*c),
                _ => Err(Refusal::bad("\"colors\" must hold whole numbers only")),
            });
            let colors = colors.collect::<Result<Vec<_>, _>>()?;
            return Ok(Op::PutPixels {
                layer,
                frame,
                x,
                y,
                width,
                colors,
            });
        }
        let height = f.int("height")?;
        Ok(match name {
            "pixels" => Op::Pixels {
                layer,
                frame,
                x,
                y,
                width,
                height,
            },
            _ => Op::FillRect {
                layer,
                frame,
                x,
                y,
                width,
                height,
                color: f.int("color")?,
            },
        })
    };
    Some(op(f))
}

fn palette(name: &str, f: &mut Fields) -> Option<Result<Op, Refusal>> {
    Some(match name {
        "add_swatch" => f.int("color").map(|color| Op::AddSwatch { color }),
        "recolor" => (|| {
            let (swatch, color) = (f.int("swatch")?, f.int("color")?);
            Ok(Op::Recolor { swatch, color })
        })(),
        "remove_swatch" => f.int("swatch").map(|swatch| Op::RemoveSwatch { swatch }),
        _ => return None,
    })
}

fn int(n: impl Into<i64>) -> Value {
    Value::Int(n.into())
}

/// A count or place: these are far below `i64::MAX` in any document that can be opened.
fn size(n: usize) -> Value {
    Value::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

fn object(members: Vec<(&str, Value)>) -> Value {
    Value::Object(members.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Rgba => "rgba",
        Mode::Palette => "palette",
    }
}

/// An operation's reply: `null` for done, else an object.
pub fn reply(reply: &Reply) -> Value {
    match reply {
        Reply::Done => Value::Null,
        Reply::Version(v) => object(vec![("version", int(*v))]),
        Reply::Document(doc) => document(doc),
        Reply::Layer(layer) => layer_info(layer),
        Reply::Color(c) => object(vec![("color", int(*c))]),
        Reply::Swatches(list) => {
            let items = list
                .iter()
                .map(|&(id, color)| object(vec![("id", int(id)), ("color", int(color))]))
                .collect();
            object(vec![("swatches", Value::Array(items))])
        }
        Reply::Created(id) => object(vec![("id", int(*id))]),
        Reply::Frame(handle) => object(vec![("frame", handle_value(*handle))]),
        Reply::Pixels(colors) => {
            let items = colors.iter().map(|&c| int(c)).collect();
            object(vec![("pixels", Value::Array(items))])
        }
    }
}

/// A frame handle. Handles count up from 1 while the program runs, so they stay far below
/// `i64::MAX`, which JSON integers here can hold.
fn handle_value(handle: u64) -> Value {
    Value::Int(i64::try_from(handle).unwrap_or(i64::MAX))
}

/// What a document is, in outline, as the `document` operation gives it.
pub fn document(doc: &DocumentInfo) -> Value {
    let durations = doc.durations.iter().map(|&d| int(d)).collect();
    let frames = doc.frames.iter().map(|&h| handle_value(h)).collect();
    let layers = doc.layers.iter().map(|&id| int(id)).collect();
    object(vec![
        ("width", size(doc.width)),
        ("height", size(doc.height)),
        ("durations", Value::Array(durations)),
        ("frames", Value::Array(frames)),
        ("layers", Value::Array(layers)),
    ])
}

fn layer_info(layer: &LayerInfo) -> Value {
    object(vec![
        ("id", int(layer.id)),
        ("index", size(layer.index)),
        ("name", Value::Str(layer.name.clone())),
        ("kind", Value::Str(layer.kind.into())),
        ("mode", Value::Str(mode_name(layer.mode).into())),
        ("visible", Value::Bool(layer.visible)),
        ("locked", Value::Bool(layer.locked)),
        ("opacity", int(layer.opacity)),
        ("blend", Value::Str(layer.blend.into())),
    ])
}

/// An API error as a refusal, with a code a tool can match and the API's own words.
pub fn api_error(error: &ApiError) -> Refusal {
    let code = match error {
        ApiError::Version { .. } => "version",
        ApiError::NoSuchLayer(_) => "no_such_layer",
        ApiError::NoSuchFrame(_) => "no_such_frame",
        ApiError::NoSuchSwatch(_) => "no_such_swatch",
        ApiError::OutOfRange { .. } => "out_of_range",
        ApiError::Outside { .. } => "outside",
        ApiError::Transparent(_) => "transparent",
        ApiError::BadName { .. } => "bad_name",
        ApiError::UnknownBlend(_) => "unknown_blend",
        ApiError::Refused(_) => "refused",
        ApiError::Cancelled => "cancelled",
    };
    Refusal::new(code, error.to_string())
}

#[cfg(test)]
#[path = "api_json_tests.rs"]
pub(crate) mod tests;
