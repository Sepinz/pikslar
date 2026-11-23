//! The state an Aseprite file is read into, a bounds-checked byte cursor, and the layer and
//! tag chunks. Written from the public file specification only; see [`crate::ase_read`].

use crate::ase_cel::Budget;
use crate::ase_names::{tidy, Name};
use crate::ase_read::{AseWarning, Tally, Warnings};
use crate::FormatError;
use pikslar_core::command::MAX_LAYER_NAME;
use pikslar_core::document::MAX_LAYERS;
use pikslar_core::groups::MAX_GROUP_DEPTH;
use pikslar_core::{
    BlendMode, Cel, Direction, Slice, Tag, TagId, MAX_TAGS, MAX_TAG_NAME, MAX_TAG_REPEAT,
};
use std::borrow::Cow;

const CUT_SHORT: FormatError = FormatError::Corrupt("a chunk is cut short");

/// A cursor over bytes that never reads past the end: every read says if it could not.
pub(crate) struct Bytes<'a>(pub &'a [u8]);

impl<'a> Bytes<'a> {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn take(&mut self, count: usize) -> Result<&'a [u8], FormatError> {
        let (head, rest) = self.0.split_at_checked(count).ok_or(CUT_SHORT)?;
        self.0 = rest;
        Ok(head)
    }

    pub fn skip(&mut self, count: usize) -> Result<(), FormatError> {
        self.take(count).map(|_| ())
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], FormatError> {
        self.take(N)?.try_into().map_err(|_| CUT_SHORT)
    }

    pub fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(self.array::<1>()?[0])
    }

    pub fn u16(&mut self) -> Result<u16, FormatError> {
        self.array().map(u16::from_le_bytes)
    }

    pub fn i16(&mut self) -> Result<i16, FormatError> {
        self.array().map(i16::from_le_bytes)
    }

    pub fn i32(&mut self) -> Result<i32, FormatError> {
        self.array().map(i32::from_le_bytes)
    }

    pub fn u32(&mut self) -> Result<u32, FormatError> {
        self.array().map(u32::from_le_bytes)
    }

    /// A `STRING`: a 16-bit length, then that many UTF-8 bytes. Bad UTF-8 is replaced, and the
    /// text is then an owned `Cow` where valid text is borrowed.
    pub fn text(&mut self) -> Result<Cow<'a, str>, FormatError> {
        let length = usize::from(self.u16()?);
        Ok(String::from_utf8_lossy(self.take(length)?))
    }

    /// A `STRING` as [`Bytes::text`], for where it does not matter if it was repaired.
    pub fn string(&mut self) -> Result<String, FormatError> {
        self.text().map(Cow::into_owned)
    }
}

/// The color depth of the file.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Depth {
    Rgba,
    Gray,
    Indexed,
}

impl Depth {
    /// Bytes one pixel takes.
    pub fn bytes(self) -> u64 {
        match self {
            Depth::Rgba => 4,
            Depth::Gray => 2,
            Depth::Indexed => 1,
        }
    }
}

/// What a layer chunk made of a layer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Image,
    Group,
    /// A layer of tiles, naming a tileset by its number in the file.
    Tilemap,
    /// Counted in the file's numbering but not brought in.
    Skipped,
}

pub(crate) struct AseLayer {
    pub name: String,
    /// The child level, 0 to [`MAX_GROUP_DEPTH`].
    pub level: u8,
    pub kind: Kind,
    pub visible: bool,
    pub locked: bool,
    pub folded: bool,
    pub background: bool,
    /// The tileset a tilemap layer names, by the number the file gives it; 0 on other layers.
    pub tileset: u32,
    pub opacity: u8,
    pub blend: BlendMode,
    /// One slot per frame, left `None` where the file has no cel.
    pub cels: Vec<Option<Cel>>,
    /// One per frame: 0, or the number of the hold the frame is in.
    pub holds: Vec<u32>,
    /// One per frame: the opacity the frame's cel was stored with, 255 where it has none.
    pub opacities: Vec<u8>,
}

/// Everything read so far from one file, and what to ask whether to stop reading it.
pub(crate) struct Sprite<'s> {
    pub width: usize,
    pub height: usize,
    pub depth: Depth,
    pub frames: usize,
    pub flags: u32,
    /// The deprecated header speed: the length of a frame that gives none.
    pub speed: u32,
    pub transparent: u8,
    pub layers: Vec<AseLayer>,
    pub kept: usize,
    pub palette: Option<Vec<u32>>,
    pub old_palette: Vec<u32>,
    pub tags: Vec<Tag>,
    pub slices: Vec<Slice>,
    /// The tilesets the file holds, in its order, at most
    /// [`crate::ase_tileset::MAX_KEPT_TILESETS`].
    pub tilesets: Vec<crate::ase_tileset::AseTileset>,
    /// Where each tileset number is in `tilesets`, so chunks and cels find theirs at once.
    pub tileset_at: std::collections::HashMap<u32, usize>,
    /// How many of `tilesets` a document can take.
    pub usable_tilesets: usize,
    pub durations: Vec<u32>,
    /// What the cels may still ask for in all.
    pub budget: Budget,
    /// The highest index an indexed pixel holds, apart from the transparent one.
    pub highest: Option<u8>,
    pub warnings: Warnings,
    /// Asked while a picture is unpacked; a true answer stops the read.
    pub stop: &'s dyn Fn() -> bool,
}

