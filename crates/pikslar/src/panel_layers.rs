//! The layer list in the right panel: rows top first, groups indented under their row and
//! folded away on request, and the tools at the right of the Layers header.

use crate::app::App;
use crate::panel_fit::{List, LAYER_ROW};
use crate::panel_right::{Section, ID_LAYER_LOCK, ID_LAYER_ROW, ID_LAYER_TOOLS};
use pikslar_core::document::MAX_LAYERS;
use pikslar_core::{ColorMode, LayerCmd, LayerKind};
use pikslar_render::{Rect, Surface};
use pikslar_ui::chrome::mark_button;
use pikslar_ui::fold::{draw_arrow, ARROW_ROOM};
use pikslar_ui::icons::MarkIcon;
use pikslar_ui::theme;
use pikslar_ui::{cap_height, draw_text, draw_text_edit, icon_button_if, text_width, Icon};

const ID_LAYER_EYE: u32 = 90_000;
pub(crate) const ID_LAYER_FOLD: u32 = 92_000;
/// New, remove, up, down, duplicate, merge and group, at the right of the Layers header.
pub(crate) const LAYER_TOOLS: usize = 7;
/// Units each group a layer is in moves its row to the right.
const INDENT: i32 = 8;

impl App {
    /// The row of the layer list that stands for layer `at`: its own, or that of the folded
    /// group it is hidden in.
    pub(crate) fn listed_row(&self, listed: &[usize], at: usize) -> usize {
        let doc = self.editor.doc();
        let holds = |&index: &usize| {
            let (low, high) = doc.block(index);
            (low..=high).contains(&at)
        };
        let own = listed.iter().position(|&i| i == at);
        own.or_else(|| listed.iter().rposition(holds)).unwrap_or(0)
    }

    pub(crate) fn draw_layers(&mut self, sf: &mut Surface<'_>, area: Rect, folded: bool) {
        let s = self.ui.scale;
        let tools_left = area.right() - LAYER_TOOLS as i32 * (24 * s);
        let header = Rect::new(area.x, area.y, tools_left - area.x - 4 * s, 22 * s);
        self.ctx.note_layers(area);
        self.section_header(sf, Section::Layers, header, folded);
        self.draw_layer_tools(sf, area);
        // Folded, the list keeps two rows and the active layer's settings go.
        let details = if folded {
            area.bottom() + 6 * s
        } else {
            self.draw_layer_details(sf, area)
        };
        let list = Rect::new(area.x, area.y + 28 * s, area.w, details - area.y - 34 * s);
        let row_h = LAYER_ROW * s;
        let visible = (list.h / row_h).max(1) as usize;
        let listed = self.editor.doc().listed_layers();
        let count = listed.len();
        let active_row = self.listed_row(&listed, self.editor.layer);
        let first = self
            .panel_scroll
            .layer_window(list, active_row, visible, count);
        self.draw_scroll_bar(sf, List::Layers, list, (first, visible, count));
        for (shown, &index) in listed.iter().skip(first).take(visible).enumerate() {
            let y = list.y + shown as i32 * row_h;
            let r = Rect::new(list.x, y, list.w, row_h - 2 * s);
            self.draw_layer_row(sf, r, index);
        }
    }

