//! Host functions that change the document (`EditDocument`). Each burns its fuel, then runs
//! one API operation; `set_pixel` keeps its pixel waiting (see `pending.rs`).

use crate::binding::Binding;
use crate::cost;
use crate::values::{int, Args};
use pikslar_api::{ApiError, Mode, Op, Reply};
use pikslar_script::{HostCx, HostError, Value};

type Out = Result<Value, HostError>;

/// Burns an edit's fuel and runs its operation, giving back what it made, if anything.
fn edit(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>, extra: u64, op: Op) -> Out {
    let paid = cost::edit(b.size()).saturating_add(extra);
    cx.charge(paid)?;
    b.before_edit();
    let made = match b.op(a.name, op)? {
        Reply::Created(id) => int(id),
        Reply::Frame(handle) => int(handle),
        _ => Value::Nil,
    };
    b.after_edit(a.name, paid)?;
    Ok(made)
}

pub fn add_layer(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let name = Some(a.text(0)?);
    let op = Op::AddLayer {
        above: None,
        mode: Mode::Rgba,
        name,
    };
    edit(b, a, cx, 0, op)
}

pub fn add_layer_above(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (above, name) = (Some(a.int(0)?), Some(a.text(1)?));
    let op = Op::AddLayer {
        above,
        mode: Mode::Rgba,
        name,
    };
    edit(b, a, cx, 0, op)
}

pub fn remove_layer(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let layer = a.int(0)?;
    edit(b, a, cx, 0, Op::RemoveLayer { layer })
}

pub fn rename_layer(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (layer, name) = (a.int(0)?, a.text(1)?);
    edit(b, a, cx, 0, Op::RenameLayer { layer, name })
}

pub fn move_layer(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (layer, to) = (a.int(0)?, a.int(1)?);
    edit(b, a, cx, 0, Op::MoveLayer { layer, to })
}

pub fn duplicate_layer(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let layer = a.int(0)?;
    // A group is copied with everything in it: each copied cel is paid for, so the copy
    // stays within the growth budget's bytes per unit (`growth.rs`).
    let cels = b.cels_copied(layer).unwrap_or(0) as u64;
    let extra = cels.saturating_mul(cost::COPIED_CEL);
    edit(b, a, cx, extra, Op::DuplicateLayer { layer })
}

pub fn set_opacity(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (layer, opacity) = (a.int(0)?, a.int(1)?);
    edit(b, a, cx, 0, Op::SetOpacity { layer, opacity })
}

pub fn set_visible(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (layer, visible) = (a.int(0)?, a.flag(1)?);
    edit(b, a, cx, 0, Op::SetVisible { layer, visible })
}

pub fn set_blend(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (layer, blend) = (a.int(0)?, a.text(1)?);
    edit(b, a, cx, 0, Op::SetBlend { layer, blend })
}

pub fn add_frame(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let at = a.int(0)?;
    let cels = cost::moved_cels(b.size());
    edit(b, a, cx, cels, Op::AddFrame { at, copy: false })
}

pub fn copy_frame(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let at = a.int(0)?;
    let cels = cost::moved_cels(b.size());
    edit(b, a, cx, cels, Op::AddFrame { at, copy: true })
}

pub fn remove_frame(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let frame = a.int(0)?;
    let cels = cost::moved_cels(b.size());
    edit(b, a, cx, cels, Op::RemoveFrame { frame })
}

pub fn set_duration(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (frame, ms) = (a.int(0)?, a.int(1)?);
    edit(b, a, cx, 0, Op::SetDuration { frame, ms })
}

pub fn add_tag(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (name, from, to) = (a.text(0)?, a.int(1)?, a.int(2)?);
    edit(b, a, cx, 0, Op::AddTag { name, from, to })
}

/// A color as the API takes it, checked here so a waiting pixel is refused where it is set.
fn color(b: &mut Binding<'_, '_>, name: &str, value: i64) -> Result<u32, HostError> {
    let refused = match u32::try_from(value) {
        Ok(c) if c == 0 || c >> 24 != 0 => return Ok(c),
        Ok(_) => ApiError::Transparent(value),
        Err(_) => ApiError::OutOfRange {
            what: "a color",
            value,
            min: 0,
            max: i64::from(u32::MAX),
        },
    };
    Err(b.refuse(name, refused))
}

pub fn set_pixel(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (layer, frame, x, y) = (a.int(0)?, a.int(1)?, a.int(2)?, a.int(3)?);
    let c = color(b, a.name, a.int(4)?)?;
    if c == 0 {
        return edit(b, a, cx, 0, Op::ErasePixel { layer, frame, x, y });
    }
    let cel = match b.pending.is_for(layer, frame) {
        Some(cel) => cel,
        None => {
            cx.charge(cost::edit(b.size()))?;
            b.start_cel(a.name, layer, frame)?
        }
    };
    let inside = |v: i64, size: usize| usize::try_from(v).ok().filter(|&v| v < size);
    let (Some(px), Some(py)) = (inside(x, cel.width), inside(y, cel.height)) else {
        return Err(b.refuse(a.name, ApiError::Outside { x, y }));
    };
    if b.pending.needs_block(px, py) {
        let paid = cost::new_block(b.size());
        cx.charge(paid)?;
        b.before_edit();
        b.after_edit(a.name, paid)?;
        if b.pending.is_full() {
            b.flush()?;
        }
    } else {
        cx.charge(cost::SET)?;
    }
    b.pending.put(px, py, c);
    Ok(Value::Nil)
}

