//! The Color panel, and the fold headers of the side dock panels.

use crate::app::App;
use crate::color_ui::{from_hsva, hex, to_hsva, to_screen, Hsva};
use crate::panel_fit::List;
use crate::workspace::model::Panel;
use pikslar_core::{BlendMode, ColorMode, LayerCmd, PaletteCmd, StepCmd};
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{
    button, cap_height, draw_text, em, fold_header, scroll_bar, slider, text_width, Slide,
};

pub(crate) const ID_COLOR: u32 = 200;
pub(crate) const ID_SCROLL: u32 = 700_000;
pub(crate) const ID_LAYER_TOOLS: u32 = 240;
pub(crate) const ID_LAYER_ROW: u32 = 80_000;
pub(crate) const ID_LAYER_LOCK: u32 = 91_000;
/// The fold headers of the color, palette, layers and Pixel Lint sections, in that order.
pub(crate) const ID_SECTION: u32 = 270;

/// A section of the right panel that folds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    Color,
    Palette,
    Layers,
    Lint,
}

impl Section {
    /// The panel whose header this is.
    pub(crate) fn panel(self) -> Panel {
        match self {
            Section::Color => Panel::Color,
            Section::Palette => Panel::Palette,
            Section::Layers => Panel::Layers,
            Section::Lint => Panel::Lint,
        }
    }
}

const SLIDER_STEPS: i32 = 48;

/// Paints a horizontal gradient of colors chosen by position.
fn gradient(sf: &mut Surface<'_>, r: Rect, color_at: impl Fn(f32) -> u32) {
    let step = (r.w / SLIDER_STEPS).max(1);
    let mut x = r.x;
    while x < r.right() {
        let t = (x - r.x) as f32 / r.w.max(1) as f32;
        sf.fill(
            Rect::new(x, r.y, step.min(r.right() - x), r.h),
            to_screen(color_at(t)),
        );
        x += step;
    }
}

impl App {
    pub(crate) fn sync_color(&mut self) {
        // Undo and Rewind can change or remove the swatch being recolored.
        if let Some(id) = self.selected_swatch {
            if !self.editor.doc().colors.swatches().contains(&id) {
                self.selected_swatch = None;
            } else {
                self.follow_swatch(id);
            }
        }
        if self.editor.color != self.synced_color {
            let new = to_hsva(self.editor.color);
            // Gray and black carry no hue, so keep the hue the user last chose.
            self.hsva = Hsva {
                h: if new.s > 0.0 { new.h } else { self.hsva.h },
                s: if new.v > 0.0 { new.s } else { self.hsva.s },
                ..new
            };
            self.synced_color = self.editor.color;
        }
    }

    /// In Live mode the color is the swatch's, unless something else just chose a new color.
    fn follow_swatch(&mut self, id: u32) {
        let swatch = self.editor.doc().colors.color(id);
        let unchanged = swatch == std::mem::replace(&mut self.swatch_seen, swatch);
        if !self.live_palette {
            return;
        }
        if unchanged && self.editor.color != swatch {
            self.selected_swatch = None;
        } else {
            self.editor.color = swatch;
        }
    }

    fn apply_color(&mut self, hsva: Hsva, started: bool) {
        self.defer(move |a| a.pick_color(hsva, started));
    }

    pub(crate) fn pick_color(&mut self, hsva: Hsva, started: bool) {
        let color = from_hsva(hsva);
        self.hsva = hsva;
        self.editor.color = color;
        self.synced_color = color;
        match self.selected_swatch.filter(|_| self.live_palette) {
            Some(id) => {
                if started {
                    self.run(StepCmd::EndDrag);
                }
                self.run(PaletteCmd::Recolor { id, color });
            }
            None => self.selected_swatch = None,
        }
        self.ui_dirty = true;
    }

    /// Draws a section's fold header in `r`, which folds or opens it when clicked.
    pub(crate) fn section_header(
        &mut self,
        sf: &mut Surface<'_>,
        section: Section,
        r: Rect,
        folded: bool,
    ) {
        let title = match section {
            Section::Color => "",
            Section::Palette => "Palette",
            Section::Layers => "Layers",
            Section::Lint => "Pixel Lint",
        };
        let id = ID_SECTION + section as u32;
        // Folded by the panel for want of room, not by hand: a click could not show it.
        let for_room = folded && !self.folded_by_hand(section);
        // The hint names the section, which matters where its title has no room to show.
        let hint = match (for_room, folded, section) {
            (true, _, _) => "Folded for room. A taller window shows it",
            (_, true, Section::Color) => "Open the Color section",
            (_, true, Section::Palette) => "Open the Palette section",
            (_, true, Section::Layers) => "Open the Layers section",
            (_, true, Section::Lint) => "Open the Pixel Lint section",
            (_, _, Section::Color) => "Fold the Color section",
            (_, _, Section::Palette) => {
                "Fold the Palette section. Right click for the palette menu"
            }
            (_, _, Section::Layers) => "Fold the Layers section",
            (_, _, Section::Lint) => "Fold the Pixel Lint section",
        };
        self.note_hint(r, hint);
        if fold_header(&mut self.ui, sf, id, r, title, !folded) && !for_room {
            self.defer(move |a| a.fold_section(section, !folded));
        }
    }

