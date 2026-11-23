//! Drawing shared by the redesign's widgets (docs/ui-redesign.md 2.5): faces with notched
//! corners and a lip, wells, and the focus ring. Every state differs by shape as well as by
//! colour: chosen is filled, pressed drops the content one unit and loses the lip, focus is a
//! ring outside the control, hover changes the edge.

use crate::icons::{self, Art};
use crate::role::{Role, State};
use crate::theme;
use crate::widgets::{Interaction, Ui};
use pikslar_render::{Rect, Surface};

/// The state a face is drawn in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    pub hover: bool,
    /// Held down with the pointer still on it.
    pub pressed: bool,
    pub chosen: bool,
    pub disabled: bool,
}

impl Look {
    /// The look of a control in `state`, chosen or not. A disabled control answers nothing.
    pub fn of(state: Interaction, chosen: bool, disabled: bool) -> Look {
        Look {
            hover: state.hover && !disabled,
            pressed: state.held && state.hover && !disabled,
            chosen,
            disabled,
        }
    }

    /// The colour of words and icons on the face.
    pub fn ink(self) -> u32 {
        if self.chosen {
            theme::on_accent()
        } else if self.disabled {
            theme::disabled()
        } else {
            theme::text()
        }
    }

    /// How far, in units, the content moves down: one while pressed.
    pub fn drop(self) -> i32 {
        i32::from(self.pressed)
    }

    pub(crate) fn surface(self) -> u32 {
        match (self.chosen, self.pressed, self.hover) {
            (true, true, _) => theme::accent_pressed(),
            (true, false, true) => theme::accent_hover(),
            (true, false, false) => theme::accent(),
            (false, true, _) => theme::pressed(),
            (false, false, true) => theme::hover(),
            (false, false, false) => theme::raised(),
        }
    }
}

/// Paints the four corner units of `r` in `parent`, so the control reads as an object.
pub fn notch(sf: &mut Surface<'_>, r: Rect, s: i32, parent: u32) {
    for (x, y) in [
        (r.x, r.y),
        (r.right() - s, r.y),
        (r.x, r.bottom() - s),
        (r.right() - s, r.bottom() - s),
    ] {
        sf.fill(Rect::new(x, y, s, s), parent);
    }
}

/// A raised face: a one-unit edge, the face inside it, a one-unit lip on the face's top row
/// while raised and not chosen (skipped where the theme makes Lip the face colour), and notched
/// corners in `parent`.
pub fn face(sf: &mut Surface<'_>, r: Rect, s: i32, parent: u32, look: Look) {
    crate::font::note_box(r);
    // One rule for a disabled control: its outline in Disabled and no lip, so it keeps its
    // shape but reads as off beside Muted words.
    let edge = if look.disabled && icons::high_contrast() {
        // GrayText would only change the hue: in high contrast a disabled control has no
        // outline at all, so it differs by shape and never outshines an enabled one.
        look.surface()
    } else if look.disabled {
        theme::disabled()
    } else if look.hover {
        theme::edge_hover()
    } else {
        theme::edge()
    };
    sf.fill(r, edge);
    let inner = r.inset(s);
    let surface = look.surface();
    sf.fill(inner, surface);
    if !look.chosen && !look.pressed && !look.disabled && theme::lip() != surface {
        sf.fill(Rect::new(inner.x, inner.y, inner.w, s), theme::lip());
    }
    notch(sf, r, s, parent);
}

/// The four corner units of `r`, left to right and top to bottom.
fn corner_spots(r: Rect, s: i32) -> [(i32, i32); 4] {
    [
        (r.x, r.y),
        (r.right() - s, r.y),
        (r.x, r.bottom() - s),
        (r.right() - s, r.bottom() - s),
    ]
}

/// What is on the surface under each corner unit of `r`, or 0 where there is no surface.
fn read_corners(sf: &Surface<'_>, r: Rect, s: i32) -> [u32; 4] {
    corner_spots(r, s).map(|(x, y)| {
        if x < 0 || y < 0 || x >= sf.w || y >= sf.h {
            return 0;
        }
        sf.px
            .get(y as usize * sf.w as usize + x as usize)
            .copied()
            .unwrap_or(0)
    })
}

/// Paints the corner units of `r` with what [`read_corners`] found there before.
fn restore_corners(sf: &mut Surface<'_>, r: Rect, s: i32, saved: [u32; 4]) {
    for ((x, y), color) in corner_spots(r, s).into_iter().zip(saved) {
        sf.fill(Rect::new(x, y, s, s), color);
    }
}

