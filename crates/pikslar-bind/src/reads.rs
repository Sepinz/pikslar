//! Host functions that read the document (`ReadDocument`), and `print`.
//!
//! Reads of the document's shape leave waiting pixels waiting, since no pixel changes it;
//! reads of pixels and swatches write them first.

use crate::binding::Binding;
use crate::cost;
use crate::values::{int, Args};
use pikslar_api::{ApiError, DocumentInfo, LayerInfo, Op, Reply};
use pikslar_core::glyphs::{self, Font};
use pikslar_script::{HostCx, HostError, Value};

type Out = Result<Value, HostError>;

fn unexpected() -> HostError {
    HostError::new("the API gave an unexpected reply")
}

fn document(
    b: &mut Binding<'_, '_>,
    name: &str,
    cx: &mut HostCx<'_, '_>,
) -> Result<DocumentInfo, HostError> {
    let (_, _, layers, frames) = b.size();
    cx.charge(cost::READ + (layers + frames) as u64)?;
    match b.op_now(name, Op::Document)? {
        Reply::Document(doc) => Ok(doc),
        _ => Err(unexpected()),
    }
}

fn layer(
    b: &mut Binding<'_, '_>,
    a: &Args<'_>,
    cx: &mut HostCx<'_, '_>,
) -> Result<LayerInfo, HostError> {
    cx.charge(cost::layer_read(b.size()))?;
    let layer = a.int(0)?;
    match b.op_now(a.name, Op::Layer { layer })? {
        Reply::Layer(info) => Ok(info),
        _ => Err(unexpected()),
    }
}

pub fn print(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let text = crate::values::render(a.values.first().unwrap_or(&Value::Nil));
    cx.charge(1 + cost::pixels(text.len() as u64))?;
    b.print(&text);
    b.print("\n");
    Ok(Value::Nil)
}

pub fn api_version(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    cx.charge(cost::READ)?;
    match b.op_now(a.name, Op::Version)? {
        Reply::Version(v) => Ok(int(v)),
        _ => Err(unexpected()),
    }
}

pub fn width(b: &mut Binding<'_, '_>, _: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    cx.charge(cost::READ)?;
    Ok(int(b.size().0))
}

pub fn height(b: &mut Binding<'_, '_>, _: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    cx.charge(cost::READ)?;
    Ok(int(b.size().1))
}

pub fn layers(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let doc = document(b, a.name, cx)?;
    cx.new_int_list(&doc.layers)
}

pub fn frames(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let doc = document(b, a.name, cx)?;
    let handles: Vec<Value> = doc.frames.iter().map(|&h| int(h)).collect();
    cx.new_list(&handles)
}

pub fn layer_name(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let info = layer(b, a, cx)?;
    cx.new_str(&info.name)
}

pub fn layer_kind(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let info = layer(b, a, cx)?;
    cx.new_str(info.kind)
}

pub fn layer_index(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    Ok(int(layer(b, a, cx)?.index))
}

pub fn layer_visible(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    Ok(Value::Bool(layer(b, a, cx)?.visible))
}

pub fn layer_locked(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    Ok(Value::Bool(layer(b, a, cx)?.locked))
}

pub fn layer_opacity(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    Ok(int(layer(b, a, cx)?.opacity))
}

pub fn layer_blend(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let info = layer(b, a, cx)?;
    cx.new_str(info.blend)
}

pub fn frame_duration(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let frame = a.int(0)?;
    let doc = document(b, a.name, cx)?;
    let at = doc
        .frames
        .iter()
        .position(|&h| i64::try_from(h) == Ok(frame));
    match at.and_then(|i| doc.durations.get(i)) {
        Some(&ms) => Ok(int(ms)),
        None => Err(b.refuse(a.name, ApiError::NoSuchFrame(frame))),
    }
}

pub fn get_pixel(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    cx.charge(cost::cel_read(b.size()))?;
    let (layer, frame, x, y) = (a.int(0)?, a.int(1)?, a.int(2)?, a.int(3)?);
    match b.op(a.name, Op::Pixel { layer, frame, x, y })? {
        Reply::Color(c) => Ok(int(c)),
        _ => Err(unexpected()),
    }
}

pub fn read_rect(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (layer, frame, x, y) = (a.int(0)?, a.int(1)?, a.int(2)?, a.int(3)?);
    let (width, height) = (a.int(4)?, a.int(5)?);
    let area = width.saturating_mul(height).max(0) as u64;
    // Refused before any work when the list could never fit the run's memory.
    if !cx.list_fits(usize::try_from(area).unwrap_or(usize::MAX)) {
        return Err(HostError::new(format!(
            "{}: a {width} by {height} block is more than the script's memory holds",
            a.name
        )));
    }
    // The API's read (4 bytes a pixel) is charged to the script's memory until the list is
    // made from it, so the two together stay within the limit.
    let pixels = usize::try_from(area).unwrap_or(usize::MAX);
    let read_copy = cx.hold(pixels.saturating_mul(std::mem::size_of::<u32>()))?;
    cx.charge(cost::edit(b.size()).saturating_add(area))?;
    let read = Op::Pixels {
        layer,
        frame,
        x,
        y,
        width,
        height,
    };
    let list = match b.op(a.name, read)? {
        Reply::Pixels(colors) => cx.new_int_list(&colors),
        _ => Err(unexpected()),
    };
    drop(read_copy);
    list
}

fn swatch_list(
    b: &mut Binding<'_, '_>,
    name: &str,
    cx: &mut HostCx<'_, '_>,
) -> Result<Vec<(u32, u32)>, HostError> {
    // Paid before the list is made: one unit a swatch it will hold.
    cx.charge(cost::READ.saturating_add(b.swatch_count() as u64))?;
    match b.op(name, Op::Swatches)? {
        Reply::Swatches(list) => Ok(list),
        _ => Err(unexpected()),
    }
}

pub fn swatches(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let ids: Vec<u32> = swatch_list(b, a.name, cx)?.iter().map(|s| s.0).collect();
    cx.new_int_list(&ids)
}

pub fn swatch_color(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let id = a.int(0)?;
    let list = swatch_list(b, a.name, cx)?;
    match list.iter().find(|s| i64::from(s.0) == id) {
        Some(&(_, color)) => Ok(int(color)),
        None => Err(b.refuse(a.name, ApiError::NoSuchSwatch(id))),
    }
}

/// The width in pixels of the widest line of built-in 5x7 text at a scale, as `draw_text`
/// draws it. Pure, so it reads nothing of the document; it is paid for by its length.
pub fn text_width(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (text, scale) = (a.text(0)?, a.int(1)?);
    let chars = text.chars().count();
    cx.charge(cost::READ.saturating_add(cost::pixels(chars as u64)))?;
    let refusal = |what, value, (min, max): (i64, i64)| ApiError::OutOfRange {
        what,
        value,
        min,
        max,
    };
    let most = i64::from(glyphs::MAX_SCALE);
    if !(1..=most).contains(&scale) {
        return Err(b.refuse(a.name, refusal("the text scale", scale, (1, most))));
    }
    let longest = glyphs::MAX_CHARS as i64;
    if chars as i64 > longest {
        let value = i64::try_from(chars).unwrap_or(i64::MAX);
        let range = (0, longest);
        return Err(b.refuse(a.name, refusal("the length of the text", value, range)));
    }
    let scale = i32::try_from(scale).unwrap_or(1);
    Ok(int(glyphs::text_width(Font::Pikslar5x7, &text, scale, 1)))
}
