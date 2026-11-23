//! Every change to a document, as plain data.
//!
//! A [`Command`] holds no references and no code, so it can be queued, logged, replayed or one
//! day produced by a script. `Editor::apply` is the only way to run one, and the rules for what
//! becomes an undo step are written in `docs/command-core.md`.

use crate::document::{BlendMode, ColorMode, LimitError};
use crate::editor::Editor;
use crate::editor::Tool;
use crate::floating::Clip;
use crate::layer::{Layer, Turn};
use crate::mask::Mask;
use crate::selection::SelectOp;
use crate::tileset::{TileId, TilesetId};
use std::sync::Arc;

mod canvas;
mod canvas_size;
#[cfg(test)]
mod canvas_size_tests;
mod cel;
#[cfg(test)]
mod cel_tests;
#[cfg(test)]
mod edge_tests;
mod frame;
mod guard;
#[cfg(test)]
mod guard_tests;
mod layer;
mod memory;
#[cfg(test)]
mod paint_reach_tests;
mod palette;
mod palette_load;
mod palette_remap;
mod reach;
#[cfg(test)]
mod rollback_tests;
mod slice;
mod tag_palette;
mod terrain;
#[cfg(test)]
mod tests;
mod tile;
#[cfg(test)]
mod tile_tests;
#[cfg(test)]
mod turn_tests;
mod tween;
#[cfg(test)]
mod tween_tests;

pub use canvas_size::{anchor_shift, crop_to};
pub use layer::MAX_LAYER_NAME;
pub(crate) use memory::system_has_room;
pub use memory::MemoryCheck;
pub use tile::{Corners, TerrainShape, TerrainTool, TileCmd, TileSource, TileTool};

/// Names a layer for the length of the session, wherever it is moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerId(pub(crate) u32);

/// Commands are kept small, because every pointer movement during a stroke is one.
const _: () = assert!(std::mem::size_of::<Command>() <= MOST_BYTES_IN_A_COMMAND);
const MOST_BYTES_IN_A_COMMAND: usize = 56;