/// [`face`] on whatever is already drawn: the notched corners keep the colour that was under
/// them, so no caller has to know its parent's colour.
pub fn face_here(sf: &mut Surface<'_>, r: Rect, s: i32, look: Look) {
    let under = read_corners(sf, r, s);
    face(sf, r, s, 0, look);
    restore_corners(sf, r, s, under);
}

/// The surface of a menu, a dialog or a tooltip: Header with a one-unit Edge outside `r` and
/// notched corners, so it reads as an object lifted off whatever lies behind it.
pub fn popup(sf: &mut Surface<'_>, r: Rect, s: i32) {
    let outer = r.inset(-s);
    let under = read_corners(sf, outer, s);
    sf.fill(outer, theme::edge());
    sf.fill(r, theme::header());
    restore_corners(sf, outer, s, under);
}

/// A well, for fields and value readouts: a one-unit edge, the Well face, notched corners.
pub fn well(sf: &mut Surface<'_>, r: Rect, s: i32, parent: u32, hover: bool) {
    crate::font::note_box(r);
    sf.fill(
        r,
        if hover {
            theme::edge_hover()
        } else {
            theme::edge()
        },
    );
    sf.fill(r.inset(s), theme::well());
    notch(sf, r, s, parent);
}

/// Draws `art` centred in `r` at one pixel of art a unit, in the tones for `look`, dropped
/// while pressed.
pub fn art_in(sf: &mut Surface<'_>, r: Rect, art: Art, look: Look, s: i32) {
    let tones = match (look.chosen, look.disabled) {
        (true, _) => icons::Tones::for_look(icons::Look::Chosen, icons::high_contrast()),
        (false, true) => icons::Tones::one(theme::disabled()),
        (false, false) => icons::Tones::for_look(icons::Look::Normal, icons::high_contrast()),
    };
    let side = art.size() * s;
    let (x, y) = (
        r.x + (r.w - side) / 2,
        r.y + (r.h - side) / 2 + look.drop() * s,
    );
    icons::draw_art(sf, x, y, art, tones, s);
}

/// A button showing `art` on a notched face over `parent`, said to be a button in `state`.
/// Disabled, it ignores the pointer and keys. Returns true when clicked.
pub fn art_button(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    (id, r): (u32, Rect),
    art: Art,
    parent: u32,
    state: State,
) -> bool {
    let i = ui.interact(id, r);
    ui.describe(Role::Button, state);
    let look = Look::of(i, state.chosen, state.disabled);
    let s = ui.scale.max(1);
    face(sf, r, s, parent, look);
    art_in(sf, r, art, look, s);
    ui.ring(sf, id, r);
    i.clicked && !state.disabled
}

/// A mark on a list row that switches something, such as the eye or the lock: no face, only
/// the 8 by 8 `art` in `ink` (Text draws the art's own tones), and a Hover plate under
/// the pointer. It is a button that is `on` or not, called `label` in the layout dump. Returns
/// true when clicked.
pub fn mark_button(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    (id, r): (u32, Rect),
    art: Art,
    (ink, on, label): (u32, bool, &str),
) -> bool {
    let i = ui.interact(id, r);
    ui.describe(Role::Button, State::chosen(on));
    if ui.naming() {
        ui.name_icon(id, label);
    }
    let unit = ui.scale.max(1);
    if i.hover {
        // The plate is the surface in high contrast, so the outline carries the hover.
        sf.fill(r, theme::edge_hover());
        sf.fill(r.inset(unit), theme::hover());
    }
    // An 8-unit mark, a pixel of art a unit: the height of the capitals beside it.
    let side = art.size() * unit;
    let (x, y) = (r.x + (r.w - side) / 2, r.y + (r.h - side) / 2);
    let tones = if ink == theme::text() {
        icons::Tones::for_look(icons::Look::Normal, icons::high_contrast())
    } else {
        icons::Tones::one(ink)
    };
    icons::draw_art(sf, x, y, art, tones, unit);
    ui.ring(sf, id, r);
    i.clicked
}