    fn draw_layer_row(&mut self, sf: &mut Surface<'_>, r: Rect, index: usize) {
        let s = self.ui.scale;
        let id = index as u32;
        let layer = &self.editor.doc().layers()[index];
        let (shown, group, name) = (layer.visible, layer.kind == LayerKind::Group, &layer.name);
        let indent = i32::from(layer.depth) * INDENT * s;
        let eye = Rect::new(r.x, r.y, r.h, r.h);
        let lock = Rect::new(r.right() - r.h, r.y, r.h, r.h);
        let left = eye.right() + 2 * s + indent;
        let body = Rect::new(left, r.y, lock.x - left - 2 * s, r.h);
        // A group's fold arrow takes the left of its row, so the row's own clicks start after it.
        let arrow = if group { (ARROW_ROOM + 2) * s } else { 0 };
        let hit = Rect::new(body.x + arrow, body.y, body.w - arrow, body.h);
        let state = self.ui.interact(ID_LAYER_ROW + id, hit);
        if index == self.editor.layer {
            self.ctx.note_active_row(r);
        }
        if self.ui.naming() {
            let kind = if group { "Group" } else { "Layer" };
            self.ui.name(ID_LAYER_ROW + id, &format!("{kind} {name}"));
        }
        self.note_hint(
            body,
            "Click to draw on this layer, double-click to rename  Up, Down",
        );
        self.note_hint(eye, "Show or hide this layer");
        self.note_hint(lock, "Lock this layer so it is not drawn on");
        let active = index == self.editor.layer;
        let fill = match (active, state.hover) {
            (true, _) => theme::pressed(),
            (false, true) => theme::raised(),
            _ => pikslar_ui::theme::panel(),
        };
        sf.fill(body, fill);
        if active {
            sf.fill(Rect::new(body.x, body.y, 2 * s, body.h), theme::accent());
        }
        self.draw_layer_label(sf, body, index);
        if group {
            self.fold_button(sf, body, index);
        }
        if state.clicked {
            self.defer(move |a| {
                if a.settle() {
                    a.editor.layer = index;
                    a.layer_row_clicked(index);
                }
                a.ui_dirty = true;
            });
        }
        // The eye is a mark in ink, not a filled accent square: amber is for what is chosen.
        let (art, ink) = match shown {
            true => (MarkIcon::Eye, theme::text()),
            false => (MarkIcon::EyeOff, theme::disabled()),
        };
        let eye_at = (ID_LAYER_EYE + id, eye);
        if mark_button(&mut self.ui, sf, eye_at, art.art(), (ink, shown, "Eye")) {
            self.defer(move |a| {
                if let Some(layer) = a.editor.layer_id(index) {
                    let visible = !shown;
                    a.run(LayerCmd::SetVisible { layer, visible });
                }
            });
        }
        self.lock_button(sf, lock, index);
    }

    /// The arrow at the left of a group's row, which folds the group or opens it.
    fn fold_button(&mut self, sf: &mut Surface<'_>, body: Rect, index: usize) {
        let s = self.ui.scale;
        let layer = &self.editor.doc().layers()[index];
        let (open, name) = (!layer.folded, layer.name.clone());
        let r = Rect::new(body.x + 2 * s, body.y, ARROW_ROOM * s, body.h);
        let id = ID_LAYER_FOLD + index as u32;
        let state = self.ui.interact(id, r);
        self.ui.name(id, &format!("Fold {name}"));
        self.note_hint(
            r,
            if open {
                "Fold this group"
            } else {
                "Open this group"
            },
        );
        let ink = if state.hover {
            theme::text()
        } else {
            theme::muted()
        };
        let ts = self.ui.text_scale();
        draw_arrow(sf, (r.x + 2 * s, r.y + r.h / 2), open, ink, ts);
        if state.clicked {
            self.defer(move |a| a.fold_group(index, open));
        }
    }

    /// Folds or opens the group at `index`. A layer folded away hands its place as the active
    /// layer to the group.
    pub(crate) fn fold_group(&mut self, index: usize, folded: bool) {
        let Some(layer) = self.editor.layer_id(index) else {
            return;
        };
        self.run(LayerCmd::SetFolded { layer, folded });
        let (low, high) = self.editor.doc().block(index);
        if folded && (low..high).contains(&self.editor.layer) && self.settle() {
            self.editor.layer = index;
        }
        self.ui_dirty = true;
    }

    /// Marked while the layer is locked. Clicking locks or unlocks it.
    fn lock_button(&mut self, sf: &mut Surface<'_>, r: Rect, index: usize) {
        let locked = self.editor.doc().layers()[index].locked;
        let id = ID_LAYER_LOCK + index as u32;
        let (art, ink) = match locked {
            true => (MarkIcon::Lock, theme::text()),
            false => (MarkIcon::Unlock, theme::muted()),
        };
        let clicked = mark_button(&mut self.ui, sf, (id, r), art.art(), (ink, locked, "Lock"));
        if self.ui.naming() {
            let name = format!("Lock {}", self.editor.doc().layers()[index].name);
            self.ui.name(id, &name);
        }
        if clicked {
            self.defer(move |a| {
                if let Some(layer) = a.editor.layer_id(index) {
                    let locked = !locked;
                    a.run(LayerCmd::SetLocked { layer, locked });
                }
            });
        }
    }

