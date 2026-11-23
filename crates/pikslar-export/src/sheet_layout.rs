//! Where a sheet's frames go, and how its description lists them.

use pikslar_core::{LimitError, Tag, MAX_DIMENSION};

/// How a sheet's frames are laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    /// Frames a row, in frame order, tags or not. None: a row for each tag, wrapping long rows.
    /// Zero is taken as one.
    pub columns: Option<usize>,
    /// Clear pixels between neighbouring frames, as exported. None round the edge.
    pub padding: usize,
}

/// How a description lists its frames. Both are read by Phaser; Pixi reads only a hash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JsonShape {
    /// `"frames":[{"filename":...,"frame":...}]`, as sheets have always been written.
    #[default]
    Array,
    /// `"frames":{"name":{"frame":...}}`.
    Hash,
}

/// Everything about a sheet that is chosen rather than taken from the document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SheetOptions {
    pub layout: Layout,
    pub json: JsonShape,
}

/// Where each frame goes in a sheet.
#[derive(Debug, PartialEq, Eq)]
pub struct Sheet {
    pub width: usize,
    pub height: usize,
    pub places: Vec<(usize, usize)>,
}

/// Lays `frames` frames of `w` by `h` out in rows. Without columns each tag gets a row of its
/// own and long rows wrap; with them, rows are that many frames wide. A sheet more than
/// [`MAX_DIMENSION`] a side is refused.
#[inline(never)]
pub fn arrange(
    (w, h): (usize, usize),
    frames: usize,
    tags: &[Tag],
    layout: Layout,
) -> Result<Sheet, LimitError> {
    let pad = layout.padding;
    if pad > MAX_DIMENSION {
        return Err(LimitError::Dimensions);
    }
    let per_row = match layout.columns {
        Some(columns) => columns.max(1),
        None => ((MAX_DIMENSION + pad) / (w + pad).max(1)).max(1),
    };
    let starts = match layout.columns {
        Some(_) => vec![false; frames],
        None => row_starts(frames, tags),
    };
    let (mut col, mut row, mut widest) = (0, 0, 0);
    let mut places = Vec::with_capacity(frames);
    for (frame, &starts_row) in starts.iter().enumerate() {
        if frame > 0 && (col == per_row || starts_row) {
            row += 1;
            col = 0;
        }
        places.push((col * (w + pad), row * (h + pad)));
        col += 1;
        widest = widest.max(col);
    }
    let span = |count: usize, side: usize| count * side + count.saturating_sub(1) * pad;
    let (width, height) = (span(widest, w), span(row + 1, h));
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(LimitError::Dimensions);
    }
    Ok(Sheet {
        width,
        height,
        places,
    })
}