pub fn erase_pixel(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (layer, frame, x, y) = (a.int(0)?, a.int(1)?, a.int(2)?, a.int(3)?);
    edit(b, a, cx, 0, Op::ErasePixel { layer, frame, x, y })
}

pub fn draw_line(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (layer, frame) = (a.int(0)?, a.int(1)?);
    let (from, to) = ((a.int(2)?, a.int(3)?), (a.int(4)?, a.int(5)?));
    let color = a.int(6)?;
    let (w, h, _, _) = b.size();
    let long = from.0.abs_diff(to.0).max(from.1.abs_diff(to.1));
    let long = long.min((w + h) as u64).saturating_mul(cost::LINE_PIXEL);
    let op = Op::DrawLine {
        layer,
        frame,
        from,
        to,
        color,
    };
    edit(b, a, cx, long, op)
}

pub fn fill(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (layer, frame, x, y, color) = (a.int(0)?, a.int(1)?, a.int(2)?, a.int(3)?, a.int(4)?);
    // A fill may reach every pixel of the canvas.
    let (w, h, _, _) = b.size();
    let canvas = (w as u64)
        .saturating_mul(h as u64)
        .saturating_mul(cost::FILL_PIXEL);
    let op = Op::Fill {
        layer,
        frame,
        x,
        y,
        color,
    };
    edit(b, a, cx, canvas, op)
}

pub fn fill_rect(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (layer, frame, x, y) = (a.int(0)?, a.int(1)?, a.int(2)?, a.int(3)?);
    let (width, height, color) = (a.int(4)?, a.int(5)?, a.int(6)?);
    // No larger than the canvas: a block that does not fit is refused, not priced.
    let (w, h, _, _) = b.size();
    let side = |v: i64, most: usize| v.clamp(0, crate::values::as_i64(most)) as u64;
    let area = side(width, w)
        .saturating_mul(side(height, h))
        .saturating_mul(cost::FILL_PIXEL);
    let op = Op::FillRect {
        layer,
        frame,
        x,
        y,
        width,
        height,
        color,
    };
    edit(b, a, cx, area, op)
}

pub fn draw_text(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (layer, frame, x, y) = (a.int(0)?, a.int(1)?, a.int(2)?, a.int(3)?);
    let (color, scale) = (a.int(5)?, a.int(6)?);
    // Priced by the drawing the API will make, before it makes it; at most 1 KB of text is
    // copied. The drawing is charged to the script's memory for the most its paste holds at
    // once (the clip, the cells made from it and a mask), not only for its own pixels.
    let text = a.text(4)?;
    let area = cost::text_area(&text, scale);
    let _drawing = cx.hold(area.saturating_mul(cost::TEXT_PASTE_BYTES_PER_PIXEL))?;
    let op = Op::DrawText {
        layer,
        frame,
        x,
        y,
        text,
        color,
        scale,
    };
    edit(b, a, cx, cost::text(area), op)
}

pub fn write_rect(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (layer, frame, x, y, width) = (a.int(0)?, a.int(1)?, a.int(2)?, a.int(3)?, a.int(4)?);
    let items = match a.values.get(5) {
        Some(Value::List(list)) => list.len() as u64,
        _ => 0,
    };
    // Paid for before the list is read: reading, checking and writing each item.
    let paid = cost::edit(b.size()).saturating_add(items.saturating_mul(2));
    cx.charge(paid)?;
    // The list is copied for the API (8 bytes an item) only when the canvas could take it:
    // more colors than pixels is refused as it stands. The copy is charged to the script's
    // memory while it lives, before it is made.
    let (w, h, _, _) = b.size();
    let area = (w as u64).saturating_mul(h as u64);
    if items > area {
        let refused = ApiError::OutOfRange {
            what: "the number of colors",
            value: i64::try_from(items).unwrap_or(i64::MAX),
            min: 1,
            max: i64::try_from(area).unwrap_or(i64::MAX),
        };
        return Err(b.refuse(a.name, refused));
    }
    let copy = usize::try_from(items).unwrap_or(usize::MAX);
    let _copy = cx.hold(copy.saturating_mul(std::mem::size_of::<i64>()))?;
    let colors = a.ints(5)?;
    let op = Op::PutPixels {
        layer,
        frame,
        x,
        y,
        width,
        colors,
    };
    b.before_edit();
    b.op(a.name, op)?;
    b.after_edit(a.name, paid)?;
    Ok(Value::Nil)
}

pub fn add_swatch(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let color = a.int(0)?;
    edit(b, a, cx, cost::palette(b.colors()), Op::AddSwatch { color })
}

pub fn recolor(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let (swatch, color) = (a.int(0)?, a.int(1)?);
    // A recolor redraws every pixel that shows the swatch.
    let (w, h, layers, frames) = b.size();
    let shown = cost::pixels((w as u64).saturating_mul(h as u64));
    let extra = shown.saturating_mul((layers as u64).saturating_mul(frames as u64).min(64));
    let extra = extra.saturating_add(cost::palette(b.colors()));
    edit(b, a, cx, extra, Op::Recolor { swatch, color })
}

pub fn remove_swatch(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    let swatch = a.int(0)?;
    edit(
        b,
        a,
        cx,
        cost::palette(b.colors()),
        Op::RemoveSwatch { swatch },
    )
}
