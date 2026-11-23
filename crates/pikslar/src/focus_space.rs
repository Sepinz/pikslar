//! Arrow keys by where widgets are drawn, for a dialog whose rows change with its choices.

use crate::app::App;
use pikslar_platform::Key;
use pikslar_render::Rect;

fn center(r: Rect) -> (i32, i32) {
    (r.x + r.w / 2, r.y + r.h / 2)
}

/// The widget an arrow takes focus to from `at`, among `choices`: Left and Right the nearest in
/// the same row, Up and Down the nearest row above or below, and in it the widget closest across.
/// Nothing wraps. None when no widget lies that way, or `at` is not among them.
pub(crate) fn spatial_move(choices: &[(Rect, u32)], at: u32, key: Key) -> Option<u32> {
    let here = choices.iter().find(|(_, id)| *id == at)?.0;
    let (cx, cy) = center(here);
    let same_row = |r: Rect| r.y < here.bottom() && here.y < r.bottom();
    let others = choices.iter().filter(|(_, id)| *id != at);
    match key {
        Key::Left => others
            .filter(|(r, _)| same_row(*r) && center(*r).0 < cx)
            .max_by_key(|(r, _)| center(*r).0),
        Key::Right => others
            .filter(|(r, _)| same_row(*r) && center(*r).0 > cx)
            .min_by_key(|(r, _)| center(*r).0),
        Key::Up | Key::Down => {
            let down = key == Key::Down;
            let ahead = |r: Rect| !same_row(r) && ((center(r).1 > cy) == down);
            let near = others
                .clone()
                .filter(|(r, _)| ahead(*r))
                .map(|(r, _)| (center(*r).1 - cy).abs())
                .min()?;
            others
                .filter(|(r, _)| ahead(*r) && (center(*r).1 - cy).abs() == near)
                .min_by_key(|(r, _)| (center(*r).0 - cx).abs())
        }
        _ => None,
    }
    .map(|&(_, id)| id)
}

impl App {
    /// An arrow key on the focused widget of a dialog, by where the dialog drew its widgets.
    /// Returns whether it moved focus.
    pub(crate) fn focus_space_key(&mut self, key: Key) -> bool {
        let Some(at) = self.ui.focus.focused() else {
            return false;
        };
        let choices: Vec<(Rect, u32)> = self
            .ui
            .widgets()
            .iter()
            .filter(|(_, id)| self.ui.focus.order().contains(id))
            .copied()
            .collect();
        let moved = spatial_move(&choices, at, key);
        if let Some(id) = moved {
            self.focus_on(id);
        }
        moved.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i32, y: i32) -> Rect {
        Rect::new(x, y, 20, 10)
    }

    /// Two rows: 1 2 3, then 4 5 under 1 and 3's column.
    fn rows() -> Vec<(Rect, u32)> {
        vec![
            (at(0, 0), 1),
            (at(30, 0), 2),
            (at(60, 0), 3),
            (at(0, 20), 4),
            (at(60, 20), 5),
        ]
    }

    #[test]
    fn left_and_right_stay_in_the_row_and_stop_at_its_ends() {
        let r = rows();
        assert_eq!(spatial_move(&r, 1, Key::Right), Some(2));
        assert_eq!(spatial_move(&r, 2, Key::Left), Some(1));
        assert_eq!(spatial_move(&r, 3, Key::Right), None);
        assert_eq!(spatial_move(&r, 1, Key::Left), None);
        assert_eq!(spatial_move(&r, 4, Key::Right), Some(5));
    }

    #[test]
    fn up_and_down_take_the_nearest_row_and_the_closest_column() {
        let r = rows();
        assert_eq!(spatial_move(&r, 1, Key::Down), Some(4));
        assert_eq!(spatial_move(&r, 2, Key::Down), Some(4), "closer than 5");
        assert_eq!(spatial_move(&r, 3, Key::Down), Some(5));
        assert_eq!(spatial_move(&r, 5, Key::Up), Some(3));
        assert_eq!(spatial_move(&r, 4, Key::Down), None);
        assert_eq!(spatial_move(&r, 1, Key::Up), None);
        assert_eq!(spatial_move(&r, 9, Key::Up), None);
    }
}
