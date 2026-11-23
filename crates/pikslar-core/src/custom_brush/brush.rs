//! A brush: source pixels, how they paint, and how they are turned.

use crate::document::LimitError;
use crate::floating::Clip;
use crate::orient::Orientation;
use crate::Turn;
use std::sync::Arc;

/// The most pixels a brush has on a side.
pub const MAX_BRUSH_SIDE: usize = 256;

/// How a brush's pixels reach the canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BrushPaint {
    /// The brush's own colours replace the cell wherever the brush pixel is not clear.
    #[default]
    OwnColors,
    /// Only the brush's shape counts, painted in the current colour.
    Shape,
}

/// The point of the brush that sits on the stroke.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Handle {
    /// The centre, by the rule round brushes use (`brush_origin`).
    #[default]
    Centre,
    /// A corner of the source picture: 0 top left, 1 top right, 2 bottom left, 3 bottom right.
    /// It stays on that pixel when the brush is turned.
    Corner(u8),
}

/// Pixels captured from the canvas, with the way they are used. Cloning shares the pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Brush {
    pub(super) name: String,
    pub(super) width: u16,
    pub(super) height: u16,
    pub(super) colors: Arc<[u32]>,
    pub(super) ids: Option<Arc<[u32]>>,
    pub(super) paint: BrushPaint,
    pub(super) orient: Orientation,
    pub(super) handle: Handle,
}

impl Brush {
    /// A brush from straight-alpha colours, row by row, 0 where clear. `ids` are the palette ids
    /// the colours came from, if any. Refused when empty, too large, or inconsistent.
    pub fn from_parts(
        name: &str,
        width: usize,
        height: usize,
        colors: &[u32],
        ids: Option<&[u32]>,
        paint: BrushPaint,
    ) -> Result<Brush, LimitError> {
        if width == 0 || height == 0 || width > MAX_BRUSH_SIDE || height > MAX_BRUSH_SIDE {
            return Err(LimitError::BrushTooLarge);
        }
        let agrees = colors.len() == width * height && ids.is_none_or(|i| i.len() == colors.len());
        if !agrees {
            return Err(LimitError::Dimensions);
        }
        if colors.iter().all(|&c| c == 0) {
            return Err(LimitError::NothingToCapture);
        }
        Ok(Brush {
            name: name.to_owned(),
            width: width as u16,
            height: height as u16,
            colors: colors.into(),
            ids: ids.map(Into::into),
            paint,
            orient: Orientation::IDENTITY,
            handle: Handle::Centre,
        })
    }

    /// A brush from what Copy captured.
    pub fn from_clip(name: &str, clip: &Clip, paint: BrushPaint) -> Result<Brush, LimitError> {
        if !clip.is_well_formed() {
            return Err(LimitError::Dimensions);
        }
        Brush::from_parts(
            name,
            clip.width,
            clip.height,
            &clip.colors,
            clip.ids.as_deref(),
            paint,
        )
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Width and height of the source, before any turn.
    pub fn source_size(&self) -> (usize, usize) {
        (self.width.into(), self.height.into())
    }

    /// Width and height as stamped, after the turn.
    pub fn size(&self) -> (usize, usize) {
        let (w, h) = self.source_size();
        self.orient.turned_size(w, h)
    }

    /// The source colours, row by row.
    pub fn colors(&self) -> &[u32] {
        &self.colors
    }

    /// The palette ids the source came from, if it came from a palette layer.
    pub fn ids(&self) -> Option<&[u32]> {
        self.ids.as_deref()
    }

    pub fn paint(&self) -> BrushPaint {
        self.paint
    }

    pub fn orientation(&self) -> Orientation {
        self.orient
    }

    pub fn handle(&self) -> Handle {
        self.handle
    }

    /// The same brush renamed.
    pub fn with_name(&self, name: &str) -> Brush {
        Brush {
            name: name.to_owned(),
            ..self.clone()
        }
    }

    pub fn with_paint(&self, paint: BrushPaint) -> Brush {
        Brush {
            paint,
            ..self.clone()
        }
    }

    pub fn with_handle(&self, handle: Handle) -> Brush {
        Brush {
            handle,
            ..self.clone()
        }
    }

    /// The same source turned further by `then`. Nothing is resampled.
    pub fn turned(&self, then: Orientation) -> Brush {
        Brush {
            orient: self.orient.compose(then),
            ..self.clone()
        }
    }

    /// The same source after a flip or quarter turn of the whole picture.
    pub fn turned_by(&self, turn: Turn) -> Brush {
        self.turned(match turn {
            Turn::FlipHorizontal => Orientation::flip_horizontal(),
            Turn::FlipVertical => Orientation::flip_vertical(),
            Turn::Clockwise => Orientation::rotate90(),
            Turn::CounterClockwise => Orientation::rotate270(),
        })
    }

    /// Bytes of pixels this brush holds, for the memory estimate.
    pub fn pixel_bytes(&self) -> usize {
        (self.colors.len() + self.ids.as_ref().map_or(0, |i| i.len())) * size_of::<u32>()
    }
}
