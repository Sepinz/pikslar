//! The document's one table of tilesets.

use super::ids::TilesetId;
use super::set::{clean_name, Tileset, MAX_TILESETS};
use crate::document::{ColorMode, LimitError};

/// Every tileset of a document, in the order they were made. Tilemap layers name them by id.
/// The counter only goes up, so an id in a file, a history step or the clipboard can never meet
/// a different tileset; undoing a change restores the counter with the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tilesets {
    next_id: u32,
    list: Vec<Tileset>,
}

impl Default for Tilesets {
    fn default() -> Self {
        Tilesets {
            next_id: 1,
            list: Vec::new(),
        }
    }
}

impl Tilesets {
    /// The table as a file kept it, with the saved counter for the next id. `None` unless there
    /// are at most [`MAX_TILESETS`], their ids are distinct, from 1 and below the counter.
    pub fn from_parts(next_id: u32, list: Vec<Tileset>) -> Option<Self> {
        let distinct: std::collections::HashSet<u32> =
            list.iter().map(|set| set.id().get()).collect();
        let fits = list.len() <= MAX_TILESETS
            && distinct.len() == list.len()
            && list
                .iter()
                .all(|set| (1..next_id).contains(&set.id().get()));
        fits.then_some(Tilesets { next_id, list })
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Tileset> {
        self.list.iter()
    }

    /// The id the next tileset takes.
    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    pub fn get(&self, id: TilesetId) -> Option<&Tileset> {
        self.list.iter().find(|set| set.id() == id)
    }

    pub(crate) fn get_mut(&mut self, id: TilesetId) -> Option<&mut Tileset> {
        self.list.iter_mut().find(|set| set.id() == id)
    }

    /// Adds an empty tileset with the next id.
    pub(crate) fn add(
        &mut self,
        name: String,
        mode: ColorMode,
        tile_size: (usize, usize),
    ) -> Result<TilesetId, LimitError> {
        if self.list.len() >= MAX_TILESETS {
            return Err(LimitError::TooManyTilesets);
        }
        let id = TilesetId::new(self.next_id);
        let set = Tileset::new(id, name, mode, tile_size)?;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(LimitError::TooManyTilesets)?;
        self.list.push(set);
        Ok(id)
    }

    /// Adds a copy of a tileset under the next id and a new name. Its tiles are the same ids and
    /// images, shared until one is edited.
    pub(crate) fn copy(&mut self, id: TilesetId, name: &str) -> Result<TilesetId, LimitError> {
        if self.list.len() >= MAX_TILESETS {
            return Err(LimitError::TooManyTilesets);
        }
        let source = self.get(id).ok_or(LimitError::NoSuchItem)?;
        let new_id = TilesetId::new(self.next_id);
        let mut set = source.clone();
        set.relabel(new_id, clean_name(name));
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(LimitError::TooManyTilesets)?;
        self.list.push(set);
        Ok(new_id)
    }

    /// Takes a tileset out. The counter does not go back.
    pub(crate) fn remove(&mut self, id: TilesetId) -> bool {
        let before = self.list.len();
        self.list.retain(|set| set.id() != id);
        self.list.len() != before
    }

    /// What the table takes besides the images the tiles hold.
    pub(crate) fn table_bytes(&self) -> usize {
        size_of::<Tilesets>()
            + self.list.capacity() * size_of::<Tileset>()
            + self.list.iter().map(Tileset::table_bytes).sum::<usize>()
    }

    /// What every tile image of every tileset comes to in a file.
    pub fn stored_bytes(&self) -> usize {
        self.list
            .iter()
            .fold(0usize, |sum, set| sum.saturating_add(set.stored_bytes()))
    }

    /// Every tile image, in tileset and panel order.
    pub(crate) fn images(&self) -> impl Iterator<Item = &std::sync::Arc<crate::image::Image>> {
        self.list
            .iter()
            .flat_map(|set| set.tiles().iter().map(|entry| &entry.image))
    }
}

impl<'a> IntoIterator for &'a Tilesets {
    type Item = &'a Tileset;
    type IntoIter = std::slice::Iter<'a, Tileset>;

    fn into_iter(self) -> Self::IntoIter {
        self.list.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(t: &mut Tilesets) -> TilesetId {
        t.add("T".into(), ColorMode::Rgba, (8, 8)).expect("room")
    }

    #[test]
    fn ids_count_up_and_are_not_reused_after_a_removal() {
        let mut t = Tilesets::default();
        let (a, b) = (add(&mut t), add(&mut t));
        assert_eq!((a.get(), b.get()), (1, 2));
        assert!(t.remove(b));
        assert!(!t.remove(b));
        assert_eq!(add(&mut t).get(), 3);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn at_most_64_tilesets_and_a_bad_size_takes_no_id() {
        let mut t = Tilesets::default();
        assert_eq!(
            t.add("x".into(), ColorMode::Rgba, (0, 8)),
            Err(LimitError::TileSize)
        );
        assert_eq!(t.next_id(), 1);
        for _ in 0..MAX_TILESETS {
            add(&mut t);
        }
        assert_eq!(
            t.add("x".into(), ColorMode::Rgba, (8, 8)),
            Err(LimitError::TooManyTilesets)
        );
    }
}