/// The focus ring: two units of Focus, one unit outside `r`, so a three-unit band round it.
pub fn ring(sf: &mut Surface<'_>, r: Rect, s: i32) {
    let (inner, outer) = (r.inset(-s), r.inset(-3 * s));
    let t = 2 * s;
    let c = theme::focus();
    sf.fill(Rect::new(outer.x, outer.y, outer.w, t), c);
    sf.fill(Rect::new(outer.x, inner.bottom(), outer.w, t), c);
    sf.fill(Rect::new(outer.x, inner.y, t, inner.h), c);
    sf.fill(Rect::new(inner.right(), inner.y, t, inner.h), c);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{SystemColors, Theme};

    fn paint(look: Look) -> Vec<u32> {
        theme::set(Theme::LIGHTBOX);
        let mut px = vec![0u32; 20 * 20];
        let mut sf = Surface {
            px: &mut px,
            w: 20,
            h: 20,
        };
        face(&mut sf, Rect::new(2, 2, 16, 16), 1, 7, look);
        px
    }

    #[test]
    fn a_face_is_notched_edged_and_lipped_only_while_raised() {
        let px = paint(Look::default());
        let at = |x: usize, y: usize| px[y * 20 + x];
        assert_eq!(at(2, 2), 7, "corner in the parent's colour");
        assert_eq!(at(17, 17), 7);
        assert_eq!(at(3, 2), theme::edge());
        assert_eq!(at(9, 3), theme::lip(), "lip under the top edge");
        assert_eq!(at(9, 9), theme::raised());
        let pressed = paint(Look {
            hover: true,
            pressed: true,
            ..Look::default()
        });
        assert_eq!(
            pressed[3 * 20 + 9],
            theme::pressed(),
            "no lip while pressed"
        );
        let chosen = paint(Look {
            chosen: true,
            ..Look::default()
        });
        assert_eq!(
            chosen[3 * 20 + 9],
            theme::accent(),
            "chosen is filled, no lip"
        );
        theme::set(Theme::NORMAL);
    }

    #[test]
    fn the_ring_is_two_units_of_focus_one_unit_outside() {
        theme::set(Theme::LIGHTBOX);
        let mut px = vec![0u32; 30 * 30];
        let mut sf = Surface {
            px: &mut px,
            w: 30,
            h: 30,
        };
        ring(&mut sf, Rect::new(10, 10, 10, 10), 1);
        let at = |x: usize, y: usize| px[y * 30 + x];
        assert_eq!(at(9, 15), 0, "a unit of gap");
        assert_eq!(at(8, 15), theme::focus());
        assert_eq!(at(7, 15), theme::focus());
        assert_eq!(at(6, 15), 0, "two units, no more");
        assert_eq!(at(21, 15), theme::focus());
        assert_eq!(at(15, 22), theme::focus());
        assert_eq!(at(7, 7), theme::focus(), "corners closed");
        assert_eq!(theme::focus(), 0xff4c_c9f0, "cyan in Lightbox");
        theme::set(Theme::NORMAL);
    }

    #[test]
    fn a_face_here_notches_with_whatever_is_under_the_corners() {
        theme::set(Theme::LIGHTBOX);
        let mut px = vec![7u32; 20 * 20];
        px[2 * 20 + 2] = 11; // under the top left corner
        let mut sf = Surface {
            px: &mut px,
            w: 20,
            h: 20,
        };
        face_here(&mut sf, Rect::new(2, 2, 16, 16), 1, Look::default());
        let at = |x: usize, y: usize| px[y * 20 + x];
        assert_eq!(at(2, 2), 11, "the corner shows what was there");
        assert_eq!(at(17, 2), 7);
        assert_eq!(at(17, 17), 7);
        assert_eq!(at(3, 2), theme::edge());
        assert_eq!(at(9, 3), theme::lip(), "the lip, as on any raised face");
        face_here(
            &mut Surface {
                px: &mut [0u32; 4],
                w: 2,
                h: 2,
            },
            Rect::new(-3, -3, 9, 9),
            1,
            Look::default(),
        ); // a face partly off the surface is safe
        theme::set(Theme::NORMAL);
    }

    #[test]
    fn a_popup_is_edged_outside_its_rect_filled_in_header_and_notched() {
        theme::set(Theme::LIGHTBOX);
        let mut px = vec![7u32; 30 * 30];
        let mut sf = Surface {
            px: &mut px,
            w: 30,
            h: 30,
        };
        popup(&mut sf, Rect::new(5, 5, 20, 20), 1);
        let at = |x: usize, y: usize| px[y * 30 + x];
        assert_eq!(at(4, 4), 7, "corner notched");
        assert_eq!(at(25, 25), 7);
        assert_eq!(at(10, 4), theme::edge(), "the edge is outside the rect");
        assert_eq!(at(4, 10), theme::edge());
        assert_eq!(at(5, 10), theme::header());
        assert_eq!(at(24, 24), theme::header());
        assert_eq!(at(3, 10), 7, "nothing further out");
        theme::set(Theme::NORMAL);
    }

    #[test]
    fn a_mark_is_ink_without_a_face_and_a_plate_under_the_pointer() {
        theme::set(Theme::LIGHTBOX);
        let mut ui = Ui::new();
        ui.scale = 1;
        let mut px = vec![7u32; 30 * 30];
        let mut sf = Surface {
            px: &mut px,
            w: 30,
            h: 30,
        };
        ui.begin();
        let r = Rect::new(5, 5, 20, 20);
        let art = icons::MarkIcon::Lock.art();
        let clicked = mark_button(&mut ui, &mut sf, (9, r), art, (theme::text(), true, "Lock"));
        assert!(!clicked);
        assert_eq!(px[5 * 30 + 5], 7, "no face: the corner is the row");
        assert!(px.contains(&theme::text()), "the lock is drawn in ink");
        assert!(!px.contains(&theme::accent()), "never in the chosen signal");
        assert!(ui.widgets().iter().any(|&(wr, id)| id == 9 && wr == r));
        theme::set(Theme::NORMAL);
    }
    #[test]
    fn a_mark_under_the_pointer_is_outlined_where_hover_equals_the_surface() {
        for sys in [&SystemColors::BLACK, &SystemColors::WHITE] {
            theme::set(Theme::high_contrast(sys));
            assert_eq!(
                theme::hover(),
                theme::panel(),
                "no plate shows in high contrast"
            );
            let mut ui = Ui::new();
            ui.scale = 1;
            ui.mouse = (15, 15);
            let mut px = vec![theme::panel(); 30 * 30];
            let mut sf = Surface {
                px: &mut px,
                w: 30,
                h: 30,
            };
            ui.begin();
            let r = Rect::new(5, 5, 20, 20);
            let art = icons::MarkIcon::Lock.art();
            mark_button(&mut ui, &mut sf, (9, r), art, (theme::text(), true, "Lock"));
            for (x, y) in [(5, 15), (24, 15), (15, 5), (15, 24)] {
                assert_eq!(px[y * 30 + x], theme::edge_hover(), "edge at ({x}, {y})");
            }
            assert_eq!(px[15 * 30 + 6], theme::panel(), "inside stays the surface");
        }
        theme::set(Theme::NORMAL);
    }

    #[test]
    fn ink_follows_the_state() {
        theme::set(Theme::LIGHTBOX);
        let chosen = Look {
            chosen: true,
            ..Look::default()
        };
        let off = Look {
            disabled: true,
            ..Look::default()
        };
        assert_eq!(chosen.ink(), theme::on_accent());
        assert_eq!(off.ink(), theme::disabled());
        assert_eq!(Look::default().ink(), theme::text());
        assert_eq!(off.drop(), 0);
        theme::set(Theme::NORMAL);
    }
}