    fn draw_layer_label(&self, sf: &mut Surface<'_>, body: Rect, index: usize) {
        let s = self.ui.scale;
        let ts = self.ui.text_scale();
        let layer = &self.editor.doc().layers()[index];
        let tag = match (layer.kind, layer.mode) {
            (LayerKind::Group, _) => "GRP",
            (LayerKind::Tilemap(_), _) => "MAP",
            (_, ColorMode::Palette) => "PAL",
            (_, ColorMode::Rgba) => "RGB",
        };
        let arrow = if layer.kind == LayerKind::Group {
            ARROW_ROOM * s
        } else {
            0
        };
        let text_x = body.x + 8 * s + arrow;
        let text_y = body.y + (body.h - cap_height() * ts) / 2;
        match self.layer_naming.as_ref().filter(|n| n.layer == index) {
            Some(naming) => {
                let room = body.right() - text_x - 8 * s - text_width(tag, ts);
                let colors = (theme::text(), theme::raised());
                draw_text_edit(sf, (text_x, text_y), room, &naming.edit, colors, ts);
            }
            None => {
                let label = crate::panel_right::layer_label(&layer.name, ts);
                draw_text(sf, text_x, text_y, &label, theme::text(), ts);
            }
        }
        let tag_x = body.right() - text_width(tag, ts) - 6 * s;
        draw_text(sf, tag_x, text_y, tag, theme::muted(), ts);
    }

    fn draw_layer_tools(&mut self, sf: &mut Surface<'_>, area: Rect) {
        let s = self.ui.scale;
        let size = 22 * s;
        for (i, (icon, hint, usable)) in self.layer_tools().into_iter().enumerate() {
            // The group tool came last, so it goes at the left and the others keep their places.
            let slot = (i + 1) % LAYER_TOOLS;
            let x = area.right() - (LAYER_TOOLS - slot) as i32 * (size + 2 * s) + 2 * s;
            let r = Rect::new(x, area.y, size, size);
            if i == 0 {
                self.tile_ui.new_layer = r;
            }
            // Scripts find the plus button by its name, so the right click hint rides apart.
            let name = if i == 0 { "New layer" } else { hint };
            self.note_hint_named(r, name, hint);
            if icon_button_if(usable, &mut self.ui, sf, ID_LAYER_TOOLS + i as u32, r, icon) {
                self.defer(move |a| a.layer_action(i));
            }
        }
    }

    /// The layer tools in `layer_action` order: icon, hint and whether each can be used now.
    pub(crate) fn layer_tools(&self) -> [(Icon, &'static str, bool); LAYER_TOOLS] {
        let doc = self.editor.doc();
        let at = self.editor.layer;
        let (low, high) = doc.block(at);
        let count = doc.layers().len();
        let layer = &doc.layers()[at];
        let grouped = layer.depth > 0;
        let group = layer.kind == LayerKind::Group;
        let merges = !group && at > 0 && {
            let below = &doc.layers()[at - 1];
            below.kind != LayerKind::Group && below.depth == layer.depth
        };
        let group_hint = match group {
            true => "Ungroup: take the layers out of this group",
            false => "Put this layer in a group",
        };
        [
            (
                Icon::Plus,
                "New layer. Right click for a tilemap layer",
                true,
            ),
            (Icon::Minus, "Remove this layer", high - low + 1 < count),
            (
                Icon::Up,
                "Move layer up  Shift Up",
                high + 1 < count || grouped,
            ),
            (
                Icon::Down,
                "Move layer down  Shift Down",
                low > 0 || grouped,
            ),
            (
                Icon::Copy,
                "Duplicate this layer",
                count + high - low < MAX_LAYERS,
            ),
            (Icon::Merge, "Merge into the layer below", merges),
            (Icon::Group, group_hint, group || count < MAX_LAYERS),
        ]
    }

    pub(crate) fn layer_action(&mut self, action: usize) {
        let doc = self.editor.doc();
        let at = self.editor.layer;
        let (low, high) = doc.block(at);
        let grouped = doc.layers()[at].depth > 0;
        let group = doc.layers()[at].kind == LayerKind::Group;
        let count = doc.layers().len();
        let mode = doc.layers()[at].mode;
        let Some(layer) = self.editor.layer_id(at) else {
            return;
        };
        let command = match action {
            0 => LayerCmd::Add { at: at + 1, mode },
            1 => LayerCmd::Remove { layer },
            2 if high + 1 < count || grouped => LayerCmd::Step { layer, up: true },
            3 if low > 0 || grouped => LayerCmd::Step { layer, up: false },
            4 => LayerCmd::Duplicate { layer },
            5 => LayerCmd::MergeDown { layer },
            6 if group => LayerCmd::Ungroup { layer },
            6 => LayerCmd::Group { layer },
            _ => {
                self.notify("That layer is already at the end".to_string());
                return;
            }
        };
        self.run(command);
    }
}
