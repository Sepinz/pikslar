//! Slices drawn over the canvas: an outline in the slice's color, its name at the top left, the
//! pivot as a small cross and the centre as a dashed rectangle. Written straight to the screen
//! like the selection's outline, so a document with no slices costs nothing.

use crate::app::App;
use crate::panel_bottom::fit_to_width;
use pikslar_core::{Slice, MAX_SLICE_NAME};
use pikslar_render::{draw_frame, screen_rect, Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{cap_height, draw_text, draw_text_edit, text_width};

const SHADOW: u32 = 0xff00_0000;
/// How far the pivot's cross reaches from its pixel's middle, in screen pixels.
const ARM: i32 = 3;
/// The length of a dash and of the gap after it, in screen pixels.
const DASH: i32 = 3;
const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// A slice's color is kept as RGBA; the screen has it opaque, with blue lowest.
fn on_screen(rgba: u32) -> u32 {
    0xff00_0000 | (rgba >> 8)
}

fn mix(hash: &mut u64, value: u64) {
    *hash = (*hash ^ value).wrapping_mul(FNV_PRIME);
}

fn mix_slice(hash: &mut u64, s: &Slice) {
    let b = s.bounds;
    for v in [b.x, b.y, b.width as i32, b.height as i32] {
        mix(hash, v as u64);
    }
    if let Some((x, y)) = s.pivot {
        mix(hash, u64::from(x as u32) << 32 | u64::from(y as u32));
    }
    if let Some(c) = s.center {
        for v in [c.x, c.y, c.width as i32, c.height as i32] {
            mix(hash, v as u64);
        }
    }
    mix(hash, u64::from(s.color));
    s.name.chars().for_each(|c| mix(hash, u64::from(c)));
}

fn put(sf: &mut Surface<'_>, clip: Rect, x: i32, y: i32, color: u32) {
    if clip.contains(x, y) {
        sf.px[(y * sf.w + x) as usize] = color;
    }
}

/// The screen pixels a one-pixel frame in dashes covers within `clip`. Only the part of each
/// side inside the clip is walked, however far the rectangle reaches.
pub(crate) fn dashed_points(clip: Rect, r: Rect) -> impl Iterator<Item = (i32, i32)> {
    let on = |t: &i32| t.rem_euclid(2 * DASH) < DASH;
    let across = move |from: i32, to: i32, lo: i32, hi: i32| (from.max(lo)..to.min(hi)).filter(on);
    let rows = [r.y, r.bottom() - 1]
        .into_iter()
        .filter(move |&y| y >= clip.y && y < clip.bottom());
    let columns = [r.x, r.right() - 1]
        .into_iter()
        .filter(move |&x| x >= clip.x && x < clip.right());
    let top_and_bottom =
        rows.flat_map(move |y| across(r.x, r.right(), clip.x, clip.right()).map(move |x| (x, y)));
    let sides = columns
        .flat_map(move |x| across(r.y, r.bottom(), clip.y, clip.bottom()).map(move |y| (x, y)));
    top_and_bottom.chain(sides)
}

/// A one-pixel frame drawn in dashes.
fn dashed_frame(sf: &mut Surface<'_>, clip: Rect, r: Rect, color: u32) {
    for (x, y) in dashed_points(clip, r) {
        put(sf, clip, x, y, color);
    }
}

impl App {
    /// What the marks look like now, so the canvas is drawn again when it changes. Nothing is
    /// done, and nothing is kept, for a document without slices.
    pub(crate) fn slices_stamp(&self) -> u64 {
        let slices = &self.editor.doc().slices;
        if slices.is_empty() {
            return 0;
        }
        if !self.show_slices {
            return 1;
        }
        let mut hash = FNV_BASIS;
        slices.iter().for_each(|s| mix_slice(&mut hash, s));
        if let Some(n) = &self.slice_naming {
            mix(&mut hash, n.index as u64);
            mix(&mut hash, n.edit.caret() as u64);
            let (from, to) = n.edit.selection();
            mix(&mut hash, (from as u64) << 32 | to as u64);
            n.edit
                .text()
                .chars()
                .for_each(|c| mix(&mut hash, u64::from(c)));
        }
        hash | 2
    }

    /// Asks for the canvas to be drawn again when the slices, or whether they show, changed.
    pub(crate) fn sync_slices(&mut self) {
        let stamp = self.slices_stamp();
        if stamp != self.slices_drawn {
            self.slices_drawn = stamp;
            self.full_canvas = true;
        }
    }

    pub(crate) fn draw_slice_marks(&self, sf: &mut Surface<'_>, area: Rect) {
        let slices = &self.editor.doc().slices;
        if !self.show_slices || slices.is_empty() {
            return;
        }
        let clip = area.intersect(self.layout.canvas).intersect(sf.bounds());
        if clip.is_empty() {
            return;
        }
        for (index, slice) in slices.iter().enumerate() {
            self.draw_slice(sf, clip, index, slice);
        }
    }

    fn draw_slice(&self, sf: &mut Surface<'_>, clip: Rect, index: usize, slice: &Slice) {
        let (view, b) = (self.view, slice.bounds);
        let color = on_screen(slice.color);
        let whole = Rect::new(b.x, b.y, b.width as i32, b.height as i32);
        draw_frame(sf, &view, clip, whole, color);
        if let Some(c) = slice.center {
            let r = screen_rect(&view, b.x + c.x, b.y + c.y, c.width as i32, c.height as i32);
            dashed_frame(sf, clip, r, color);
        }
        if let Some((px, py)) = slice.pivot {
            let cell = (view.num / view.den).max(1);
            let at = screen_rect(&view, b.x + px, b.y + py, 1, 1);
            let (cx, cy) = (at.x + cell / 2, at.y + cell / 2);
            sf.fill(
                Rect::new(cx - ARM, cy, 2 * ARM + 1, 1).intersect(clip),
                color,
            );
            sf.fill(
                Rect::new(cx, cy - ARM, 1, 2 * ARM + 1).intersect(clip),
                color,
            );
        }
        let top_left = screen_rect(&view, b.x, b.y, b.width as i32, b.height as i32);
        self.draw_slice_name(sf, index, slice, top_left);
    }

    /// The name inside the slice's top left corner, or the name being typed. A corner off the
    /// canvas moves the name to the visible part of the slice; it is skipped only when none of
    /// the slice shows or the name would not fit: it must not spill over the panels.
    fn draw_slice_name(&self, sf: &mut Surface<'_>, index: usize, slice: &Slice, on: Rect) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let canvas = self.layout.canvas;
        let seen = on.intersect(canvas);
        if seen.is_empty() {
            return;
        }
        let (x, y) = (seen.x + 2 * s, seen.y + 2 * s);
        let high = (cap_height() + 1) * ts;
        match self.slice_naming.as_ref().filter(|n| n.index == index) {
            Some(naming) => {
                let room = (text_width(&"M".repeat(MAX_SLICE_NAME), ts)).min(canvas.right() - x);
                // The box is kept on the canvas, however little of the slice is in view.
                let wide = room + 2 * s;
                let left = (x - s).min(canvas.right() - wide).max(canvas.x);
                let top = (y - s).min(canvas.bottom() - high - 2 * s).max(canvas.y);
                let box_ = Rect::new(left, top, wide, high + 2 * s);
                if room > 0 && inside(box_, canvas) {
                    sf.fill(box_, theme::raised());
                    let at = (box_.x + s, box_.y + s);
                    draw_text_edit(
                        sf,
                        at,
                        room,
                        &naming.edit,
                        (theme::text(), theme::raised()),
                        ts,
                    );
                }
            }
            None => {
                let label = fit_to_width(&slice.name, seen.w - 4 * s, ts);
                // The shadow reaches one pixel of text scale further.
                let box_ = Rect::new(x, y, text_width(&label, ts) + ts, high + ts);
                if !label.is_empty() && inside(box_, canvas) {
                    draw_text(sf, x + ts, y + ts, &label, SHADOW, ts);
                    draw_text(sf, x, y, &label, on_screen(slice.color), ts);
                }
            }
        }
    }
}

/// Whether `r` lies wholly within `outer`.
fn inside(r: Rect, outer: Rect) -> bool {
    r.x >= outer.x && r.y >= outer.y && r.right() <= outer.right() && r.bottom() <= outer.bottom()
}