    /// Whether the slot showing `section` was folded by hand.
    fn folded_by_hand(&self, section: Section) -> bool {
        let ws = self.workspaces.active();
        ws.find(section.panel())
            .and_then(|(side, i, _)| ws.area(side).slots().get(i).map(|sl| !sl.open))
            .unwrap_or(false)
    }

    /// Folds or opens the slot showing a section, by hand.
    pub(crate) fn fold_section(&mut self, section: Section, fold: bool) {
        let Some((side, i, _)) = self.workspaces.active().find(section.panel()) else {
            return;
        };
        self.edit_layout(|ws| {
            crate::dock::with_slot(ws, side, i, |sl| crate::workspace::model::Slot {
                open: !fold,
                ..sl
            })
        });
    }

    /// The Color panel in `area`: folded to the current color's swatch and value, compact
    /// when shorter than its full form, or full.
    pub(crate) fn draw_color(&mut self, sf: &mut Surface<'_>, area: Rect, folded: bool) {
        let s = self.ui.scale;
        let header = Rect::new(area.x, area.y, 24 * s, 22 * s);
        self.section_header(sf, Section::Color, header, folded);
        let swatch = Rect::new(header.right() + s, area.y, area.w - header.w - s, area.h);
        if folded {
            self.draw_color_value(sf, swatch, 22, true, None);
            return;
        }
        let compact = area.h < crate::panel_fit::FULL_COLOR * s;
        let (size, gap) = if compact { (22, 4) } else { (44, 10) };
        // A compact block keeps its two rows of sliders; what height is left may take the
        // value's label on a line of its own when it does not fit beside the value.
        let channels = crate::panel_fit::COLOR_CHANNELS * s;
        let below = (area.bottom() - channels - gap * s).max(area.y);
        let bottom = self.draw_color_value(sf, swatch, size, compact, Some(below));
        self.draw_channels(sf, area, bottom + gap * s, compact);
    }

    /// The four channel sliders, a row each at a 24 unit pitch, which their hit areas need. A
    /// compact block puts two side by side to save the height.
    fn draw_channels(&mut self, sf: &mut Surface<'_>, area: Rect, top: i32, compact: bool) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let columns = if compact { 2 } else { 1 };
        let width = (area.w - (columns - 1) * 8 * s) / columns;
        for (i, name) in ["H", "S", "V", "A"].iter().enumerate() {
            let (col, line) = (i as i32 % columns, i as i32 / columns);
            let x = area.x + col * (width + 8 * s);
            let y = top + line * 24 * s + 4 * s;
            let row = Rect::new(x + 16 * s, y, width - 16 * s, 16 * s);
            draw_text(
                sf,
                x,
                y + (row.h - cap_height() * ts) / 2,
                name,
                theme::muted(),
                ts,
            );
            self.channel_slider(sf, i, row);
        }
    }

    /// The current color's swatch, `size` units square, with its value and what it is for.
    /// Compact, the label goes beside the value, or on a line under it when it does not fit
    /// there and that line ends by `second_line_by`. Where neither has room, as in *Large text*
    /// at 2x, the label takes the value's place: what the swatch is for is the one thing the
    /// swatch cannot show, and its value stays in the swatch's hint. Returns the bottom of what
    /// was drawn.
    fn draw_color_value(
        &mut self,
        sf: &mut Surface<'_>,
        area: Rect,
        size: i32,
        compact: bool,
        second_line_by: Option<i32>,
    ) -> i32 {
        let s = self.ui.scale;
        let ts = self.ui.text_scale();
        let preview = Rect::new(area.x, area.y, size * s, size * s);
        sf.fill(preview, to_screen(self.editor.color));
        pikslar_ui::chrome::edge_outline(sf, preview, s);
        let value = hex(self.editor.color);
        let hint = if self.live_palette && self.selected_swatch.is_some() {
            "Recoloring"
        } else {
            "Foreground"
        };
        self.note_hint(preview, format!("{hint} color {value}"));
        let color = if hint == "Recoloring" {
            theme::accent()
        } else {
            theme::muted()
        };
        let text_x = preview.right() + if compact { 6 * s } else { 10 * s };
        let line = cap_height() * ts;
        let top = area.y + 4 * s;
        let beside = (text_x + text_width(&value, ts) + 6 * s, top);
        let under = (text_x, top + line + 4 * s);
        let fits = |(x, _): (i32, i32)| x + text_width(hint, ts) <= area.right();
        let place = if !compact {
            Some((text_x, area.y + 26 * s))
        } else if fits(beside) {
            Some(beside)
        } else {
            second_line_by
                .filter(|&by| under.1 + line <= by)
                .map(|_| under)
                .filter(|&at| fits(at))
        };
        let instead = place.is_none() && compact && fits((text_x, top));
        if !instead {
            draw_text(sf, text_x, top, &value, theme::text(), ts);
        }
        let place = place.or(instead.then_some((text_x, top)));
        let mut bottom = preview.bottom();
        if let Some((x, y)) = place {
            draw_text(sf, x, y, hint, color, ts);
            // Named for tests and scripts, which check that the label is shown.
            if self.ui.naming() {
                let r = Rect::new(x, y, text_width(hint, ts), line);
                self.ui.name_rect(r, &format!("Color label {hint}"));
            }
            bottom = bottom.max(y + line);
        }
        bottom
    }

