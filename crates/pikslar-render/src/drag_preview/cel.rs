//! Showing a whole cel being dragged, the same way as floating pixels: what lies under and over
//! the active layer is cached once, and each frame reads the cel where it is now.

use super::{sampling, DragPreview};
use crate::compose::Compositor;
use crate::view::{Rect, View};
use pikslar_core::Editor;

/// The epoch of a preview built for a cel rather than for floating pixels.
pub const CEL_EPOCH: u64 = u64::MAX;

impl DragPreview {
    /// Prepares a preview of the active cel being moved. `None` if no canvas is in view, or if
    /// the onion skin shows a frame drawn from the same image, which a move changes too.
    pub fn build_cel(
        comp: &mut Compositor,
        editor: &Editor,
        view: View,
        area: Rect,
    ) -> Option<DragPreview> {
        comp.refresh_lut(editor, false);
        let layer = editor.layer;
        let active = editor.doc().layers().get(layer)?;
        let current = active.cel(editor.frame);
        let ghosts = comp.ghosts(editor);
        if ghosts
            .iter()
            .any(|g| active.cel(g.frame).same_image(current))
        {
            return None;
        }
        let (width, height) = (editor.doc().width as i32, editor.doc().height as i32);
        let sample = sampling(view, area, width, height)?;
        let size = (width, height);
        let mut preview = DragPreview::empty((view, area, size), sample, comp, CEL_EPOCH);
        // The whole cel moves, so where it is not, the layer shows nothing.
        preview.fill_caches(comp, editor, layer, |_, _| 0);
        Some(preview)
    }

