//! What the context bar shows for each tool (docs/ui-redesign.md 3.4), and what each setting
//! does. Only settings the editor has are listed: mirror, ink, tolerance and the rest of the
//! plan's table wait for the features behind them.

use crate::app::{App, ToolChoice};
use crate::context_bar::{ID_MODE, ID_ONLY_CEL, ID_STEPPER, ID_WAND};
use crate::frame_menu::{Entries, MenuKind, OpenMenu};
use crate::panel_side::ID_BRUSH;
use pikslar_core::{SelectOp, Tool};
use pikslar_render::{Rect, Surface};
use pikslar_ui::segmented::{segmented, Segment, Show};
use pikslar_ui::stepper::{stepper, Range};
use pikslar_ui::{text_width, theme, Icon};
use std::borrow::Cow;

/// One control of the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Setting {
    Size,
    Brush,
    PixelPerfect,
    Rhythm,
    SelectMode,
    Contiguity,
    OnlyCel,
}

const PAINT: [Setting; 4] = [
    Setting::Size,
    Setting::Brush,
    Setting::PixelPerfect,
    Setting::Rhythm,
];
const SHAPE: [Setting; 2] = [Setting::Size, Setting::PixelPerfect];
const SELECT: [Setting; 1] = [Setting::SelectMode];
const WAND: [Setting; 2] = [Setting::SelectMode, Setting::Contiguity];
const MOVE: [Setting; 1] = [Setting::OnlyCel];

/// The settings of `tool`, in the order they are drawn and given up (last first).
pub(crate) fn settings(tool: ToolChoice) -> &'static [Setting] {
    match tool {
        ToolChoice::Paint(Tool::Pencil | Tool::Eraser) => &PAINT,
        ToolChoice::Paint(Tool::Line | Tool::Rect) => &SHAPE,
        ToolChoice::Paint(Tool::Fill | Tool::Picker) | ToolChoice::Text => &[],
        ToolChoice::Marquee | ToolChoice::Lasso => &SELECT,
        ToolChoice::Wand => &WAND,
        ToolChoice::Move => &MOVE,
    }
}

/// The tool's icon.
pub(crate) fn badge(tool: ToolChoice) -> Icon {
    match tool {
        ToolChoice::Paint(Tool::Pencil) => Icon::Pencil,
        ToolChoice::Paint(Tool::Eraser) => Icon::Eraser,
        ToolChoice::Paint(Tool::Fill) => Icon::Fill,
        ToolChoice::Paint(Tool::Picker) => Icon::Picker,
        ToolChoice::Paint(Tool::Line) => Icon::Line,
        ToolChoice::Paint(Tool::Rect) => Icon::Rect,
        ToolChoice::Text => Icon::Text,
        ToolChoice::Marquee => Icon::Marquee,
        ToolChoice::Lasso => Icon::Lasso,
        ToolChoice::Wand => Icon::Wand,
        ToolChoice::Move => Icon::Move,
    }
}

