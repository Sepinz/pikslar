//! Palette workflows in the random runs: reordering, sorting, groups, ramps, the three lossless
//! remaps, Replace colour and loading a palette file's colours, mixed with everything else. Each
//! is one step or nothing and undoes and redoes to the very document it made. A remap must keep
//! every composited frame, every layer, every palette tile and every session brush showing the
//! same colours. Now and then the document is saved and opened again, which must lose nothing.

use super::{bad, Model};
use crate::go;
use pikslar_core::{
    rgba, ColorMode, Damage, Document, Editor, LayerKind, LoadHow, PaletteCmd, RemapKind, SortBy,
};
use pikslar_render::Compositor;
use std::sync::Arc;

const KINDS: [RemapKind; 3] = [
    RemapKind::ToSwatchOrder,
    RemapKind::MergeDuplicates,
    RemapKind::RemoveUnused,
];
const SORTS: [SortBy; 3] = [SortBy::Hue, SortBy::Lightness, SortBy::Usage];
const LOADS: [LoadHow; 3] = [LoadHow::Append, LoadHow::Replace, LoadHow::AsColors];

/// What a document looks like, part by part, as colours rather than ids.
#[derive(PartialEq)]
struct Looks {
    frames: Vec<Vec<u32>>,
    cels: Vec<Vec<u32>>,
    tiles: Vec<Vec<u32>>,
}

impl Model {
    /// One palette action, chosen at random.
    pub(super) fn palette_step(&mut self) -> Result<(), String> {
        let pushes = self.editor.history().pushes();
        self.one_palette_action()?;
        self.counts.palette += u64::from(self.editor.history().pushes() != pushes);
        Ok(())
    }

    fn one_palette_action(&mut self) -> Result<(), String> {
        match self.rng.below(16) {
            0 => self.edit("moving a swatch", Self::move_swatch),
            1 => self.edit("sorting the swatches", |m| {
                let by = SORTS[m.rng.below(SORTS.len())];
                go(&mut m.editor, PaletteCmd::Sort { by });
            }),
            2 | 3 => self.edit("palette groups", Self::group_step),
            4 => self.edit("a ramp", Self::ramp),
            5..=7 => self.remap(),
            8 | 9 => self.edit("replacing a colour", Self::replace),
            10 | 11 => self.load(),
            12 => self.edit("a duplicate colour", Self::duplicate_color),
            13 | 14 => self.palette_tiles(),
            _ => self.save_and_reload(),
        }
    }

    fn pick_swatch(&mut self) -> Option<u32> {
        let swatches = self.editor.doc().colors.swatches();
        match swatches.len() {
            0 => None,
            n => Some(swatches[self.rng.below(n)]),
        }
    }

    /// Any id the table has, transparent included now and then.
    fn pick_id(&mut self) -> u32 {
        self.rng.below(self.editor.doc().colors.len()) as u32
    }

    fn move_swatch(&mut self) {
        let n = self.editor.doc().colors.swatches().len().max(1);
        let (from, to) = (self.rng.below(n + 1), self.rng.below(n + 1));
        go(&mut self.editor, PaletteCmd::MoveSwatch { from, to });
    }

    fn group_step(&mut self) {
        let groups = self.editor.doc().colors.groups().len();
        let group = self.rng.below(groups.max(1) + 1);
        let id = self.pick_swatch().unwrap_or(1);
        let command = match self.rng.below(5) {
            0 => {
                let swatches = self.editor.doc().colors.swatches().to_vec();
                let from = self.rng.below(swatches.len().max(1));
                let take = 1 + self.rng.below(4);
                let ids: Arc<[u32]> = swatches.iter().copied().skip(from).take(take).collect();
                PaletteCmd::AddGroup {
                    name: format!("g{}", self.rng.below(9)),
                    ids,
                }
            }
            1 => PaletteCmd::RenameGroup {
                group,
                name: format!("n{}", self.rng.below(9)),
            },
            2 => PaletteCmd::RemoveGroup { group },
            3 => PaletteCmd::AddToGroup { group, id },
            _ => PaletteCmd::RemoveFromGroup { group, id },
        };
        go(&mut self.editor, command);
    }

    fn ramp(&mut self) {
        let (Some(from), Some(to)) = (self.pick_swatch(), self.pick_swatch()) else {
            return;
        };
        let steps = self.rng.below(18) as u32;
        go(&mut self.editor, PaletteCmd::AddRamp { from, to, steps });
    }

    /// Gives a swatch the colour another id has, so that merging has work to do.
    fn duplicate_color(&mut self) {
        let Some(id) = self.pick_swatch() else {
            return;
        };
        let other = self.pick_id();
        let color = self.editor.doc().colors.color(other);
        go(&mut self.editor, PaletteCmd::Recolor { id, color });
    }

    fn replace(&mut self) {
        let len = self.editor.doc().colors.len() as u32;
        let from = self.pick_id().max(u32::from(len > 1));
        let to = self.pick_id();
        let layer = match self.rng.below(3) {
            0 => self.editor.layer_id(self.editor.layer),
            _ => None,
        };
        go(
            &mut self.editor,
            PaletteCmd::ReplaceColor { from, to, layer },
        );
    }