    /// Draws the active cel where it is now. Returns false if it has not moved.
    pub fn render_cel(&mut self, editor: &Editor) -> bool {
        let layer = editor.layer;
        let Some(active) = editor.doc().layers().get(layer) else {
            return false;
        };
        let cel = active.cel(editor.frame);
        let (x, y) = cel.position();
        let state = (((x as u16 as u64) << 16) | y as u16 as u64, None);
        if self.drawn == Some(state) {
            return false;
        }
        self.drawn = Some(state);
        let s = self.sample;
        let (doc, frame) = (editor.doc(), editor.frame);
        self.compose(editor, layer, |row_y, scan| {
            let mut reader = cel.reader();
            for (col, out) in scan.iter_mut().enumerate() {
                let x = s.x + col as i32 * s.step;
                // A map's cells are tiles to look up, not pixels to read.
                *out = if active.is_tilemap() {
                    doc.layer_pixel(active, frame, x, row_y)
                } else {
                    reader.get(x, row_y)
                };
            }
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        area, blank, scene, scene_with_map, surface, through_compositor, views, SIZE,
    };
    use super::*;
    use pikslar_core::{BlendMode, CelCmd, ColorMode, FrameCmd, StepCmd};

    fn move_active(e: &mut Editor, dx: i32, dy: i32) {
        let layer = e.layer_id(e.layer).expect("layer");
        let frame = e.frame;
        e.apply(CelCmd::Move {
            layer,
            frame,
            dx,
            dy,
        })
        .expect("move");
    }

    fn shown(preview: &mut DragPreview, e: &Editor) -> Vec<u32> {
        preview.render_cel(e);
        let mut px = blank();
        preview.blit(&mut surface(&mut px), &mut Vec::new());
        px
    }

    /// Builds a preview, then drags the cel through several places, comparing each with the
    /// compositor. The preview is built once, as during a real drag.
    fn assert_matches_compositor(e: &mut Editor, setup: impl Fn(&mut Compositor), what: &str) {
        let moves = [(5, 3), (-9, 4), (17, -16), (-60, 2), (70, 70), (-3, -100)];
        for view in views() {
            let mut comp = Compositor::new(SIZE, SIZE);
            setup(&mut comp);
            e.damage_all();
            through_compositor(e, &mut comp, view);
            let mut preview = DragPreview::build_cel(&mut comp, e, view, area()).expect("in view");
            for (dx, dy) in moves {
                move_active(e, dx, dy);
                let previewed = shown(&mut preview, e);
                let settled = through_compositor(e, &mut comp, view);
                let at = e.doc().layers()[e.layer].cel(e.frame).position();
                assert!(previewed == settled, "{what}, {view:?}, at {at:?}");
            }
            e.apply(StepCmd::AbortJoin).expect("back");
        }
    }

    #[test]
    fn matches_the_compositor_for_each_color_mode_blend_and_opacity() {
        for mode in [ColorMode::Palette, ColorMode::Rgba] {
            for blend in [BlendMode::Normal, BlendMode::Multiply] {
                let mut e = scene(mode);
                let id = e.layer_id(1).expect("layer");
                e.apply(pikslar_core::LayerCmd::SetBlend { layer: id, blend })
                    .expect("blend");
                e.apply(pikslar_core::LayerCmd::SetOpacity {
                    layer: id,
                    opacity: 150,
                })
                .expect("opacity");
                assert_matches_compositor(&mut e, |_| {}, &format!("{mode:?} {blend:?}"));
            }
        }
    }

    /// A tilemap cel being dragged shows its tiles where it is, as the compositor does, and
    /// the tilemap layers beside it show theirs.
    #[test]
    fn a_tilemap_cel_is_dragged_with_its_tiles_drawn() {
        for at in [0, 2, 4] {
            let mut e = scene_with_map(at);
            e.layer = at;
            assert!(e.doc().layers()[e.layer].is_tilemap());
            assert_matches_compositor(&mut e, |_| {}, &format!("a tilemap layer at {at}"));
        }
    }

    #[test]
    fn a_paint_cel_dragged_among_tilemap_layers_matches_the_compositor() {
        for map in [0, 2, 4] {
            let mut e = scene_with_map(map);
            e.layer = if map == 2 { 1 } else { 2 };
            assert!(!e.doc().layers()[e.layer].is_tilemap());
            assert_matches_compositor(&mut e, |_| {}, &format!("a map at {map}"));
            assert_matches_compositor(
                &mut e,
                |comp| comp.onion = true,
                &format!("a map at {map} with onion skin"),
            );
        }
    }

    #[test]
    fn matches_the_compositor_for_the_bottom_and_top_layers_and_when_hidden() {
        for layer in [0, 3] {
            let mut e = scene(ColorMode::Rgba);
            e.layer = layer;
            assert_matches_compositor(&mut e, |_| {}, &format!("layer {layer}"));
        }
        let mut e = scene(ColorMode::Rgba);
        for (layer, visible) in [(1, false), (2, false)] {
            let id = e.layer_id(layer).expect("layer");
            e.apply(pikslar_core::LayerCmd::SetVisible { layer: id, visible })
                .expect("hide");
        }
        assert_matches_compositor(&mut e, |_| {}, "hidden");
    }

    #[test]
    fn matches_the_compositor_with_onion_skin_of_other_drawings() {
        let mut e = scene(ColorMode::Rgba);
        for at in 1..3 {
            e.apply(FrameCmd::Add { at, copy: false }).expect("room");
        }
        e.frame = 1;
        super::super::tests::paint(&mut e, 1, 99);
        super::super::tests::paint(&mut e, 2, 98);
        e.layer = 1;
        assert_matches_compositor(&mut e, |comp| comp.onion = true, "onion");
    }

    #[test]
    fn a_ghost_drawn_from_the_same_image_is_left_to_the_compositor() {
        let mut e = scene(ColorMode::Rgba);
        e.apply(FrameCmd::Add { at: 1, copy: true }).expect("room");
        e.frame = 1;
        e.layer = 1;
        let mut comp = Compositor::new(SIZE, SIZE);
        comp.onion = true;
        let view = views()[0];
        let shares = e.doc().layers()[1]
            .cel(0)
            .same_image(e.doc().layers()[1].cel(1));
        let built = DragPreview::build_cel(&mut comp, &e, view, area());
        assert_eq!(
            built.is_none(),
            shares,
            "only a shared drawing stops the preview"
        );
        let id = e.layer_id(1).expect("layer");
        e.apply(FrameCmd::HoldPrevious {
            layer: id,
            frame: 1,
        })
        .expect("hold");
        assert!(DragPreview::build_cel(&mut comp, &e, view, area()).is_none());
        comp.onion = false;
        assert!(DragPreview::build_cel(&mut comp, &e, view, area()).is_some());
    }

    #[test]
    fn a_ghost_of_the_drawing_at_another_place_is_drawn_by_the_compositor() {
        use super::super::tests::{onion_styles, shared_apart};
        for (n, style) in onion_styles().into_iter().enumerate() {
            let mut e = shared_apart();
            let view = views()[0];
            let mut comp = Compositor::new(SIZE, SIZE);
            comp.onion = true;
            comp.onion_depth = 1;
            comp.onion_style = style;
            // Moving the cel moves the rest of its hold, so the preview leaves it.
            let built = DragPreview::build_cel(&mut comp, &e, view, area());
            assert!(built.is_none(), "{style:?}");
            let ghosted = through_compositor(&mut e, &mut comp, view);
            comp.onion = false;
            e.damage_all();
            let plain = through_compositor(&mut e, &mut comp, view);
            // The third style shows both sides strongly.
            if n == 2 {
                assert_ne!(ghosted, plain, "the ghost at its own place");
            }
        }
    }

    #[test]
    fn rendering_again_without_a_move_does_nothing() {
        let mut e = scene(ColorMode::Rgba);
        let mut comp = Compositor::new(SIZE, SIZE);
        let view = views()[1];
        let mut preview = DragPreview::build_cel(&mut comp, &e, view, area()).expect("in view");
        assert!(preview.render_cel(&e));
        assert!(!preview.render_cel(&e));
        move_active(&mut e, 1, 0);
        assert!(preview.render_cel(&e));
        assert_eq!(preview.epoch(), CEL_EPOCH);
    }
}