impl From<&Layer> for LayerId {
    fn from(layer: &Layer) -> Self {
        LayerId(layer.id())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerCmd {
    /// Inserts a blank layer, counted from the bottom, and makes it the active one.
    Add {
        at: usize,
        mode: ColorMode,
    },
    Remove {
        layer: LayerId,
    },
    /// Moves a layer to a position, and the active layer goes with it.
    Move {
        layer: LayerId,
        to: usize,
    },
    /// Joins earlier changes of this layer's opacity until `StepCmd::EndDrag`.
    SetOpacity {
        layer: LayerId,
        opacity: u8,
    },
    /// Never undone: hiding a layer is how the artist looks at the work.
    SetVisible {
        layer: LayerId,
        visible: bool,
    },
    /// Never undone, like visibility. Locking the active layer puts down a stroke or floating
    /// pixels first, so nothing is left to land on it.
    SetLocked {
        layer: LayerId,
        locked: bool,
    },
    SetBlend {
        layer: LayerId,
        blend: BlendMode,
    },
    SetMode {
        layer: LayerId,
        mode: ColorMode,
    },
    Rename {
        layer: LayerId,
        name: String,
    },
    /// Puts a copy above the layer, sharing its drawings, and makes the copy active.
    Duplicate {
        layer: LayerId,
    },
    /// Replaces the layer and the one below with one layer drawn as the two are shown.
    MergeDown {
        layer: LayerId,
    },
    /// Puts the layer, with everything in it, into a new group, which becomes active.
    Group {
        layer: LayerId,
    },
    /// Takes the layers out of a group and removes it.
    Ungroup {
        layer: LayerId,
    },
    /// Moves the layer, with everything in it, one place up or down, into and out of groups.
    Step {
        layer: LayerId,
        up: bool,
    },
    /// Never undone, like visibility: a folded group shows only its own row.
    SetFolded {
        layer: LayerId,
        folded: bool,
    },
    /// Puts a reference layer showing `image` above layer `below`, and makes it active.
    PlaceReference {
        below: usize,
        image: std::sync::Arc<crate::image::Image>,
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameCmd {
    /// Inserts a frame and shows it. `copy` starts it from the frame before.
    Add { at: usize, copy: bool },
    /// Removes a run of frames and shows what takes their place.
    Remove { from: usize, to: usize },
    /// Moves a frame and goes with it.
    Move { from: usize, to: usize },
    /// Sets the timing of the frames from `from` onward. Joins earlier changes to those frames.
    Retime { from: usize, durations: Vec<u32> },
    /// Makes a frame show the same drawing as the one before it.
    HoldPrevious { layer: LayerId, frame: usize },
    /// Shows one frame's drawing in every frame of a range.
    HoldAcross {
        layer: LayerId,
        source: usize,
        from: usize,
        to: usize,
    },
    /// Gives each frame of a range its own copy of the drawing.
    Release {
        layer: LayerId,
        from: usize,
        to: usize,
    },
    /// Puts a copy of a run of frames straight after it. The current frame's copy becomes
    /// current.
    Duplicate { from: usize, to: usize },
    /// Moves a run of frames so that its first is at `at`. The current frame goes with it.
    MoveRun { from: usize, to: usize, at: usize },
    /// Times the frames by a frame rate, or in milliseconds for `None`, rounding each frame to
    /// whole ticks of a new rate.
    SetRate { fps: Option<u8> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagCmd {
    /// Adds a tag. A name typed into it straight after is part of the same step.
    Add {
        name: String,
        from: usize,
        to: usize,
    },
    /// Changes a name. Typing joins one step until `StepCmd::EndDrag`.
    SetName {
        tag: usize,
        name: String,
    },
    Remove {
        tag: usize,
    },
    CycleDirection {
        tag: usize,
    },
    /// How many passes the tag plays, 0 for forever; kept to [`crate::MAX_TAG_REPEAT`].
    SetRepeat {
        tag: usize,
        repeat: u16,
    },
    /// Gives a tag other frames, as when one of its edges is dragged.
    SetRange {
        tag: usize,
        from: usize,
        to: usize,
    },
}

/// Slices of the canvas, by their place in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SliceCmd {
    /// Adds a slice at the end of the list.
    Add {
        slice: Box<crate::slice::Slice>,
    },
    /// Replaces a slice whole: its name, color, rectangle, pivot and centre.
    Set {
        index: usize,
        slice: Box<crate::slice::Slice>,
    },
    Remove {
        index: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteCmd {
    AddSwatch {
        color: u32,
    },
    RemoveSwatch {
        id: u32,
    },
    /// Changes what a color looks like. Joins earlier changes of it until `StepCmd::EndDrag`.
    Recolor {
        id: u32,
        color: u32,
    },
    /// Moves the swatch at position `from` to position `to`. Ids and pixels are untouched.
    MoveSwatch {
        from: usize,
        to: usize,
    },
    /// Puts the swatches in order. Ids and pixels are untouched.
    Sort {
        by: crate::color::SortBy,
    },
    /// Names an ordered list of swatches, as a group at the end. Every id must be a swatch.
    AddGroup {
        name: String,
        ids: Arc<[u32]>,
    },
    /// Joins earlier renames of this group until `StepCmd::EndDrag`, as typing does.
    RenameGroup {
        group: usize,
        name: String,
    },
    /// Removes the group only: its swatches stay.
    RemoveGroup {
        group: usize,
    },
    /// Adds a swatch to the end of a group. Nothing happens if it is already in the group.
    AddToGroup {
        group: usize,
        id: u32,
    },
    /// Takes a swatch out of a group, which keeps its other swatches. Nothing happens if it was
    /// not in the group.
    RemoveFromGroup {
        group: usize,
        id: u32,
    },
    /// Adds `steps` colours between two swatches (see `palette_ops::ramp`) as swatches, and
    /// a Ramp group of `from`, those, then `to`, at the end of the groups. Ids and pixels are
    /// untouched. The outcome's `created` is the group's index.
    AddRamp {
        from: u32,
        to: u32,
        steps: u32,
    },
    /// Renumbers, merges or drops colours without changing how any frame looks. Every palette
    /// image changes id and nothing changes colour, so locks and reference layers do not refuse
    /// it: a layer left on the old ids would break. One step.
    Remap(crate::palette_ops::remap::RemapKind),
    /// Changes pixels: every pixel showing id `from` takes id `to`, on one layer (and the cels
    /// linked to it) or, with `layer` as None, everywhere, palette tilesets included. Refused,
    /// as painting would be, if a layer it would change is locked or a reference. This is not
    /// "change colours" (`Recolor`), which changes what an id looks like.
    ReplaceColor {
        from: u32,
        to: u32,
        layer: Option<LayerId>,
    },
    /// Puts a list of colours (a palette file's) into the palette: see
    /// [`crate::palette_ops::load::LoadHow`]. Colours arrive opaque. Refused before anything
    /// changes if the table cannot hold them or, for Load as colours, the list is not as long
    /// as the swatch list. One step, none if nothing would change.
    Load {
        colors: Arc<[u32]>,
        how: crate::palette_ops::load::LoadHow,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepCmd {
    /// The value being dragged or typed is finished, so the next change starts a new step.
    EndDrag,
    /// Takes back the value being dragged or typed and leaves nothing to redo.
    AbortJoin,
    Undo,
    Redo,
    /// Walks the undo stack until this many steps are behind the current state.
    ScrubTo {
        depth: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrawCmd {
    /// Presses a tool on the canvas. A fill is finished at once.
    Begin {
        tool: Tool,
        x: i32,
        y: i32,
    },
    /// Continues the stroke in progress.
    To {
        x: i32,
        y: i32,
    },
    /// Puts the stroke in progress into the document as one step.
    Commit,
    /// Throws the stroke in progress away.
    Cancel,
    /// A whole stroke, given all at once.
    Stroke {
        tool: Tool,
        path: Vec<(i32, i32)>,
    },
    EraseCell {
        x: i32,
        y: i32,
    },
    DeleteSelected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectionCmd {
    /// Boxed so a command stays small.
    Select {
        shape: Box<Mask>,
        op: SelectOp,
    },
    /// Selects a shape just to pick it up, which is no step of its own.
    Temporary {
        shape: Box<Mask>,
    },
    All,
    None,
    Invert,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FloatingCmd {
    Lift {
        copy: bool,
    },
    /// `join` continues the last move, as a held arrow key does.
    Move {
        dx: i32,
        dy: i32,
        join: bool,
    },
    Scale {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    FlipHorizontal,
    FlipVertical,
    Rotate {
        clockwise: bool,
    },
    Anchor,
    Revert,
    Paste {
        clip: Arc<Clip>,
        x: i32,
        y: i32,
    },
    /// Swaps the pixels of a paste that is still floating for another clip, keeping where they
    /// were moved to. Refused unless a paste is floating. Text typed after a paste uses it, as
    /// pasting again would anchor what is there.
    ReplacePasted {
        clip: Arc<Clip>,
    },
    /// Moves between Begin and End are one step back, or none if they went nowhere.
    GestureBegin,
    GestureEnd,
    /// Puts the pixels back where the gesture found them.
    GestureCancel,
}

/// Changes to where a cel sits. Its pixels are never touched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CelCmd {
    /// Moves a frame's cel, and every frame held with it, as far as the offset range allows.
    /// Joins earlier moves of the same cel until `StepCmd::EndDrag`, which ends the drag;
    /// `StepCmd::AbortJoin` puts the cel back where the drag found it.
    Move {
        layer: LayerId,
        frame: usize,
        dx: i32,
        dy: i32,
    },
    /// Moves only this cel, whatever link it is in, as far as the offset range allows. Joins
    /// earlier moves of only this cel as [`CelCmd::Move`] does, and never a move of its hold.
    MoveOne {
        layer: LayerId,
        frame: usize,
        dx: i32,
        dy: i32,
    },
    /// Flips or rotates a frame's whole drawing about the canvas centre, pixels past the edge
    /// included, in every frame held with it. Refused if they sit in different places.
    Turn {
        layer: LayerId,
        frame: usize,
        turn: Turn,
    },
    /// Moves, copies or links a block of cels to another place in the timeline, as one step.
    /// The block's landing place becomes the current layer and frame.
    Drag(crate::cel_drag::CelDrag),
    /// Gives every frame between `from` and `to` an offset between theirs, eased over the
    /// frames' durations, as one step. The run must be at least three frames of one hold.
    Tween {
        layer: LayerId,
        from: usize,
        to: usize,
        ease: crate::tween::Ease,
    },
}

/// Changes to the canvas itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanvasCmd {
    /// A canvas `width` by `height`, with the old one's top left corner at (`dx`, `dy`) in it.
    /// Every cel moves with it, and no pixel is removed. The selection is cleared.
    Resize {
        width: usize,
        height: usize,
        dx: i32,
        dy: i32,
    },
    /// How the layers combine: Pikslar's own blending, or Aseprite-compatible blending for
    /// imported art. One step; no stored pixel or layer setting changes, only how the layers
    /// are drawn together, so undo brings the look back exactly.
    SetBlending { blending: crate::Blending },
}

/// Terrain set commands (M11): the terrain sets of a tileset and their slots. A set is named by
/// its tileset and its id within it. Each is one undo step, and none when nothing changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerrainCmd {
    /// Adds a terrain set with every slot empty. Its id is the outcome's `created`. With
    /// `template`, a blank tile is added at the end of the tileset for every slot, in slot
    /// order, and each slot takes its own, in the same step.
    New {
        tileset: TilesetId,
        kind: crate::autotile::TerrainKind,
        name: String,
        template: bool,
    },
    /// Typing joins one step until `StepCmd::EndDrag`.
    Rename {
        tileset: TilesetId,
        set: u16,
        name: String,
    },
    /// Removes a terrain set. Its tiles stay.
    Remove { tileset: TilesetId, set: u16 },
    /// Puts a tile of the tileset in a slot of the set, in the rule engine's slot order.
    Assign {
        tileset: TilesetId,
        set: u16,
        slot: u8,
        tile: TileId,
    },
    /// Empties a slot of the set.
    Clear {
        tileset: TilesetId,
        set: u16,
        slot: u8,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Canvas(CanvasCmd),
    Layer(LayerCmd),
    Frame(FrameCmd),
    Tag(TagCmd),
    Slice(SliceCmd),
    Palette(PaletteCmd),
    Step(StepCmd),
    Draw(DrawCmd),
    Selection(SelectionCmd),
    Floating(FloatingCmd),
    Cel(CelCmd),
    Tile(TileCmd),
    Terrain(TerrainCmd),
}

impl Command {
    /// True for a command that can carry on a value being dragged or typed, or that leaves the
    /// document alone. Any other ends the value first, so its step can be finished while the
    /// document is as the value left it.
    fn may_continue_a_value(&self) -> bool {
        matches!(
            self,
            Command::Frame(FrameCmd::Retime { .. })
                | Command::Layer(
                    LayerCmd::SetOpacity { .. }
                        | LayerCmd::SetVisible { .. }
                        | LayerCmd::SetLocked { .. }
                        | LayerCmd::SetFolded { .. }
                )
                | Command::Palette(PaletteCmd::Recolor { .. } | PaletteCmd::RenameGroup { .. })
                | Command::Tag(TagCmd::SetName { .. } | TagCmd::Remove { .. })
                | Command::Tile(TileCmd::RenameTileset { .. })
                | Command::Terrain(TerrainCmd::Rename { .. })
                | Command::Step(StepCmd::EndDrag | StepCmd::AbortJoin)
                | Command::Cel(CelCmd::Move { .. } | CelCmd::MoveOne { .. } | CelCmd::Turn { .. })
                | Command::Selection(_)
                | Command::Floating(_)
        )
    }
}

impl From<CanvasCmd> for Command {
    fn from(command: CanvasCmd) -> Self {
        Command::Canvas(command)
    }
}

impl From<CelCmd> for Command {
    fn from(command: CelCmd) -> Self {
        Command::Cel(command)
    }
}

impl From<DrawCmd> for Command {
    fn from(command: DrawCmd) -> Self {
        Command::Draw(command)
    }
}

impl From<SelectionCmd> for Command {
    fn from(command: SelectionCmd) -> Self {
        Command::Selection(command)
    }
}

impl From<FloatingCmd> for Command {
    fn from(command: FloatingCmd) -> Self {
        Command::Floating(command)
    }
}

impl From<TagCmd> for Command {
    fn from(command: TagCmd) -> Self {
        Command::Tag(command)
    }
}

impl From<SliceCmd> for Command {
    fn from(command: SliceCmd) -> Self {
        Command::Slice(command)
    }
}

impl From<PaletteCmd> for Command {
    fn from(command: PaletteCmd) -> Self {
        Command::Palette(command)
    }
}

impl From<TileCmd> for Command {
    fn from(command: TileCmd) -> Self {
        Command::Tile(command)
    }
}

impl From<TerrainCmd> for Command {
    fn from(command: TerrainCmd) -> Self {
        Command::Terrain(command)
    }
}

impl From<StepCmd> for Command {
    fn from(command: StepCmd) -> Self {
        Command::Step(command)
    }
}

impl From<FrameCmd> for Command {
    fn from(command: FrameCmd) -> Self {
        Command::Frame(command)
    }
}

impl From<LayerCmd> for Command {
    fn from(command: LayerCmd) -> Self {
        Command::Layer(command)
    }
}

/// What a command did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// False if the document was left as it was.
    pub changed: bool,
    /// The position or id of anything the command made.
    pub created: Option<u32>,
}

pub(crate) fn did(changed: bool) -> Outcome {
    if changed {
        Outcome::CHANGED
    } else {
        Outcome::UNCHANGED
    }
}

impl Outcome {
    pub(crate) const CHANGED: Outcome = Outcome {
        changed: true,
        created: None,
    };
    pub(crate) const UNCHANGED: Outcome = Outcome {
        changed: false,
        created: None,
    };
}

impl Editor {
    /// Runs one command. On an error the document is as it was.
    // Built once for each kind, and called rather than copied into each caller: the program
    // and the scripts both run commands, and a copy in each was the same code twice.
    #[inline(never)]
    pub fn apply(&mut self, command: impl Into<Command>) -> Result<Outcome, LimitError> {
        let mut command = command.into();
        if command.may_paint() {
            self.refuse_painting(&command)?;
        } else {
            // Skipped, since the check would pass it whatever the document; see `may_paint`.
            debug_assert!(
                self.refuse_painting(&command).is_ok(),
                "{command:?} claims to paint nothing"
            );
        }
        self.keep_in_reach(&mut command)?;
        self.check_memory(&command)?;
        let before = (self.history.pushes(), self.revision);
        let (outcome, transaction) = match command.may_continue_a_value() {
            true => self.run(command)?,
            false => self.with_open_step_finished(|e| e.run(command))?,
        };
        // A transaction that left everything as it found it records nothing.
        let moved = before != (self.history.pushes(), self.revision);
        Ok(Outcome {
            changed: outcome.changed && (moved || !transaction),
            ..outcome
        })
    }

    /// Runs the command, saying also whether it is a transaction.
    // Inlined into each kind's `apply`, where only that kind's arm is left.
    #[inline(always)]
    fn run(&mut self, command: Command) -> Result<(Outcome, bool), LimitError> {
        Ok(match command {
            Command::Canvas(command) => (self.apply_canvas(command)?, true),
            Command::Layer(command) => (self.apply_layer(command)?, true),
            Command::Frame(command) => (self.apply_frame(command)?, true),
            Command::Tag(command) => (self.apply_tag(command)?, true),
            Command::Slice(command) => (self.apply_slice(command)?, true),
            Command::Palette(command) => (self.apply_palette(command)?, true),
            Command::Step(command) => (self.apply_step(command)?, false),
            Command::Draw(command) => (self.apply_draw(command)?, false),
            Command::Selection(command) => (self.apply_selection(command)?, false),
            Command::Floating(command) => (self.apply_floating(command)?, false),
            Command::Cel(command) => (self.apply_cel(command)?, false),
            Command::Tile(command) => (self.apply_tile(command)?, true),
            Command::Terrain(command) => (self.apply_terrain(command)?, true),
        })
    }

    /// The name that keeps finding this layer after others are added, moved or removed.
    pub fn layer_id(&self, index: usize) -> Option<LayerId> {
        self.doc.layers.get(index).map(LayerId::from)
    }

    pub(crate) fn index_of(&self, layer: LayerId) -> Result<usize, LimitError> {
        self.doc
            .layers
            .iter()
            .position(|l| l.id() == layer.0)
            .ok_or(LimitError::NoSuchItem)
    }
}
