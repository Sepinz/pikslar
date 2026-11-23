//! Floating pixels as plain data, so a crash cannot lose a move, paste or transform.
//!
//! Nothing about floating pixels has reached the document yet, so the snapshot is stored
//! beside an untouched document rather than merged into it. Restoring gives back a live
//! selection: it can still be moved, applied, or cancelled to get the original art back.

use super::{transform, Floating, Orientation};
use crate::color::ColorTable;
use crate::document::{ColorMode, LimitError, MAX_DIMENSION};
use crate::editor::Editor;
use crate::mask::Mask;
use std::sync::Arc;

/// Far beyond anywhere a drag can reach, but small enough that sums cannot overflow.
const POSITION_LIMIT: i32 = 1 << 24;

/// Everything needed to rebuild floating pixels on top of the document they came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FloatingSnapshot {
    pub frame: usize,
    pub layer: usize,
    /// The area the pixels fill now.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub flip: bool,
    pub turns: u8,
    pub copy: bool,
    pub pasted: bool,
    pub source: Arc<Mask>,
    /// The selection to go back to on cancel.
    pub before: Option<Arc<Mask>>,
    /// Pasted pixels. Lifted pixels are still in the document, so they are not stored.
    pub pixels: Option<Arc<Vec<u32>>>,
    /// The color table to go back to if a paste that added colors is cancelled.
    pub colors_before: Option<Arc<ColorTable>>,
}

fn in_range(value: i32) -> bool {
    (-POSITION_LIMIT..=POSITION_LIMIT).contains(&value)
}

fn valid_size(w: i32, h: i32) -> bool {
    w > 0 && h > 0 && w as usize <= MAX_DIMENSION && h as usize <= MAX_DIMENSION
}

impl Editor {
    /// Changes whenever the floating pixels do. Zero when there is nothing to protect.
    pub fn floating_stamp(&self) -> u64 {
        if self.floating.as_ref().is_some_and(|f| f.changed) {
            self.selection_version.max(1)
        } else {
            0
        }
    }

    /// Shares its data with the editor, so taking one costs almost nothing.
    pub fn floating_snapshot(&self) -> Option<FloatingSnapshot> {
        let f = self.floating.as_ref().filter(|f| f.changed)?;
        let (x, y, w, h) = self.selection.as_deref()?.bounds();
        let colors_before = f
            .paste_colors
            .as_ref()
            .filter(|(_, after)| Arc::ptr_eq(&self.doc.colors, after))
            .map(|(before, _)| before.clone());
        Some(FloatingSnapshot {
            frame: self.frame,
            layer: self.layer,
            x,
            y,
            w,
            h,
            flip: f.orientation.flip,
            turns: f.orientation.turns,
            copy: f.copy,
            pasted: f.pasted,
            source: f.source.clone(),
            before: f.before.clone(),
            pixels: f.pasted.then(|| f.pixels.clone()),
            colors_before,
        })
    }

    /// Files are not trusted, even ones with a good checksum.
    fn check_snapshot(&self, s: &FloatingSnapshot) -> Result<(), LimitError> {
        let layer = self.doc.layers.get(s.layer).ok_or(LimitError::NoSuchItem)?;
        if let Some(refusal) = self.paint_refusal(s.layer) {
            return Err(refusal);
        }
        if s.frame >= self.doc.frames.len() || s.turns > 3 {
            return Err(LimitError::NoSuchItem);
        }
        // Putting it down draws on every cel sharing the drawing, as any stroke does.
        if let Some(refusal) = self.shared_refusal(s.layer, s.frame) {
            return Err(refusal);
        }
        let (sx, sy, sw, sh) = s.source.bounds();
        let placed_fits = valid_size(s.w, s.h) && in_range(s.x) && in_range(s.y);
        if !placed_fits || !valid_size(sw, sh) || !in_range(sx) || !in_range(sy) {
            return Err(LimitError::Dimensions);
        }
        if let Some(before) = &s.before {
            let (bx, by, bw, bh) = before.bounds();
            if bw.min(bh) < 0
                || bw.max(bh) as usize > MAX_DIMENSION
                || !in_range(bx)
                || !in_range(by)
            {
                return Err(LimitError::Dimensions);
            }
        }
        let on_canvas = sx >= 0
            && sy >= 0
            && (sx + sw) as usize <= self.doc.width
            && (sy + sh) as usize <= self.doc.height;
        match &s.pixels {
            None if s.pasted || !on_canvas => Err(LimitError::Dimensions),
            None => Ok(()),
            Some(pixels) => {
                let palette = layer.mode == ColorMode::Palette;
                let known = self.doc.colors.len() as u32;
                let bad_id = palette && pixels.iter().any(|&id| id >= known);
                if pixels.len() != sw as usize * sh as usize || bad_id {
                    return Err(LimitError::Dimensions);
                }
                Ok(())
            }
        }
    }

