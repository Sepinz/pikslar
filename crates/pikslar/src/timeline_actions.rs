//! The timeline's own actions (docs/ui-redesign.md phase 6): the unfold action, the layer-name
//! column with its splitter, and the keys of the cel matrix.

use crate::app::App;
use crate::layout::matrix_row;
use crate::matrix_drag::Block;
use crate::panel_matrix::{ID_MATRIX, ID_NAMES, ID_NAMES_SPLIT};
use crate::panel_timeline::CELL;
use crate::shortcuts::Action;
use crate::timeline_geom::DEFAULT_NAMES;
use crate::workspace::model::NAMES_RANGE;
use pikslar_core::LayerCmd;
use pikslar_render::{Rect, Surface};
use pikslar_ui::role::{Role, State};
use pikslar_ui::splitter::{splitter, Axis, Span, SplitChange};
use pikslar_ui::{Press, WidgetKey};

/// Where the lock mark starts in a names row, in units (see `panel_matrix`).
const LOCK_FROM: i32 = 11;
/// Where the layer's name starts.
const NAME_FROM: i32 = 22;
/// A names splitter arrow moves the column this far.
const NAMES_STEP: i32 = 4;

impl App {
    /// Does `action` if it is one of this phase's. Returns whether it was.
    pub(crate) fn timeline_form_action(&mut self, action: Action) -> bool {
        match action {
            Action::TimelineForm => {
                self.toggle_matrix();
                true
            }
            _ => false,
        }
    }

    /// The layer-name column over the matrix rows in view, `first_row` rows down the list. A
    /// click on the eye or lock mark toggles it, a click on a name makes its layer current.
    /// Focused, Up and Down change the layer, Enter shows or hides it, Shift Enter locks it.
    pub(crate) fn names_column(&mut self, sf: &mut Surface<'_>, names: Rect, first_row: usize) {
        let state = self.ui.interact(ID_NAMES, names);
        // The value is the current layer, counted from 1 at the bottom.
        let layer = self.editor.layer as i32 + 1;
        self.ui.describe(Role::ListItem, State::value(layer));
        self.ui.focus.register_keys(ID_NAMES);
        self.ui.name(ID_NAMES, "Timeline layers");
        self.note_hint(
            names,
            "Click a name to draw on that layer, the eye to show or hide it, the lock to lock it",
        );
        let count = self.editor.doc().layers().len();
        if state.clicked {
            let (x, y) = self.ui.mouse;
            let s = self.ui.scale;
            let row = ((y - names.y) / (matrix_row() * s)).max(0) as usize;
            let layer = count.saturating_sub(1 + first_row + row);
            let zone = if x - names.x < LOCK_FROM * s {
                Mark::Eye
            } else if x - names.x < NAME_FROM * s {
                Mark::Lock
            } else {
                Mark::Name
            };
            self.defer(move |a| a.names_click(layer, zone));
        }
        if let Some(press) = self.ui.focus.take_press(ID_NAMES) {
            self.names_key(press);
        }
        self.ui.ring(sf, ID_NAMES, names);
    }

    /// The marks toggle at once, as the Layers panel's eye does; only picking a layer settles
    /// floating pixels first. Renaming stays with the Layers panel, which draws the field.
    fn names_click(&mut self, layer: usize, zone: Mark) {
        if layer >= self.editor.doc().layers().len() {
            return;
        }
        match zone {
            Mark::Eye => self.toggle_layer_mark(layer, false),
            Mark::Lock => self.toggle_layer_mark(layer, true),
            Mark::Name if self.settle() => self.editor.layer = layer,
            Mark::Name => {}
        }
        self.ui_dirty = true;
    }

    /// The layer `rows` rows above the current one in the list the Layers panel shows, which
    /// leaves out what folded groups hold. Kept to the list's ends.
    fn listed_neighbour(&self, rows: i32) -> usize {
        let listed = self.editor.doc().listed_layers();
        let row = self.listed_row(&listed, self.editor.layer) as i32;
        listed[(row - rows).clamp(0, listed.len() as i32 - 1) as usize]
    }

    fn names_key(&mut self, p: Press) {
        self.matrix.follow_again();
        let layer = self.editor.layer;
        let listed = self.editor.doc().listed_layers();
        let to = match p.key {
            WidgetKey::Up => self.listed_neighbour(1),
            WidgetKey::Down => self.listed_neighbour(-1),
            WidgetKey::Home => listed.first().copied().unwrap_or(layer),
            WidgetKey::End => listed.last().copied().unwrap_or(layer),
            WidgetKey::Enter => {
                self.defer(move |a| a.toggle_layer_mark(layer, p.shift));
                return;
            }
            _ => return,
        };
        self.defer(move |a| {
            if a.settle() {
                a.editor.layer = to;
                a.ui_dirty = true;
            }
        });
    }