/// For each frame, whether a tag starts or ends just before it.
fn row_starts(frames: usize, tags: &[Tag]) -> Vec<bool> {
    let mut starts = vec![false; frames];
    for tag in tags {
        for edge in [tag.from, tag.to.saturating_add(1)] {
            if let Some(start) = starts.get_mut(edge) {
                *start = true;
            }
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::Direction;

    fn tag(name: &str, from: usize, to: usize) -> Tag {
        Tag {
            name: name.to_string(),
            from,
            to,
            direction: Direction::Forward,
            repeat: 0,
            id: pikslar_core::TagId::fresh(),
        }
    }

    const ROWS: Layout = Layout {
        columns: None,
        padding: 0,
    };

    fn grid(columns: usize, padding: usize) -> Layout {
        Layout {
            columns: Some(columns),
            padding,
        }
    }

    #[test]
    fn without_tags_frames_sit_in_one_row() {
        let sheet = arrange((16, 16), 3, &[], ROWS).expect("fits");
        assert_eq!((sheet.width, sheet.height), (48, 16));
        assert_eq!(sheet.places, [(0, 0), (16, 0), (32, 0)]);
    }

    #[test]
    fn each_tag_starts_a_row_and_frames_after_it_another() {
        let sheet = arrange((10, 10), 6, &[tag("a", 1, 2), tag("b", 3, 3)], ROWS).expect("fits");
        assert_eq!(
            sheet.places,
            [(0, 0), (0, 10), (10, 10), (0, 20), (0, 30), (10, 30)]
        );
        assert_eq!((sheet.width, sheet.height), (20, 40));
    }

    #[test]
    fn long_animations_wrap_instead_of_failing() {
        let sheet = arrange((4096, 8), 5, &[], ROWS).expect("fits");
        assert_eq!(sheet.places[1], (4096, 0));
        assert_eq!(sheet.places[2], (0, 8));
        assert_eq!(sheet.places[4], (0, 16));
        assert_eq!(sheet.width, 8192);
    }

    #[test]
    fn a_sheet_too_tall_for_any_image_is_refused() {
        assert_eq!(
            arrange((8192, 8192), 2, &[], ROWS),
            Err(LimitError::Dimensions)
        );
    }

    #[test]
    fn columns_fill_rows_in_frame_order_whatever_the_tags() {
        let sheet = arrange((10, 6), 5, &[tag("a", 1, 2)], grid(2, 0)).expect("fits");
        assert_eq!(sheet.places, [(0, 0), (10, 0), (0, 6), (10, 6), (0, 12)]);
        assert_eq!((sheet.width, sheet.height), (20, 18));
    }

    #[test]
    fn more_columns_than_frames_make_one_row_no_wider_than_the_frames() {
        let sheet = arrange((10, 6), 3, &[], grid(8, 0)).expect("fits");
        assert_eq!((sheet.width, sheet.height), (30, 6));
    }

    #[test]
    fn zero_columns_are_one() {
        let sheet = arrange((10, 6), 2, &[], grid(0, 0)).expect("fits");
        assert_eq!(sheet.places, [(0, 0), (0, 6)]);
    }

    #[test]
    fn padding_goes_between_frames_and_not_round_the_edge() {
        let sheet = arrange((10, 6), 5, &[], grid(2, 3)).expect("fits");
        assert_eq!(sheet.places, [(0, 0), (13, 0), (0, 9), (13, 9), (0, 18)]);
        assert_eq!((sheet.width, sheet.height), (23, 24));
        let one = arrange((10, 6), 1, &[], grid(2, 3)).expect("fits");
        assert_eq!((one.width, one.height), (10, 6));
    }

    #[test]
    fn padding_between_tag_rows_too() {
        let layout = Layout {
            columns: None,
            padding: 2,
        };
        let sheet = arrange((4, 4), 3, &[tag("a", 1, 2)], layout).expect("fits");
        assert_eq!(sheet.places, [(0, 0), (0, 6), (6, 6)]);
        assert_eq!((sheet.width, sheet.height), (10, 10));
    }

    #[test]
    fn padded_rows_wrap_where_the_next_frame_and_its_gap_would_not_fit() {
        let layout = Layout {
            columns: None,
            padding: 2,
        };
        // 4 frames of 2047 and 3 gaps of 2 are 8194: the fourth wraps.
        let sheet = arrange((2047, 8), 4, &[], layout).expect("fits");
        assert_eq!(sheet.places[2], (2 * 2049, 0));
        assert_eq!(sheet.places[3], (0, 10));
        assert_eq!(sheet.width, 3 * 2047 + 2 * 2);
        // 4 of 2046 and 3 gaps of 2 are 8190: one row.
        let fits = arrange((2046, 8), 4, &[], layout).expect("fits");
        assert_eq!((fits.width, fits.height), (8190, 8));
    }

    #[test]
    fn a_layout_wider_or_taller_than_an_image_may_be_is_refused() {
        let wide = arrange((4096, 8), 3, &[], grid(3, 0));
        assert_eq!(wide, Err(LimitError::Dimensions));
        assert!(arrange((4096, 8), 2, &[], grid(2, 0)).is_ok());
        assert_eq!(
            arrange((4096, 8), 2, &[], grid(2, 1)),
            Err(LimitError::Dimensions)
        );
        let tall = arrange((8, 4096), 2, &[], grid(1, 1));
        assert_eq!(tall, Err(LimitError::Dimensions));
        let huge = Layout {
            columns: None,
            padding: usize::MAX,
        };
        assert_eq!(arrange((8, 8), 2, &[], huge), Err(LimitError::Dimensions));
    }
}
