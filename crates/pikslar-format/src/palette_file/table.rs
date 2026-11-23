//! Between palette files and a document's colours: what an export writes, and the list of
//! colours a load hands to `PaletteCmd::Load`.

use super::PaletteData;
use pikslar_core::ColorTable;

/// The swatches of a palette as a file holds them, and how many are partly clear. The formats have
/// no alpha, so those are written as their colour alone; the caller says so in a note.
pub struct Exported {
    pub palette: PaletteData,
    pub partly_clear: usize,
}

/// The swatches in the order shown, named by nothing.
pub fn export_swatches(table: &ColorTable) -> Exported {
    let mut partly_clear = 0;
    let colors = table
        .swatches()
        .iter()
        .map(|&id| {
            let [r, g, b, a] = table.color(id).to_le_bytes();
            partly_clear += usize::from(a != 255);
            [r, g, b, a]
        })
        .collect();
    Exported {
        palette: PaletteData::from_colors(colors),
        partly_clear,
    }
}

impl PaletteData {
    /// The colours as the straight-alpha cells a document holds, each opaque (files have no alpha).
    pub fn packed(&self) -> Vec<u32> {
        self.colors
            .iter()
            .map(|&[r, g, b, _]| u32::from_le_bytes([r, g, b, 255]))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::rgba;

    #[test]
    fn swatches_are_exported_in_order_and_partly_clear_ones_are_counted() {
        let mut table = ColorTable::new();
        for color in [rgba(1, 2, 3, 255), rgba(9, 9, 9, 128), rgba(4, 5, 6, 255)] {
            table.add_swatch(color).expect("room");
        }
        table.move_swatch(0, 2);
        let out = export_swatches(&table);
        assert_eq!(out.partly_clear, 1);
        let rgb: Vec<[u8; 3]> = out
            .palette
            .colors
            .iter()
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        assert_eq!(rgb, [[9, 9, 9], [4, 5, 6], [1, 2, 3]]);
    }

    #[test]
    fn a_table_without_swatches_exports_nothing() {
        let out = export_swatches(&ColorTable::new());
        assert!(out.palette.colors.is_empty());
    }

    #[test]
    fn packed_colours_are_opaque_cells() {
        let data = PaletteData::from_colors(vec![[1, 2, 3, 0], [255, 0, 0, 7]]);
        assert_eq!(data.packed(), [rgba(1, 2, 3, 255), rgba(255, 0, 0, 255)]);
    }
}