/// The stop of a read nobody stops.
fn never() -> bool {
    false
}

/// Header flag: layer opacity has a valid value.
const LAYER_OPACITY_VALID: u32 = 1;
/// Header flag: blend mode and opacity are valid for groups.
const GROUP_SETTINGS_VALID: u32 = 2;

/// The most layers a file may number, brought in or not: what a cel's layer index can reach.
const MAX_NUMBERED_LAYERS: usize = 1 << 16;

/// What a drawn layer holds for each frame while the file is read: its cel, hold and opacity.
pub(crate) const FRAME_SLOT_BYTES: usize = std::mem::size_of::<Option<Cel>>() + 4 + 1;

const VISIBLE: u16 = 1;
const EDITABLE: u16 = 2;
const BACKGROUND: u16 = 8;
const COLLAPSED: u16 = 32;
const REFERENCE: u16 = 64;
/// The layer type of a tilemap layer.
const TILEMAP_LAYER: u16 = 2;

/// The mode a blend mode numbered as in the file is, and whether it is known. Every one of
/// Aseprite's nineteen is; a document that uses one other than Normal is given
/// Aseprite-compatible blending ([`crate::ase_blending`]), which draws each as itself.
pub(crate) fn blend_for(number: u16) -> (BlendMode, bool) {
    match number {
        0 => (BlendMode::Normal, true),
        1 => (BlendMode::Multiply, true),
        2 => (BlendMode::Screen, true),
        3 => (BlendMode::Overlay, true),
        4 => (BlendMode::Darken, true),
        5 => (BlendMode::Lighten, true),
        6 => (BlendMode::ColorDodge, true),
        7 => (BlendMode::ColorBurn, true),
        8 => (BlendMode::HardLight, true),
        9 => (BlendMode::SoftLight, true),
        10 => (BlendMode::Difference, true),
        11 => (BlendMode::Exclusion, true),
        12 => (BlendMode::Hue, true),
        13 => (BlendMode::Saturation, true),
        14 => (BlendMode::Color, true),
        15 => (BlendMode::Luminosity, true),
        16 => (BlendMode::Addition, true),
        17 => (BlendMode::Subtract, true),
        18 => (BlendMode::Divide, true),
        _ => (BlendMode::Normal, false),
    }
}

