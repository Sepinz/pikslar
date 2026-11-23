//! What a script asks for and what it gets back, as plain data, so that any interpreter or a
//! stream of JSON lines can produce and read them.
//!
//! Numbers arrive as `i64`, as a script engine or a JSON reader holds them, and are checked here
//! at the boundary: nothing out of range is clamped, it is refused.

/// A layer, by the persisted id the project file keeps for it. Never given to another layer
/// while the document is open, so a removed layer's id is refused, not taken for a new one.
pub type LayerRef = i64;
/// A frame, by its handle: [`DocumentInfo::frames`] lists them, and [`Op::AddFrame`] gives
/// back the new one's. A handle follows its frame wherever frames are added or moved, and is
/// refused once the frame is removed (until undo brings it back). Handles last while the
/// program runs and are not saved: after a project is opened again its frames have new ones.
pub type FrameRef = i64;
/// A palette swatch, by the color id the project file keeps for it.
pub type SwatchRef = i64;

/// How a new layer stores its pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Rgba,
    Palette,
}

/// One script operation of API version 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    // Reading.
    /// The API version: [`Reply::Version`].
    Version,
    /// [`Reply::Document`].
    Document,
    /// [`Reply::Layer`].
    Layer {
        layer: LayerRef,
    },
    /// The color shown by one layer's cel at a canvas pixel, straight-alpha RGBA as
    /// `0xAABBGGRR`: [`Reply::Color`].
    Pixel {
        layer: LayerRef,
        frame: FrameRef,
        x: i64,
        y: i64,
    },
    /// The palette swatches in order: [`Reply::Swatches`].
    Swatches,

    // Layers.
    /// A blank layer above `above`, or at the top for `None`: [`Reply::Created`] with its id.
    AddLayer {
        above: Option<LayerRef>,
        mode: Mode,
        name: Option<String>,
    },
    RemoveLayer {
        layer: LayerRef,
    },
    RenameLayer {
        layer: LayerRef,
        name: String,
    },
    /// Moves a layer to a place counted from the bottom, from 0.
    MoveLayer {
        layer: LayerRef,
        to: i64,
    },
    /// A copy above the layer sharing its drawings: [`Reply::Created`] with its id.
    DuplicateLayer {
        layer: LayerRef,
    },
    SetOpacity {
        layer: LayerRef,
        opacity: i64,
    },
    SetVisible {
        layer: LayerRef,
        visible: bool,
    },
    /// A blend mode by its name, as the layer panel shows it, such as `"Multiply"`.
    SetBlend {
        layer: LayerRef,
        blend: String,
    },

    // Frames.
    /// Inserts a frame at a place from 0 to the frame count; `copy` starts it from the one
    /// before. [`Reply::Frame`] with its handle.
    AddFrame {
        at: i64,
        copy: bool,
    },
    RemoveFrame {
        frame: FrameRef,
    },
    /// Milliseconds, from 10 to 60,000.
    SetDuration {
        frame: FrameRef,
        ms: i64,
    },
    /// A tag over frames `from` to `to`, both included; `to` may not come before `from`.
    AddTag {
        name: String,
        from: FrameRef,
        to: FrameRef,
    },

    // Pixels of one cel: a layer at a frame. Colors are straight-alpha RGBA, `0xAABBGGRR`.
    SetPixel {
        layer: LayerRef,
        frame: FrameRef,
        x: i64,
        y: i64,
        color: i64,
    },
    ErasePixel {
        layer: LayerRef,
        frame: FrameRef,
        x: i64,
        y: i64,
    },
    /// A one-pixel line between two canvas pixels, both included.
    DrawLine {
        layer: LayerRef,
        frame: FrameRef,
        from: (i64, i64),
        to: (i64, i64),
        color: i64,
    },
    /// Fills the area of one color around a pixel, as the fill tool does.
    Fill {
        layer: LayerRef,
        frame: FrameRef,
        x: i64,
        y: i64,
        color: i64,
    },

    /// Writes a block of `width` pixels a row, row by row from its top left corner at (`x`,
    /// `y`): each color that is not 0 replaces the pixel under it, as `SetPixel` does, and 0
    /// leaves it as it is. The block must lie inside the canvas.
    PutPixels {
        layer: LayerRef,
        frame: FrameRef,
        x: i64,
        y: i64,
        width: i64,
        colors: Vec<i64>,
    },
    /// Paints every pixel of a block inside the canvas with one color, or erases them for 0.
    FillRect {
        layer: LayerRef,
        frame: FrameRef,
        x: i64,
        y: i64,
        width: i64,
        height: i64,
        color: i64,
    },
    /// Draws `text` in the built-in 5x7 font with its top left at (`x`, `y`), a canvas pixel, in
    /// a visible `color`. Every font pixel is `scale` canvas pixels square, `scale` from 1 to 8,
    /// and lines break at a newline. At most 256 characters. What runs past the canvas is
    /// clipped, and what the font lacks is drawn as `?`. Hard-edged, as the text tool draws it.
    DrawText {
        layer: LayerRef,
        frame: FrameRef,
        x: i64,
        y: i64,
        text: String,
        color: i64,
        scale: i64,
    },
    /// The colors `Pixel` would read over a block inside the canvas, row by row:
    /// [`Reply::Pixels`].
    Pixels {
        layer: LayerRef,
        frame: FrameRef,
        x: i64,
        y: i64,
        width: i64,
        height: i64,
    },

    // Palette.
    /// [`Reply::Created`] with the swatch's color id.
    AddSwatch {
        color: i64,
    },
    /// Changes what a swatch looks like, everywhere it is used.
    Recolor {
        swatch: SwatchRef,
        color: i64,
    },
    RemoveSwatch {
        swatch: SwatchRef,
    },
}

/// What one layer is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerInfo {
    pub id: u32,
    /// From the bottom, from 0.
    pub index: usize,
    pub name: String,
    /// `"paint"`, `"reference"`, `"group"` or `"tilemap"`.
    pub kind: &'static str,
    pub mode: Mode,
    pub visible: bool,
    pub locked: bool,
    pub opacity: u8,
    pub blend: &'static str,
}

/// What the document is, in outline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentInfo {
    pub width: usize,
    pub height: usize,
    /// Each frame's duration in milliseconds, in order.
    pub durations: Vec<u32>,
    /// Each frame's handle, in order.
    pub frames: Vec<u64>,
    /// Layer ids from the bottom.
    pub layers: Vec<u32>,
}

/// What an operation gave back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Done; nothing to report.
    Done,
    Version(u32),
    Document(DocumentInfo),
    Layer(LayerInfo),
    Color(u32),
    /// `(color id, color)` for each swatch, in palette order.
    Swatches(Vec<(u32, u32)>),
    /// The id of the layer or swatch the operation made.
    Created(u32),
    /// The handle of the frame the operation made.
    Frame(u64),
    /// Colors of a block, row by row, as `0xAABBGGRR`.
    Pixels(Vec<u32>),
}