    /// Shows or hides the layer, or locks or unlocks it.
    fn toggle_layer_mark(&mut self, index: usize, lock: bool) {
        let Some(layer) = self.editor.layer_id(index) else {
            return;
        };
        let doc = self.editor.doc().layers()[index].clone();
        if lock {
            let locked = !doc.locked;
            self.run(LayerCmd::SetLocked { layer, locked });
        } else {
            let visible = !doc.visible;
            self.run(LayerCmd::SetVisible { layer, visible });
        }
    }

    /// The gutter between the names and the cels, which sizes the names column.
    pub(crate) fn names_splitter(
        &mut self,
        sf: &mut Surface<'_>,
        gutter: Rect,
        (shown, most): (i32, i32),
    ) {
        // The size is the width drawn, so a press that does not drag keeps the preference.
        let span = Span {
            size: shown,
            least: NAMES_RANGE.0,
            most,
            step: NAMES_STEP,
            after: false,
        };
        let change = splitter(&mut self.ui, sf, ID_NAMES_SPLIT, gutter, Axis::Across, span);
        self.ui.name(ID_NAMES_SPLIT, "Size the layer names");
        let width = match change {
            Some(SplitChange::Resize(n)) => n,
            Some(SplitChange::Fold) => DEFAULT_NAMES,
            None => return,
        };
        self.defer(move |a| {
            a.workspaces = a.workspaces.with_names(width);
            a.ui_dirty = true;
        });
    }

    /// The keys of the focused cel matrix: arrows move the current cel, Shift extends the picked
    /// block from where it began, Home and End go to the first and last frame, Enter picks.
    pub(crate) fn matrix_key(&mut self, p: Press) {
        self.matrix.follow_again();
        let doc = self.editor.doc();
        let frames = doc.frames.len();
        let cur = (self.editor.layer, self.editor.frame);
        let to = match p.key {
            WidgetKey::Left => (cur.0, cur.1.saturating_sub(1)),
            WidgetKey::Right => (cur.0, (cur.1 + 1).min(frames - 1)),
            WidgetKey::Up => (self.listed_neighbour(1), cur.1),
            WidgetKey::Down => (self.listed_neighbour(-1), cur.1),
            WidgetKey::Home => (cur.0, 0),
            WidgetKey::End => (cur.0, frames - 1),
            WidgetKey::Enter => {
                self.pick(Block::single(cur));
                self.ui_dirty = true;
                return;
            }
            WidgetKey::Menu => return,
        };
        let picked = if p.shift {
            Block::spanning(self.anchor(cur), to)
        } else {
            Block::single(to)
        };
        // Picked once the move is made: settling floating pixels is an edit, which drops a
        // block picked before it, and a move that cannot settle leaves things as they were.
        self.defer(move |a| {
            if a.settle() {
                a.editor.layer = to.0.min(a.editor.doc().layers().len() - 1);
                a.set_frame(to.1);
                a.pick(picked);
            }
        });
    }

    /// The corner of the picked block opposite the current cel, or the current cel itself.
    fn anchor(&self, cur: (usize, usize)) -> (usize, usize) {
        match self.matrix.selection.filter(|b| b.contains(cur)) {
            Some(b) => {
                let layer = if cur.0 == b.layers.0 {
                    b.layers.1
                } else {
                    b.layers.0
                };
                let frame = if cur.1 == b.frames.0 {
                    b.frames.1
                } else {
                    b.frames.0
                };
                (layer, frame)
            }
            None => cur,
        }
    }

    /// The focus ring round the current cel while the matrix has focus.
    pub(crate) fn ring_current_cel(
        &mut self,
        sf: &mut Surface<'_>,
        (cells, first_row, first): (Rect, usize, usize),
        shown: usize,
    ) {
        let s = self.ui.scale;
        let count = self.editor.doc().layers().len();
        let row = (count - 1 - self.editor.layer).checked_sub(first_row);
        let col = self.editor.frame.checked_sub(first).filter(|&c| c < shown);
        let (Some(row), Some(col)) = (row, col) else {
            return;
        };
        let at = Rect::new(
            cells.x + col as i32 * CELL * s,
            cells.y + row as i32 * matrix_row() * s,
            CELL * s - 2 * s,
            matrix_row() * s - 2 * s,
        );
        if at.bottom() <= cells.bottom() {
            self.ui.ring(sf, ID_MATRIX, at);
        }
    }
}

/// Which part of a names row was clicked.
#[derive(Clone, Copy)]
enum Mark {
    Eye,
    Lock,
    Name,
}
