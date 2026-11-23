//! The step for an edit of tile pixels: the images the tiles had, swapped for the ones they
//! have each time the step is played.

use super::held::Held;
use super::{Edit, History};
use crate::color::ColorTable;
use crate::document::Document;
use crate::image::Image;
use crate::tilemap::TileEdit;
use crate::tileset::{TileId, TilesetId};
use std::sync::Arc;

impl Edit {
    /// A step that restores these tile images of one tileset, and the colour table from before
    /// it if the edit added colours. History charges it when it is recorded.
    pub fn tile_images(
        tileset: TilesetId,
        before: Box<[(TileId, Arc<Image>)]>,
        colors: Option<Arc<ColorTable>>,
    ) -> Edit {
        Edit::TileImages {
            tileset,
            before,
            colors,
            charge: 0,
        }
    }

    /// What a step of tile images keeps: its list, its colour table if it has one, and the images
    /// only history keeps, which are counted when the step is taken on.
    pub(super) fn tile_images_size(&self) -> usize {
        let Edit::TileImages {
            before,
            colors,
            charge,
            ..
        } = self
        else {
            return 0;
        };
        let list = size_of_val::<[(TileId, Arc<Image>)]>(before);
        list + colors.as_ref().map_or(0, |table| table.kept_bytes()) + charge
    }

    /// Plays the step: each tile takes the image listed for it and the list keeps what the tile
    /// had. Playing it again undoes it.
    pub(super) fn swap_tile_images(&mut self, doc: &mut Document) {
        let Edit::TileImages {
            tileset,
            before,
            colors,
            ..
        } = self
        else {
            return;
        };
        doc.swap_tile_images(*tileset, before);
        if let Some(table) = colors {
            std::mem::swap(table, &mut doc.colors);
        }
    }

    /// Counts what the step keeps in history, and charges it for its share.
    pub(super) fn take_on(&mut self, held: &mut Held) {
        match self {
            Edit::Snapshot(s) => s.take_on(held),
            Edit::TileImages { before, charge, .. } => {
                *charge = held.take_on_images(before.iter().map(|(_, image)| image));
            }
            Edit::Tiles { .. } | Edit::Move { .. } | Edit::Selection(_) => {}
        }
    }

    /// Lets go of what the step keeps, before it is played back or dropped.
    pub(super) fn let_go(&mut self, held: &mut Held) {
        match self {
            Edit::Snapshot(s) => s.let_go(held),
            Edit::TileImages { before, charge, .. } => {
                held.let_go_images(before.iter().map(|(_, image)| image));
                *charge = 0;
            }
            Edit::Tiles { .. } | Edit::Move { .. } | Edit::Selection(_) => {}
        }
    }

    /// Plays a step of tile images and charges it again for the images it keeps afterwards.
    /// Returns what the step is charged now.
    #[inline(never)]
    pub(super) fn play_tile_images(
        &mut self,
        size: usize,
        (held, used): (&mut Held, &mut usize),
        doc: &mut Document,
    ) -> usize {
        self.let_go(held);
        self.swap_tile_images(doc);
        self.take_on(held);
        let now = self.size();
        *used = used.wrapping_add(now).wrapping_sub(size);
        now
    }
}

impl History {
    /// The tiles the step that undo (or, with `redo`, redo) would play changes the pixels of,
    /// if that step is an edit of tile pixels.
    #[inline]
    pub(crate) fn next_tile_edit(&self, redo: bool) -> Option<TileEdit> {
        let list = if redo { &self.redo } else { &self.undo };
        match list.last() {
            Some((Edit::TileImages { .. }, _)) => Self::tile_edit_of(list),
            _ => None,
        }
    }

    /// The tile edit of the newest step of `list`, which is one.
    #[inline(never)]
    fn tile_edit_of(list: &[(Edit, usize)]) -> Option<TileEdit> {
        match list.last()? {
            (
                Edit::TileImages {
                    tileset, before, ..
                },
                _,
            ) => {
                let ids: Vec<TileId> = before.iter().map(|(id, _)| *id).collect();
                Some(TileEdit::new(*tileset, &ids))
            }
            _ => None,
        }
    }
}