    /// One of the three remaps, which must change no colour anywhere.
    fn remap(&mut self) -> Result<(), String> {
        let kind = KINDS[self.rng.below(KINDS.len())];
        let before = looks(&mut self.editor);
        let brushes = brush_colors(&self.editor);
        self.edit("a remap", |m| go(&mut m.editor, PaletteCmd::Remap(kind)))?;
        if looks(&mut self.editor) != before {
            return bad(format!("{kind:?} changed what the document shows"));
        }
        let after = brush_colors(&self.editor);
        let kept = |(was, now): (&u32, &u32)| *now == 0 || was == now;
        if brushes.len() != after.len() || !brushes.iter().zip(&after).all(kept) {
            return bad(format!("{kind:?} made a session brush show other colours"));
        }
        self.counts.remaps += 1;
        Ok(())
    }

    /// A palette file's colours, loaded one of the three ways. Load as colours gets a list as
    /// long as the swatches most of the time, so the swap itself is checked.
    fn load(&mut self) -> Result<(), String> {
        let how = LOADS[self.rng.below(LOADS.len())];
        let swatches = self.editor.doc().colors.swatches().len();
        let count = match (how, self.rng.below(4)) {
            (LoadHow::AsColors, 0) | (_, 0) => self.rng.below(swatches + 3),
            (LoadHow::AsColors, _) => swatches,
            _ => 1 + self.rng.below(12),
        };
        let colors: Arc<[u32]> = (0..count).map(|_| self.file_color()).collect();
        let pushes = self.editor.history().pushes();
        let command = PaletteCmd::Load {
            colors: colors.clone(),
            how,
        };
        self.edit("loading a palette", |m| go(&mut m.editor, command))?;
        if self.editor.history().pushes() == pushes {
            return Ok(());
        }
        self.counts.loads += 1;
        let table = &self.editor.doc().colors;
        let shown: Vec<u32> = table.swatches().iter().map(|&id| table.color(id)).collect();
        if how == LoadHow::AsColors
            && shown[..] != colors.iter().map(|&c| opaque(c)).collect::<Vec<_>>()
        {
            return bad("Load as colours did not give swatch i colour i".to_owned());
        }
        Ok(())
    }

    /// A colour as a palette file gives it: sometimes one the table already has, sometimes
    /// with an alpha the file formats cannot hold.
    fn file_color(&mut self) -> u32 {
        if self.rng.chance(3) {
            let id = self.pick_id();
            return self.editor.doc().colors.color(id);
        }
        let n = self.rng.next_u64();
        rgba(
            n as u8 & 0xf0,
            (n >> 8) as u8,
            (n >> 16) as u8,
            (n >> 24) as u8,
        )
    }

    /// Saves and opens the document again, which must give it back exactly, groups and
    /// tilesets included.
    fn save_and_reload(&mut self) -> Result<(), String> {
        self.check_save()?;
        self.counts.saves += 1;
        Ok(())
    }
}

/// What the importers do to a colour: the formats have no alpha.
fn opaque(color: u32) -> u32 {
    let [r, g, b, _] = color.to_le_bytes();
    rgba(r, g, b, 255)
}

/// Every frame composited, then every cel and every tile as the colours it shows.
fn looks(editor: &mut Editor) -> Looks {
    let doc = editor.doc();
    let mut compositor = Compositor::new(doc.width, doc.height);
    let keep = editor.frame;
    let frames = (0..editor.doc().frames.len())
        .map(|frame| {
            editor.frame = frame;
            compositor.update(editor, Damage::All, &mut Vec::new());
            compositor.pixels().to_vec()
        })
        .collect();
    editor.frame = keep;
    let doc = editor.doc();
    Looks {
        frames,
        cels: cel_colors(doc),
        tiles: tile_colors(doc),
    }
}

/// Each cel's pixels on and around the canvas, as colours. A map's cells are not colours, and
/// a remap must leave them as they are.
fn cel_colors(doc: &Document) -> Vec<Vec<u32>> {
    let (w, h) = (doc.width as i32, doc.height as i32);
    let mut out = Vec::new();
    for layer in doc.layers() {
        let palette = layer.mode == ColorMode::Palette;
        let map = matches!(layer.kind, LayerKind::Tilemap(_));
        for cel in layer.cels() {
            let around = (-4..h + 4).flat_map(|y| (-4..w + 4).map(move |x| (x, y)));
            let shown = around.map(|(x, y)| match (map, palette) {
                (false, true) => doc.colors.color(cel.get(x, y)),
                _ => cel.get(x, y),
            });
            out.push(shown.collect());
        }
    }
    out
}

fn tile_colors(doc: &Document) -> Vec<Vec<u32>> {
    let mut out = Vec::new();
    for set in doc.tilesets.iter() {
        let (w, h) = set.tile_size();
        for entry in set.tiles() {
            let all = (0..h as i32).flat_map(|y| (0..w as i32).map(move |x| (x, y)));
            let shown = all.map(|(x, y)| match set.mode() {
                ColorMode::Palette => doc.colors.color(entry.image.get(x, y)),
                ColorMode::Rgba => entry.image.get(x, y),
            });
            out.push(shown.collect());
        }
    }
    out
}

/// The colour each id of each session brush shows now, 0 where it shows none.
fn brush_colors(editor: &Editor) -> Vec<u32> {
    let table = &editor.doc().colors;
    let state = editor.brushes();
    let brushes = state.active_custom().into_iter().chain(state.recent());
    brushes
        .filter_map(|brush| brush.ids())
        .flat_map(|ids| ids.iter().map(|&id| table.color(id)))
        .collect()
}