/// A choice's wide label, its short one and its name.
type Words = (&'static str, &'static str, &'static str);

const MODES: [Words; 3] = [
    ("Replace", "Rep", "Selection Replace"),
    ("Add", "Add", "Selection Add"),
    ("Subtract", "Sub", "Selection Subtract"),
];
const MODE_OPS: [SelectOp; 3] = [SelectOp::Replace, SelectOp::Add, SelectOp::Subtract];
const WAND_KINDS: [Words; 2] = [
    ("Contiguous", "Cont", "Wand contiguous"),
    ("All", "All", "Wand all matching"),
];

impl App {
    pub(crate) fn tool_name(&self) -> &'static str {
        match self.tool {
            ToolChoice::Paint(Tool::Pencil) => "Pencil",
            ToolChoice::Paint(Tool::Eraser) => "Eraser",
            ToolChoice::Paint(Tool::Fill) => "Fill",
            ToolChoice::Paint(Tool::Picker) => "Color picker",
            ToolChoice::Paint(Tool::Line) => "Line",
            ToolChoice::Paint(Tool::Rect) => "Rectangle",
            ToolChoice::Text => "Text",
            ToolChoice::Marquee => "Select box",
            ToolChoice::Lasso => "Lasso",
            ToolChoice::Wand => "Magic wand",
            ToolChoice::Move => "Move and scale",
        }
    }

    /// A setting's width in pixels in its wide and its short form.
    pub(crate) fn setting_width(&self, setting: Setting) -> (i32, i32) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let word = |w: &str, pad: i32| (text_width(w, ts) + pad * s).max(24 * s);
        let group = |labels: &[Words]| {
            let n = labels.len() as i32;
            let widest = |pick: fn(&Words) -> &'static str| {
                let longest = labels.iter().map(|l| text_width(pick(l), ts)).max();
                n * (longest.unwrap_or(0) + 12 * s) + s
            };
            (widest(|l| l.0), widest(|l| l.1))
        };
        match setting {
            Setting::Size => (68 * s, 68 * s),
            Setting::Brush => {
                let w = (28 + crate::brush_button::CARET_ROOM) * s;
                (w, w)
            }
            Setting::PixelPerfect => (word("Pixel perfect", 14), word("PP", 14)),
            Setting::Rhythm => (word("Line rhythm", 14), word("LR", 14)),
            Setting::SelectMode => group(&MODES),
            Setting::Contiguity => group(&WAND_KINDS),
            Setting::OnlyCel => (word("Only this cel", 14), word("Cel", 14)),
        }
    }

    /// Draws one setting in `r`, in its short form when `short`.
    pub(crate) fn draw_setting(
        &mut self,
        sf: &mut Surface<'_>,
        setting: Setting,
        r: Rect,
        short: bool,
    ) {
        match setting {
            Setting::Size => self.draw_size(sf, r),
            Setting::Brush => {
                self.bar.brush = r;
                self.draw_brush_button(sf, r);
            }
            Setting::PixelPerfect => {
                let label = if short { "PP" } else { "Pixel perfect" };
                self.note_hint(r, "Pixel Perfect: drops doubled corners");
                let on = self.editor.pixel_perfect;
                let id = (ID_BRUSH + 2, r);
                if self.bar_button(sf, id, (label, "Pixel perfect"), Some(on)) {
                    self.toggle_setting(setting);
                }
            }
            Setting::Rhythm => {
                let label = if short { "LR" } else { "Line rhythm" };
                self.note_hint(r, "Line Rhythm: steadies small hand tremor");
                if self.bar_button(
                    sf,
                    (ID_BRUSH + 3, r),
                    (label, "Line rhythm"),
                    Some(self.rhythm),
                ) {
                    self.toggle_setting(setting);
                }
            }
            Setting::SelectMode => self.draw_mode(sf, r, short),
            Setting::Contiguity => self.draw_contiguity(sf, r, short),
            Setting::OnlyCel => {
                let label = if short { "Cel" } else { "Only this cel" };
                self.note_hint(r, "Moves only this cel. Alt does the same for one drag");
                let on = self.bar.only_cel;
                if self.bar_button(sf, (ID_ONLY_CEL, r), (label, "Only this cel"), Some(on)) {
                    self.toggle_setting(setting);
                }
            }
        }
    }

    fn draw_size(&mut self, sf: &mut Surface<'_>, r: Rect) {
        self.note_hint(r, "Brush size  ] larger, [ smaller");
        let range = Range {
            value: self.editor.brush_size,
            least: 1,
            most: 64,
        };
        let id = (ID_STEPPER, r);
        if let Some(size) = stepper(&mut self.ui, sf, id, ("Brush size", theme::panel()), range) {
            self.adjust_brush(size - self.editor.brush_size);
        }
    }

    fn draw_mode(&mut self, sf: &mut Surface<'_>, r: Rect, short: bool) {
        let segments = MODES.map(|(wide, narrow, name)| Segment {
            name,
            show: Show::Words(if short { narrow } else { wide }),
        });
        self.note_hint(
            r,
            "How a new selection combines with the old one. Shift adds, Alt removes",
        );
        let chosen = MODE_OPS
            .iter()
            .position(|&o| o == self.bar.select)
            .unwrap_or(0);
        let id = (ID_MODE, r);
        if let Some(n) = segmented(&mut self.ui, sf, id, theme::panel(), &segments, chosen) {
            self.bar.select = MODE_OPS[n];
            self.ui_dirty = true;
        }
    }

    fn draw_contiguity(&mut self, sf: &mut Surface<'_>, r: Rect, short: bool) {
        let segments = WAND_KINDS.map(|(wide, narrow, name)| Segment {
            name,
            show: Show::Words(if short { narrow } else { wide }),
        });
        self.note_hint(
            r,
            "What the wand picks: the connected area, or every match. Ctrl turns it round",
        );
        let chosen = usize::from(self.bar.all_matching);
        if let Some(n) = segmented(
            &mut self.ui,
            sf,
            (ID_WAND, r),
            theme::panel(),
            &segments,
            chosen,
        ) {
            self.bar.all_matching = n == 1;
            self.ui_dirty = true;
        }
    }

    /// Flips a yes-or-no setting.
    pub(crate) fn toggle_setting(&mut self, setting: Setting) {
        match setting {
            Setting::PixelPerfect => self.editor.pixel_perfect = !self.editor.pixel_perfect,
            Setting::Rhythm => self.rhythm = !self.rhythm,
            Setting::OnlyCel => self.bar.only_cel = !self.bar.only_cel,
            Setting::Contiguity => self.bar.all_matching = !self.bar.all_matching,
            Setting::SelectMode => {
                let at = MODE_OPS
                    .iter()
                    .position(|&o| o == self.bar.select)
                    .unwrap_or(0);
                self.bar.select = MODE_OPS[(at + 1) % MODE_OPS.len()];
            }
            Setting::Size | Setting::Brush => {}
        }
        self.ui_dirty = true;
    }

    /// Whether the wand picks the connected area, given Ctrl.
    pub(crate) fn wand_contiguous(&self, ctrl: bool) -> bool {
        ctrl == self.bar.all_matching
    }

    /// The settings that did not fit, as rows of the More menu.
    pub(crate) fn bar_more_entries(&self) -> Entries {
        let on = |b: bool| if b { "on" } else { "off" };
        let from = self.bar.hidden_from;
        let label = |s: Setting| -> String {
            match s {
                Setting::Size => "Brush size".into(),
                Setting::Brush => "Brush menu".into(),
                Setting::PixelPerfect => {
                    format!("Pixel perfect: {}", on(self.editor.pixel_perfect))
                }
                Setting::Rhythm => format!("Line rhythm: {}", on(self.rhythm)),
                Setting::SelectMode => {
                    let at = MODE_OPS
                        .iter()
                        .position(|&o| o == self.bar.select)
                        .unwrap_or(0);
                    format!("Selection mode: {}", MODES[at].0)
                }
                Setting::Contiguity => {
                    format!("Wand: {}", WAND_KINDS[usize::from(self.bar.all_matching)].0)
                }
                Setting::OnlyCel => format!("Only this cel: {}", on(self.bar.only_cel)),
            }
        };
        settings(self.tool)
            .iter()
            .skip(from)
            .map(|&s| (Cow::Owned(label(s)), true))
            .collect()
    }

    pub(crate) fn bar_more_action(&mut self, n: usize) {
        let Some(&setting) = settings(self.tool).get(self.bar.hidden_from.saturating_add(n)) else {
            return;
        };
        if setting == Setting::Brush {
            let r = self.bar.more;
            self.open_brush_menu((r.x, r.bottom()));
        } else {
            self.toggle_setting(setting);
        }
    }

    pub(crate) fn open_bar_more(&mut self, at: (i32, i32)) {
        self.menu = Some(OpenMenu {
            at,
            kind: MenuKind::BarMore,
        });
        self.ui_dirty = true;
    }

    /// Shift+F10 on a bar control that has a menu: the Brush button's and More's. False when
    /// focus is on neither.
    pub(crate) fn bar_context_key(&mut self) -> bool {
        let Some(id) = self.ui.focus.focused() else {
            return false;
        };
        if id == ID_BRUSH + 4 && self.bar.brush.w > 0 {
            let r = self.bar.brush;
            self.open_brush_menu((r.x, r.bottom()));
            true
        } else if id == crate::context_bar::ID_MORE && self.bar.more.w > 0 {
            let r = self.bar.more;
            self.open_bar_more((r.x, r.bottom()));
            true
        } else {
            false
        }
    }
}
