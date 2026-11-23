//! Checks at the boundary: every number, name and id a script gives, before any command runs.

use crate::{ApiError, FrameRef, LayerRef, SwatchRef};
use pikslar_core::{BlendMode, Document, LayerId, MAX_DURATION_MS, MIN_DURATION_MS};

/// A number from `min` to `max`, both included.
pub fn ranged(what: &'static str, value: i64, min: i64, max: i64) -> Result<i64, ApiError> {
    match (min..=max).contains(&value) {
        true => Ok(value),
        false => Err(ApiError::OutOfRange {
            what,
            value,
            min,
            max,
        }),
    }
}

/// A number from `min` to `max` in a narrower type that holds that range.
fn narrow<T: TryFrom<i64>>(
    what: &'static str,
    value: i64,
    min: i64,
    max: i64,
) -> Result<T, ApiError> {
    let refused = ApiError::OutOfRange {
        what,
        value,
        min,
        max,
    };
    T::try_from(ranged(what, value, min, max)?).map_err(|_| refused)
}

/// A place in a list of `len`, or one past it when `end` is allowed.
pub fn place(what: &'static str, value: i64, len: usize, end: bool) -> Result<usize, ApiError> {
    let last = i64::try_from(len).unwrap_or(i64::MAX) - i64::from(!end);
    let at = ranged(what, value, 0, last)?;
    usize::try_from(at).map_err(|_| ApiError::OutOfRange {
        what,
        value,
        min: 0,
        max: last,
    })
}

pub fn opacity(value: i64) -> Result<u8, ApiError> {
    narrow("opacity", value, 0, 255)
}

pub fn duration(value: i64) -> Result<u32, ApiError> {
    let (min, max) = (i64::from(MIN_DURATION_MS), i64::from(MAX_DURATION_MS));
    narrow("a duration in milliseconds", value, min, max)
}

/// A straight-alpha color, `0xAABBGGRR`, where a transparent one is 0.
pub fn color(value: i64) -> Result<u32, ApiError> {
    let color: u32 = narrow("a color", value, 0, i64::from(u32::MAX))?;
    match color != 0 && color >> 24 == 0 {
        true => Err(ApiError::Transparent(value)),
        false => Ok(color),
    }
}

/// A name of at most `most` characters, not blank and with no control characters.
pub fn name(text: &str, most: usize) -> Result<String, ApiError> {
    let why = if text.trim().is_empty() {
        Some("is empty")
    } else if text.chars().count() > most {
        Some(match most {
            32 => "is longer than 32 characters",
            _ => "is longer than 64 characters",
        })
    } else if text.chars().any(char::is_control) {
        Some("holds a control character")
    } else {
        None
    };
    match why {
        Some(why) => Err(ApiError::BadName {
            name: quoted(text),
            why,
        }),
        None => Ok(text.into()),
    }
}

/// The place of the layer with this persisted id. Refused unless exactly one layer has it,
/// as for frames: files are read with distinct ids, so this only guards against a mistake.
pub fn layer_index(doc: &Document, layer: LayerRef) -> Result<usize, ApiError> {
    let mut places = doc.layers().iter().enumerate();
    let mut named = places
        .by_ref()
        .filter(|(_, l)| i64::from(l.persisted_id().get()) == layer);
    match (named.next(), named.next()) {
        (Some((at, _)), None) => Ok(at),
        _ => Err(ApiError::NoSuchLayer(layer)),
    }
}

/// The session id commands name the layer with this persisted id by.
pub fn layer_id(doc: &Document, layer: LayerRef) -> Result<LayerId, ApiError> {
    let index = layer_index(doc, layer)?;
    doc.layers()
        .get(index)
        .map(LayerId::from)
        .ok_or(ApiError::NoSuchLayer(layer))
}

/// The place of the frame with this handle. Refused unless exactly one frame has it, so a
/// handle never names a frame it was not given for.
pub fn frame(doc: &Document, frame: FrameRef) -> Result<usize, ApiError> {
    let missing = ApiError::NoSuchFrame(frame);
    let key = u64::try_from(frame).map_err(|_| missing.clone())?;
    let mut places = doc.frames.iter().enumerate();
    let mut named = places.by_ref().filter(|(_, f)| f.key().get() == key);
    match (named.next(), named.next()) {
        (Some((at, _)), None) => Ok(at),
        _ => Err(missing),
    }
}

/// A canvas pixel.
pub fn pixel(doc: &Document, x: i64, y: i64) -> Result<(i32, i32), ApiError> {
    let inside = |v: i64, size: usize| {
        usize::try_from(v)
            .ok()
            .filter(|&v| v < size)
            .and_then(|v| i32::try_from(v).ok())
    };
    match (inside(x, doc.width), inside(y, doc.height)) {
        (Some(x), Some(y)) => Ok((x, y)),
        _ => Err(ApiError::Outside { x, y }),
    }
}

/// A block of `width` by `height` pixels from (`x`, `y`), all inside the canvas: its corner
/// and size.
pub fn rect(
    doc: &Document,
    (x, y): (i64, i64),
    width: i64,
    height: i64,
) -> Result<(i32, i32, usize, usize), ApiError> {
    let (left, top) = pixel(doc, x, y)?;
    let most_wide = i64::try_from(doc.width).unwrap_or(i64::MAX) - x;
    let most_high = i64::try_from(doc.height).unwrap_or(i64::MAX) - y;
    let w = narrow("a block's width", width, 1, most_wide)?;
    let h = narrow("a block's height", height, 1, most_high)?;
    Ok((left, top, w, h))
}

/// The color id of a swatch.
pub fn swatch(doc: &Document, swatch: SwatchRef) -> Result<u32, ApiError> {
    u32::try_from(swatch)
        .ok()
        .filter(|id| doc.colors.swatches().contains(id))
        .ok_or(ApiError::NoSuchSwatch(swatch))
}

/// A blend mode by the name the layer panel shows, among those the document's layers take.
pub fn blend(doc: &Document, name: &str) -> Result<BlendMode, ApiError> {
    doc.blending
        .modes()
        .iter()
        .copied()
        .find(|mode| mode.name() == name)
        .ok_or_else(|| ApiError::UnknownBlend(quoted(name)))
}

/// Most characters of script text an error quotes.
pub const MOST_QUOTED: usize = 64;

/// Script text as an error keeps it: at most [`MOST_QUOTED`] characters, then an ellipsis,
/// so an error never carries a copy of a huge string.
pub fn quoted(text: &str) -> String {
    match text.char_indices().nth(MOST_QUOTED) {
        None => text.to_string(),
        Some((end, _)) => format!("{}\u{2026}", &text[..end]),
    }
}