/// In high contrast, a one-unit Edge (WindowText) outline round `r`, for colour swatches and
/// colour bars whose own colour may be the window's. Elsewhere nothing: the colour is the edge.
pub fn colour_outline(sf: &mut Surface<'_>, r: Rect, s: i32) {
    if !icons::high_contrast() {
        return;
    }
    let c = theme::edge();
    sf.fill(Rect::new(r.x, r.y, r.w, s), c);
    sf.fill(Rect::new(r.x, r.bottom() - s, r.w, s), c);
    sf.fill(Rect::new(r.x, r.y, s, r.h), c);
    sf.fill(Rect::new(r.right() - s, r.y, s, r.h), c);
}

#[cfg(test)]
mod colour_outline_tests {
    use super::*;
    use crate::theme::{SystemColors, Theme};

    #[test]
    fn a_colour_swatch_is_outlined_in_high_contrast_only() {
        for (hc, sys) in [(true, &SystemColors::BLACK), (true, &SystemColors::WHITE)] {
            theme::set(Theme::high_contrast(sys));
            icons::set_high_contrast(hc);
            let mut px = vec![0u32; 20 * 20];
            let mut sf = Surface {
                px: &mut px,
                w: 20,
                h: 20,
            };
            colour_outline(&mut sf, Rect::new(2, 2, 10, 10), 1);
            assert_eq!(px[2 * 20 + 5], theme::edge());
            assert_eq!(px[11 * 20 + 11], theme::edge());
            assert_eq!(px[5 * 20 + 5], 0, "the colour itself is left alone");
        }
        theme::set(Theme::NORMAL);
        icons::set_high_contrast(false);
        let mut px = vec![0u32; 400];
        let mut sf = Surface {
            px: &mut px,
            w: 20,
            h: 20,
        };
        colour_outline(&mut sf, Rect::new(2, 2, 10, 10), 1);
        assert!(px.iter().all(|&p| p == 0));
    }
}