    fn channel_slider(&mut self, sf: &mut Surface<'_>, channel: usize, row: Rect) {
        let c = self.hsva;
        let unit = self.ui.scale;
        let value = [c.h, c.s, c.v, c.a][channel];
        let paint = |sf: &mut Surface<'_>, r: Rect| {
            gradient(sf, r, |t| {
                from_hsva(match channel {
                    0 => Hsva {
                        h: t,
                        s: 1.0,
                        v: 1.0,
                        a: 1.0,
                    },
                    1 => Hsva { s: t, a: 1.0, ..c },
                    2 => Hsva { v: t, a: 1.0, ..c },
                    _ => Hsva {
                        v: c.v * (0.25 + t * 0.75),
                        a: 1.0,
                        ..c
                    },
                })
            });
            pikslar_ui::chrome::edge_outline(sf, r, unit);
        };
        let id = ID_COLOR + channel as u32;
        if let Some(Slide { value, started }) = slider(&mut self.ui, sf, id, row, value, paint) {
            let mut next = c;
            match channel {
                0 => next.h = value.min(0.999),
                1 => next.s = value,
                2 => next.v = value,
                _ => next.a = value,
            }
            self.apply_color(next, started);
        }
    }

    /// A scroll bar beside a list that has more rows than it can show. Dragging it scrolls
    /// the list, as the wheel does.
    pub(crate) fn draw_scroll_bar(
        &mut self,
        sf: &mut Surface<'_>,
        which: List,
        list: Rect,
        (first, shown, total): (usize, usize, usize),
    ) {
        let s = self.ui.scale;
        let hit = Rect::new(list.right() + 2 * s, list.y, 6 * s, list.h);
        let id = ID_SCROLL + which as u32;
        if self.ui.naming() {
            let name = match which {
                List::Layers => "Scroll layers",
                List::Palette => "Scroll palette",
                List::Tiles => "Scroll tiles",
            };
            self.ui.name(id, name);
        }
        if let Some(wanted) = scroll_bar(&mut self.ui, sf, id, hit, first, shown, total) {
            self.panel_scroll.set_first(which, wanted);
            self.ui_dirty = true;
        }
        if total > shown {
            self.note_hint(hit, "Drag to scroll");
        }
    }

    /// The wheel over a list scrolls it. Anywhere else it belongs to the timeline.
    pub(crate) fn panel_wheel(&mut self, ticks: i32, x: i32, y: i32) {
        if self.panel_scroll.wheel(ticks, x, y) {
            self.ui_dirty = true;
        } else if !self.matrix_wheel(ticks, x, y) {
            self.timeline_wheel(ticks, x, y);
        }
    }

    /// Draws the active layer's settings at the bottom of the area, returning their top.
    pub(crate) fn draw_layer_details(&mut self, sf: &mut Surface<'_>, area: Rect) -> i32 {
        let s = self.ui.scale;
        let height = 26 * s;
        let bar = pikslar_ui::fit_h(16);
        let top = area.bottom() - height - (bar + 6) * s;
        let layer_index = self.editor.layer;
        let layer = &self.editor.doc().layers()[layer_index];
        let opacity = layer.opacity;
        let row = Rect::new(area.x, top, area.w, bar * s);
        self.note_hint(row, "Layer opacity");
        let value = opacity as f32 / 255.0;
        let label = format!("Opacity {}%", opacity as u32 * 100 / 255);
        let ts = self.ui.text_scale();
        let paint = |sf: &mut Surface<'_>, r: Rect| {
            pikslar_ui::chrome::well(sf, r, s, theme::panel(), false);
            draw_text(
                sf,
                r.x + 6 * s,
                r.y + (r.h - cap_height() * ts) / 2,
                &label,
                theme::muted(),
                ts,
            );
        };
        if let Some(Slide { value, started }) =
            slider(&mut self.ui, sf, ID_LAYER_TOOLS + 10, row, value, paint)
        {
            self.defer(move |a| {
                if started {
                    a.run(StepCmd::EndDrag);
                }
                if let Some(layer) = a.editor.layer_id(layer_index) {
                    let opacity = (value * 255.0).round() as u8;
                    a.run(LayerCmd::SetOpacity { layer, opacity });
                }
            });
        }
        self.draw_layer_buttons(sf, area, top + (bar + 6) * s, height);
        top
    }

    fn draw_layer_buttons(&mut self, sf: &mut Surface<'_>, area: Rect, buttons: i32, height: i32) {
        let s = self.ui.scale;
        let index = self.editor.layer;
        let layer = &self.editor.doc().layers()[index];
        let (blend, mode) = (layer.blend, layer.mode);
        // A mode the document's blending does not draw as itself is marked, and says so.
        let stood_in = !self.editor.doc().blending.modes().contains(&blend);
        let half = (area.w - 4 * s) / 2;
        let blend_rect = Rect::new(area.x, buttons, half, height);
        self.draw_blend_button(sf, blend_rect, (blend, stood_in));
        let mode_rect = Rect::new(area.x + half + 4 * s, buttons, half, height);
        self.draw_mode_button(sf, mode_rect, index, mode);
    }

    /// Opens the menu of blend modes under the button.
    fn draw_blend_button(
        &mut self,
        sf: &mut Surface<'_>,
        r: Rect,
        (blend, stood_in): (BlendMode, bool),
    ) {
        let (label, hint) = match stood_in {
            false => (blend.name().to_string(), "How this layer mixes with those below"),
            true => (
                format!("{}*", blend.name()),
                "An Aseprite mode, drawn as the nearest of Pikslar's here; Aseprite-compatible blending in Canvas size draws it as itself",
            ),
        };
        self.note_hint_named(r, "How this layer mixes with those below", hint);
        if button(&mut self.ui, sf, ID_LAYER_TOOLS + 11, r, &label, false) {
            self.defer(move |a| a.open_blend_menu((r.x, r.bottom())));
        }
    }

    fn draw_mode_button(&mut self, sf: &mut Surface<'_>, r: Rect, index: usize, mode: ColorMode) {
        // The mode it changes to, by its one name; the word alone where "To" does not fit.
        let (label, short, next) = match mode {
            ColorMode::Palette => ("To RGBA", "RGBA", ColorMode::Rgba),
            ColorMode::Rgba => ("To Palette", "Palette", ColorMode::Palette),
        };
        let room = r.w - 4 * self.ui.scale;
        let label = if text_width(label, self.ui.text_scale()) <= room {
            label
        } else {
            short
        };
        self.note_hint(r, "Change this layer between Palette and RGBA");
        if button(&mut self.ui, sf, ID_LAYER_TOOLS + 12, r, label, false) {
            self.defer(move |a| {
                a.set_layer_mode(index, next);
            });
        }
    }
}