    /// Rebuilds floating pixels. The document is left as it is if the snapshot is unusable.
    pub fn restore_floating(&mut self, s: FloatingSnapshot) -> Result<(), LimitError> {
        self.check_snapshot(&s)?;
        self.frame = s.frame;
        self.layer = s.layer;
        let pixels = match s.pixels {
            Some(pixels) => pixels,
            None => Arc::new(self.read_selected(&s.source)),
        };
        // A table that is not what the document grew from cannot be gone back to.
        let current = self.doc.colors.clone();
        let paste_colors = s
            .colors_before
            .filter(|before| current.colors().starts_with(before.colors()))
            .map(|before| (before, current));
        let mut floating = Floating {
            steps: Default::default(),
            before: s.before,
            source: s.source,
            pixels,
            copy: s.copy,
            pasted: s.pasted,
            changed: true,
            orientation: Orientation {
                flip: s.flip,
                turns: s.turns,
            },
            placed: Arc::new(Vec::new()),
            stale: true,
            paste_colors,
        };
        let (mask, placed) = transform(&floating, s.x, s.y, s.w, s.h);
        floating.placed = Arc::new(placed);
        self.floating_epoch += 1;
        self.floating = Some(floating);
        self.selection = Some(Arc::new(mask));
        self.selection_version += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::color::rgba;
    use crate::document::{ColorMode, Document};
    use crate::editor::Editor;
    use crate::floating::Clip;
    use crate::mask::Mask;
    use crate::selection::SelectOp;
    use std::sync::Arc;

    const INK: u32 = rgba(200, 40, 20, 255);

    fn editor(mode: ColorMode) -> Editor {
        let mut e = Editor::new(Document::new(16, 16, mode).expect("valid"));
        e.color = INK;
        for (x, y) in [(2, 2), (3, 2), (3, 3)] {
            e.begin(crate::Tool::Pencil, x, y).expect("begin");
            e.commit();
        }
        e
    }

    fn picture(e: &mut Editor) -> Vec<u32> {
        e.settle_floating();
        let mut seen = Vec::new();
        for y in -4..24 {
            for x in -4..24 {
                seen.push(e.color_at(x, y));
            }
        }
        seen
    }

    /// A new editor on the same document, as a restart would produce.
    fn reborn(e: &Editor) -> Editor {
        let mut next = Editor::new(e.doc.clone());
        next.restore_floating(e.floating_snapshot().expect("floating"))
            .expect("restores");
        next
    }

    fn lifted(mode: ColorMode) -> Editor {
        let mut e = editor(mode);
        e.select(Mask::rect(2, 2, 2, 2), SelectOp::Replace);
        e.lift(false);
        e
    }

    #[test]
    fn nothing_to_protect_until_the_pixels_change() {
        let mut e = lifted(ColorMode::Palette);
        assert_eq!(e.floating_stamp(), 0);
        assert!(e.floating_snapshot().is_none());
        e.move_floating(1, 0);
        assert_ne!(e.floating_stamp(), 0);
        assert!(e.floating_snapshot().is_some());
    }

    #[test]
    fn moved_pixels_come_back_where_they_were() {
        for mode in [ColorMode::Palette, ColorMode::Rgba] {
            let mut e = lifted(mode);
            e.move_floating(5, 3);
            let mut back = reborn(&e);
            assert_eq!(picture(&mut back), picture(&mut e), "{mode:?}");
            assert_eq!(back.selection().map(|m| m.bounds()), Some((7, 5, 2, 2)));
        }
    }

    #[test]
    fn pixels_dragged_off_the_canvas_are_not_lost() {
        let mut e = lifted(ColorMode::Rgba);
        e.move_floating(-5, 0);
        let mut back = reborn(&e);
        back.move_floating(5, 0);
        back.anchor();
        assert_eq!(back.color_at(2, 2), INK);
        assert_eq!(back.color_at(3, 3), INK);
    }

    #[test]
    fn scales_flips_and_turns_are_kept_and_still_reversible() {
        let mut e = lifted(ColorMode::Palette);
        e.flip_horizontal();
        e.rotate(true);
        e.scale_floating(4, 4, 6, 8);
        let mut back = reborn(&e);
        assert_eq!(picture(&mut back), picture(&mut e));
        back.scale_floating(4, 4, 2, 2);
        e.scale_floating(4, 4, 2, 2);
        assert_eq!(picture(&mut back), picture(&mut e));
    }

    #[test]
    fn a_color_table_the_document_did_not_grow_from_is_never_gone_back_to() {
        let mut e = lifted(ColorMode::Palette);
        e.move_floating(1, 0);
        let mut other = (*e.doc.colors).clone();
        other.set_color(other.len() as u32 - 1, rgba(1, 2, 3, 255));
        let mut snapshot = e.floating_snapshot().expect("floating");
        snapshot.colors_before = Some(Arc::new(other));
        let mut back = Editor::new(e.doc.clone());
        back.restore_floating(snapshot).expect("restored");
        back.revert_floating();
        assert_eq!(back.doc.colors.colors(), e.doc.colors.colors());
    }

    #[test]
    fn cancelling_after_a_restore_gives_back_the_original_art() {
        let mut e = lifted(ColorMode::Palette);
        let original = e.doc.layers[0].cel(0).to_cells(e.doc.width, e.doc.height);
        e.move_floating(4, 4);
        let mut back = reborn(&e);
        back.revert_floating();
        assert_eq!(
            back.doc.layers[0]
                .cel(0)
                .to_cells(back.doc.width, back.doc.height),
            original
        );
        assert_eq!(back.selection().map(|m| m.bounds()), Some((2, 2, 2, 2)));
    }

    #[test]
    fn applying_after_a_restore_is_one_undo_step() {
        let mut e = lifted(ColorMode::Rgba);
        let original = e.doc.layers[0].cel(0).to_cells(e.doc.width, e.doc.height);
        e.move_floating(4, 0);
        let mut back = reborn(&e);
        back.anchor();
        assert_eq!(back.color_at(6, 2), INK);
        assert_eq!(back.color_at(2, 2), 0);
        back.undo();
        assert_eq!(
            back.doc.layers[0]
                .cel(0)
                .to_cells(back.doc.width, back.doc.height),
            original
        );
    }

    #[test]
    fn a_copy_drag_stays_a_copy() {
        let mut e = editor(ColorMode::Palette);
        e.select(Mask::rect(2, 2, 2, 2), SelectOp::Replace);
        e.lift(true);
        e.move_floating(6, 0);
        let mut back = reborn(&e);
        back.anchor();
        assert_eq!(back.color_at(2, 2), INK);
        assert_eq!(back.color_at(8, 2), INK);
    }

    #[test]
    fn a_paste_is_kept_and_cancelling_removes_the_colors_it_added() {
        let new_color = rgba(1, 200, 3, 255);
        let mut e = editor(ColorMode::Palette);
        let table = e.doc.colors.len();
        let clip = Clip::from_image(2, 1, vec![new_color, new_color]);
        e.paste(&clip, 9, 9).expect("fits");
        assert_eq!(e.doc.colors.len(), table + 1);
        let mut back = reborn(&e);
        assert_eq!(picture(&mut back), picture(&mut e));
        back.revert_floating();
        assert_eq!(back.doc.colors.len(), table);
    }

    #[test]
    fn the_frame_and_layer_it_belongs_to_are_restored() {
        let mut e = editor(ColorMode::Palette);
        e.transact(|d| d.add_frame(1, true)).expect("room");
        e.transact(|d| d.add_layer(1, ColorMode::Rgba))
            .expect("room");
        e.frame = 1;
        e.layer = 0;
        e.select(Mask::rect(2, 2, 2, 2), SelectOp::Replace);
        e.lift(false);
        e.move_floating(1, 1);
        let back = reborn(&e);
        assert_eq!((back.frame, back.layer), (1, 0));
    }

    #[test]
    fn unusable_snapshots_are_refused_and_change_nothing() {
        let mut e = lifted(ColorMode::Palette);
        e.move_floating(1, 0);
        let good = e.floating_snapshot().expect("floating");
        let breakages: Vec<fn(&mut super::FloatingSnapshot)> = vec![
            |s| s.layer = 9,
            |s| s.frame = 9,
            |s| s.turns = 4,
            |s| s.w = 0,
            |s| s.h = -1,
            |s| s.w = 100_000,
            |s| s.x = i32::MAX,
            |s| s.source = Arc::new(Mask::rect(14, 14, 8, 8)),
            |s| s.source = Arc::new(Mask::rect(-1, 0, 4, 4)),
            |s| s.pasted = true,
            |s| s.pixels = Some(Arc::new(vec![0; 3])),
            |s| s.pixels = Some(Arc::new(vec![9_999; 4])),
            |s| s.before = Some(Arc::new(Mask::rect(i32::MAX - 1, 0, 4, 4))),
            |s| s.before = Mask::from_words(0, 0, 100_000, 0, Vec::new()).map(Arc::new),
        ];
        for (i, broken) in breakages.iter().enumerate() {
            let mut bad = good.clone();
            broken(&mut bad);
            let mut fresh = Editor::new(e.doc.clone());
            assert!(fresh.restore_floating(bad).is_err(), "case {i}");
            assert!(!fresh.is_floating(), "case {i}");
        }
    }
}