#[cfg(test)]
mod disabled_tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn a_disabled_face_has_a_disabled_outline_and_no_lip_and_differs_from_muted() {
        theme::set(Theme::LIGHTBOX);
        let mut px = vec![0u32; 20 * 20];
        let mut sf = Surface {
            px: &mut px,
            w: 20,
            h: 20,
        };
        let look = Look {
            disabled: true,
            ..Look::default()
        };
        face(&mut sf, Rect::new(2, 2, 16, 16), 1, 7, look);
        assert_eq!(px[2 * 20 + 9], theme::disabled());
        assert_ne!(px[3 * 20 + 9], theme::lip());
        assert_ne!(theme::disabled(), theme::muted());
        assert_eq!(look.ink(), theme::disabled());
    }
}

/// A one-unit Edge outline just inside `r`, in every look: for the current colour's preview and
/// colour bars, which may be as dark as the panel they sit on (final review, defect 6).
pub fn edge_outline(sf: &mut Surface<'_>, r: Rect, s: i32) {
    let c = theme::edge();
    sf.fill(Rect::new(r.x, r.y, r.w, s), c);
    sf.fill(Rect::new(r.x, r.bottom() - s, r.w, s), c);
    sf.fill(Rect::new(r.x, r.y, s, r.h), c);
    sf.fill(Rect::new(r.right() - s, r.y, s, r.h), c);
}

#[cfg(test)]
mod edge_outline_tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn a_colour_preview_has_an_edge_in_the_normal_look() {
        theme::set(Theme::LIGHTBOX);
        icons::set_high_contrast(false);
        let mut px = vec![0u32; 400];
        let mut sf = Surface {
            px: &mut px,
            w: 20,
            h: 20,
        };
        edge_outline(&mut sf, Rect::new(2, 2, 10, 10), 1);
        assert_eq!(px[2 * 20 + 5], theme::edge());
        assert_eq!(px[11 * 20 + 11], theme::edge());
        assert_eq!(px[5 * 20 + 5], 0, "the colour itself is left alone");
        theme::set(Theme::NORMAL);
    }
}

#[cfg(test)]
mod hc_disabled_tests {
    use super::*;
    use crate::theme::{SystemColors, Theme};

    /// In high contrast a disabled control loses its outline, so it differs from an enabled
    /// one by shape, not by hue alone, and never stands out more (final review, defect 7).
    #[test]
    fn in_high_contrast_a_disabled_face_has_no_outline() {
        for sys in [&SystemColors::BLACK, &SystemColors::WHITE] {
            theme::set(Theme::high_contrast(sys));
            icons::set_high_contrast(true);
            let top = |look: Look| {
                let mut px = vec![0u32; 400];
                let mut sf = Surface {
                    px: &mut px,
                    w: 20,
                    h: 20,
                };
                face(&mut sf, Rect::new(2, 2, 16, 16), 1, 7, look);
                px[2 * 20 + 9]
            };
            let off = Look {
                disabled: true,
                ..Look::default()
            };
            assert_eq!(top(off), theme::raised(), "no outline");
            assert_eq!(
                top(Look::default()),
                theme::edge(),
                "an enabled one keeps it"
            );
        }
        theme::set(Theme::NORMAL);
        icons::set_high_contrast(false);
    }
}

/// The outline of the button Enter takes: the one-unit edge of `r` in Text, its corners left
/// notched, so it reads as the default without the Accent that means chosen.
pub fn default_edge(sf: &mut Surface<'_>, r: Rect, s: i32) {
    let c = theme::text();
    sf.fill(Rect::new(r.x + s, r.y, r.w - 2 * s, s), c);
    sf.fill(Rect::new(r.x + s, r.bottom() - s, r.w - 2 * s, s), c);
    sf.fill(Rect::new(r.x, r.y + s, s, r.h - 2 * s), c);
    sf.fill(Rect::new(r.right() - s, r.y + s, s, r.h - 2 * s), c);
}