/// Said when converting a layer ended its links with cels of other layers.
pub(crate) const LINKS_ENDED: &str = "Converting ended this layer's links to other layers";

impl App {
    /// Converts a layer's color mode, saying so when that ends links with other layers, which
    /// join only layers of one mode.
    pub(crate) fn set_layer_mode(&mut self, index: usize, mode: ColorMode) {
        let Some(layer) = self.editor.layer_id(index) else {
            return;
        };
        let doc = self.editor.doc();
        let shared = doc.shares_with_other_layers(index);
        if self.run(LayerCmd::SetMode { layer, mode }).is_some() && shared {
            self.notify(LINKS_ENDED.to_string());
        }
    }
}

/// As much of a layer's name as twelve typical characters hold.
pub(crate) fn layer_label(name: &str, ts: i32) -> std::borrow::Cow<'_, str> {
    crate::panel_bottom::fit_to_width(name, (12 * em() - 1) * ts, ts)
}

#[cfg(test)]
mod label_tests {
    use super::*;

    #[test]
    fn a_layer_name_is_cut_to_twelve_cells_with_an_ellipsis_in_large_text() {
        crate::test_app::large_text();
        assert_eq!(layer_label("Background layer name", 2), "Backgroun...");
        assert_eq!(layer_label("Layer 1", 2), "Layer 1");
        assert_eq!(
            layer_label("背景のレイヤーです背景のレイヤーです", 2),
            "背景のレ..."
        );
    }

    #[test]
    fn a_layer_name_is_cut_with_an_ellipsis_in_text() {
        let cut = layer_label("Background layer name that is long", 1);
        assert!(cut.ends_with("...") && cut.len() < 20, "{cut}");
        assert_eq!(layer_label("Layer 1", 1), "Layer 1");
    }
}
