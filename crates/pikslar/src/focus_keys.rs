//! The keys that move keyboard focus and act on it, shared by the dialogs.

use crate::app::App;
use pikslar_platform::{Key, Mods};

impl App {
    /// Tab and Shift Tab move focus. Space and Enter act on the focused widget. Returns whether
    /// the key was used; with nothing focused Space and Enter are left to the dialog.
    pub(crate) fn focus_key(&mut self, key: Key, mods: Mods) -> bool {
        let used = match key {
            Key::Tab => {
                self.ui.focus.step(mods.shift);
                true
            }
            Key::Space | Key::Enter => self.ui.focus.request_activate(),
            _ => false,
        };
        self.ui_dirty |= used;
        used
    }

    /// Moves focus to `id`.
    pub(crate) fn focus_on(&mut self, id: u32) {
        self.ui.focus.set(id);
        self.ui_dirty = true;
    }
}

/// Where an arrow key takes focus on a grid of widget ids, one slice a row. Left and Right
/// stay in the row, Up and Down go to the same column of the next row, or the last place of a
/// shorter one. Nothing wraps. None when the key is no arrow or `at` is not on the grid.
pub(crate) fn grid_move(rows: &[&[u32]], at: u32, key: Key) -> Option<u32> {
    let (r, c) = rows
        .iter()
        .enumerate()
        .find_map(|(r, row)| Some((r, row.iter().position(|&id| id == at)?)))?;
    let (nr, nc) = match key {
        Key::Left => (r, c.saturating_sub(1)),
        Key::Right => (r, (c + 1).min(rows[r].len() - 1)),
        Key::Up => (r.saturating_sub(1), c),
        Key::Down => ((r + 1).min(rows.len() - 1), c),
        _ => return None,
    };
    let row = rows[nr];
    Some(row[nc.min(row.len().checked_sub(1)?)])
}

impl App {
    /// An arrow key on the focused widget of a dialog's `grid`. Returns whether it was one.
    pub(crate) fn focus_grid_key(&mut self, key: Key, grid: &[&[u32]]) -> bool {
        let moved = self
            .ui
            .focus
            .focused()
            .and_then(|at| grid_move(grid, at, key));
        if let Some(id) = moved {
            self.focus_on(id);
        }
        moved.is_some()
    }
}

impl App {
    /// In the main window Tab walks every control of the panels. With one focused, Enter and
    /// Space use it and the arrows move focus to the nearest control that way; a focused slider
    /// takes Left and Right, a step each (ten with Shift), and Home and End. Escape or a click
    /// gives focus up, and the keys their usual meaning back. Returns whether the key was used.
    pub(crate) fn slider_key(&mut self, key: Key, mods: Mods) -> bool {
        use pikslar_ui::Nudge;
        if key == Key::Tab {
            return self.focus_key(key, mods);
        }
        let Some(at) = self.ui.focus.focused() else {
            return false;
        };
        let slider = self.ui.focus.is_slider(at);
        let nudge = match key {
            Key::Left if slider => Nudge::Step(if mods.shift { -10 } else { -1 }),
            Key::Right if slider => Nudge::Step(if mods.shift { 10 } else { 1 }),
            Key::Home if slider => Nudge::Start,
            Key::End if slider => Nudge::End,
            Key::Left | Key::Right | Key::Up | Key::Down => {
                self.focus_space_key(key);
                self.ui_dirty = true;
                return true;
            }
            Key::Space | Key::Enter => return self.focus_key(key, mods),
            Key::Escape => {
                self.ui.focus.clear();
                self.ui_dirty = true;
                return true;
            }
            _ => return false,
        };
        self.ui.focus.request_nudge(nudge);
        self.ui_dirty = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: [&[u32]; 3] = [&[1, 2, 3], &[10, 11], &[20]];

    #[test]
    fn arrows_move_on_the_grid_and_stop_at_its_edges() {
        assert_eq!(grid_move(&GRID, 1, Key::Right), Some(2));
        assert_eq!(grid_move(&GRID, 3, Key::Right), Some(3));
        assert_eq!(grid_move(&GRID, 1, Key::Left), Some(1));
        assert_eq!(grid_move(&GRID, 2, Key::Down), Some(11));
        assert_eq!(grid_move(&GRID, 3, Key::Down), Some(11), "a shorter row");
        assert_eq!(grid_move(&GRID, 11, Key::Down), Some(20));
        assert_eq!(grid_move(&GRID, 20, Key::Down), Some(20));
        assert_eq!(grid_move(&GRID, 1, Key::Up), Some(1));
        assert_eq!(grid_move(&GRID, 9, Key::Up), None, "not on the grid");
        assert_eq!(grid_move(&GRID, 1, Key::Enter), None);
    }

    #[test]
    fn an_empty_row_is_no_place_to_move_to_and_does_not_panic() {
        let grid: [&[u32]; 3] = [&[1, 2], &[], &[3]];
        assert_eq!(grid_move(&grid, 1, Key::Down), None);
        assert_eq!(grid_move(&grid, 3, Key::Up), None);
        assert_eq!(grid_move(&grid, 1, Key::Right), Some(2));
    }
}