impl Sprite<'_> {
    /// A sprite with nothing read yet, which may ask for no more than `budget` in all.
    pub fn new(width: usize, height: usize, depth: Depth, frames: usize, budget: Budget) -> Self {
        Sprite {
            width,
            height,
            depth,
            frames,
            flags: 0,
            speed: 0,
            transparent: 0,
            layers: Vec::new(),
            kept: 0,
            palette: None,
            old_palette: Vec::new(),
            tags: Vec::new(),
            slices: Vec::new(),
            tilesets: Vec::new(),
            tileset_at: std::collections::HashMap::new(),
            usable_tilesets: 0,
            durations: Vec::new(),
            budget,
            highest: None,
            warnings: Warnings::default(),
            stop: &never,
        }
    }

    /// The deepest a layer coming next may be: one past the last if that is a group.
    fn open_level(&self) -> u16 {
        self.layers.last().map_or(0, |last| {
            u16::from(last.level) + u16::from(last.kind == Kind::Group)
        })
    }

    /// Tells of a name that was repaired, and gives its text.
    pub(crate) fn note_name(&mut self, name: Name) -> String {
        if name.lossy {
            self.warnings.tally(Tally::LossyName, &name.text, 0);
        }
        if name.cleaned {
            self.warnings.tally(Tally::CleanedName, &name.text, 0);
        }
        name.text
    }

    /// A layer chunk (0x2004).
    pub fn layer_chunk(&mut self, data: &[u8]) -> Result<(), FormatError> {
        let mut r = Bytes(data);
        let flags = r.u16()?;
        let kind = r.u16()?;
        let level = r.u16()?;
        r.skip(4)?;
        let number = r.u16()?;
        let opacity = r.u8()?;
        r.skip(3)?;
        let name = self.note_name(tidy(r.text()?, MAX_LAYER_NAME));
        let tileset = if kind == TILEMAP_LAYER { r.u32()? } else { 0 };
        if level > u16::from(MAX_GROUP_DEPTH) {
            return Err(FormatError::Unsupported(
                "layer groups nested deeper than four",
            ));
        }
        if level > self.open_level() {
            return Err(FormatError::Corrupt("a layer sits deeper than its group"));
        }
        let kind = self.layer_kind(kind);
        let (opacity, blend) = self.layer_look(kind, &name, opacity, number);
        if flags & REFERENCE != 0 && kind == Kind::Image {
            self.warnings.add(AseWarning::ReferenceLayer(name.clone()));
        }
        self.count_layer(kind)?;
        let drawn = matches!(kind, Kind::Image | Kind::Tilemap);
        let frames = if drawn { self.frames } else { 0 };
        self.hold_frame_table(frames)?;
        self.layers.push(AseLayer {
            name,
            level: level as u8,
            kind,
            visible: flags & VISIBLE != 0,
            locked: flags & EDITABLE == 0,
            folded: flags & COLLAPSED != 0 && kind == Kind::Group,
            background: flags & BACKGROUND != 0,
            tileset,
            opacity,
            blend,
            cels: vec![None; frames],
            holds: vec![0; frames],
            opacities: vec![u8::MAX; frames],
        });
        Ok(())
    }

    /// Takes a drawn layer's table of frames out of the memory the file may hold: a layer chunk is
    /// a few bytes, and its table is [`FRAME_SLOT_BYTES`] for each of up to 4,096 frames.
    fn hold_frame_table(&mut self, frames: usize) -> Result<(), FormatError> {
        let table = (frames as u64).saturating_mul(FRAME_SLOT_BYTES as u64);
        self.budget.held = self
            .budget
            .held
            .checked_sub(table)
            .ok_or(FormatError::FileTooLarge)?;
        Ok(())
    }

    /// Counts a layer of `kind` about to be added, refusing one past what can be numbered or
    /// kept.
    fn count_layer(&mut self, kind: Kind) -> Result<(), FormatError> {
        // A cel names its layer with 16 bits, so no more layers than that can be drawn on, and
        // the layers that are left out count too: each one is held to number the rest.
        if self.layers.len() >= MAX_NUMBERED_LAYERS {
            return Err(FormatError::Unsupported("more than 65,536 layers"));
        }
        if kind != Kind::Skipped {
            self.kept += 1;
            if self.kept > MAX_LAYERS {
                return Err(FormatError::Unsupported("more than 256 layers"));
            }
        }
        Ok(())
    }

    /// What a layer of this type number becomes, with a warning if it is left out.
    fn layer_kind(&mut self, number: u16) -> Kind {
        match number {
            0 => Kind::Image,
            1 => Kind::Group,
            TILEMAP_LAYER => Kind::Tilemap,
            _ => {
                self.warnings
                    .add(AseWarning::Skipped("layers of an unknown type"));
                Kind::Skipped
            }
        }
    }

    /// The opacity and blend mode a layer is shown with, and the warnings about them.
    fn layer_look(&mut self, kind: Kind, name: &str, opacity: u8, number: u16) -> (u8, BlendMode) {
        let (blend, exact) = blend_for(number);
        match kind {
            Kind::Image | Kind::Tilemap => {
                if !exact {
                    self.warnings.add(AseWarning::LayerBlend {
                        layer: name.to_owned(),
                        mode: number,
                        shown_as: blend,
                    });
                }
                let valid = self.flags & LAYER_OPACITY_VALID != 0;
                (if valid { opacity } else { u8::MAX }, blend)
            }
            Kind::Group => {
                let set = self.flags & GROUP_SETTINGS_VALID != 0;
                if set && (opacity != u8::MAX || number != 0) {
                    self.warnings
                        .add(AseWarning::GroupSettings(name.to_owned()));
                }
                (u8::MAX, BlendMode::Normal)
            }
            Kind::Skipped => (u8::MAX, BlendMode::Normal),
        }
    }

    /// A tags chunk (0x2018).
    pub fn tags_chunk(&mut self, data: &[u8]) -> Result<(), FormatError> {
        let mut r = Bytes(data);
        let count = r.u16()?;
        r.skip(8)?;
        for _ in 0..count {
            let (from, to) = (usize::from(r.u16()?), usize::from(r.u16()?));
            let direction = r.u8()?;
            let repeat = r.u16()?;
            r.skip(6 + 3 + 1)?;
            let name = tidy(r.text()?, MAX_TAG_NAME);
            self.add_tag(name, from, to, direction, repeat);
        }
        Ok(())
    }

    fn add_tag(&mut self, name: Name, from: usize, to: usize, direction: u8, repeat: u16) {
        let cut = name.cut;
        let name = self.note_name(name);
        if self.tags.len() >= MAX_TAGS || from > to || to >= self.frames {
            self.warnings.add(AseWarning::TagDropped(name));
            return;
        }
        let direction = match direction {
            0 => Direction::Forward,
            1 => Direction::Reverse,
            2 => Direction::PingPong,
            _ => {
                self.warnings.add(AseWarning::TagDirection(name.clone()));
                Direction::PingPong
            }
        };
        if repeat > MAX_TAG_REPEAT {
            self.warnings.add(AseWarning::TagRepeat(name.clone()));
        }
        if cut {
            self.warnings.tally(Tally::TagName, &name, 0);
        }
        self.tags.push(Tag {
            name,
            from,
            to,
            direction,
            repeat: repeat.min(MAX_TAG_REPEAT),
            id: TagId::fresh(),
        });
    }
}
