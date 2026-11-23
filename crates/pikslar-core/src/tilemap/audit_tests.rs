//! The kind audit: every `Command` variant tried with a tilemap layer active, and each either
//! refused with its own message, leaving the document and history alone, or correct, leaving
//! the document sound and one step that undoes and redoes exactly. The table is in
//! `audit_cases`.

use super::audit_cases::{cases, scene, Expect};
use crate::command::CanvasCmd;
use crate::document::LimitError;
use crate::{
    CelCmd, Command, DrawCmd, Editor, FloatingCmd, FrameCmd, LayerCmd, PaletteCmd, SelectionCmd,
    SliceCmd, StepCmd, TagCmd, TerrainCmd, TileCmd,
};
use Expect::{Any, Nothing, Refused, Works};

/// How many `Command` variants there are, counting each of the sub-enums' own. A new variant is
/// a compile error in [`variant`], and a failure here until it has a case.
const VARIANTS: usize = 108;

/// The variant a command is, by name, with no wildcard so that a new one has to be added here.
fn variant(command: &Command) -> &'static str {
    match command {
        Command::Canvas(CanvasCmd::Resize { .. }) => "Canvas::Resize",
        Command::Canvas(CanvasCmd::SetBlending { .. }) => "Canvas::SetBlending",
        Command::Layer(c) => match c {
            LayerCmd::Add { .. } => "Layer::Add",
            LayerCmd::Remove { .. } => "Layer::Remove",
            LayerCmd::Move { .. } => "Layer::Move",
            LayerCmd::SetOpacity { .. } => "Layer::SetOpacity",
            LayerCmd::SetVisible { .. } => "Layer::SetVisible",
            LayerCmd::SetLocked { .. } => "Layer::SetLocked",
            LayerCmd::SetBlend { .. } => "Layer::SetBlend",
            LayerCmd::SetMode { .. } => "Layer::SetMode",
            LayerCmd::Rename { .. } => "Layer::Rename",
            LayerCmd::Duplicate { .. } => "Layer::Duplicate",
            LayerCmd::MergeDown { .. } => "Layer::MergeDown",
            LayerCmd::Group { .. } => "Layer::Group",
            LayerCmd::Ungroup { .. } => "Layer::Ungroup",
            LayerCmd::Step { .. } => "Layer::Step",
            LayerCmd::SetFolded { .. } => "Layer::SetFolded",
            LayerCmd::PlaceReference { .. } => "Layer::PlaceReference",
        },
        Command::Frame(c) => match c {
            FrameCmd::Add { .. } => "Frame::Add",
            FrameCmd::Remove { .. } => "Frame::Remove",
            FrameCmd::Move { .. } => "Frame::Move",
            FrameCmd::Retime { .. } => "Frame::Retime",
            FrameCmd::HoldPrevious { .. } => "Frame::HoldPrevious",
            FrameCmd::HoldAcross { .. } => "Frame::HoldAcross",
            FrameCmd::Release { .. } => "Frame::Release",
            FrameCmd::Duplicate { .. } => "Frame::Duplicate",
            FrameCmd::MoveRun { .. } => "Frame::MoveRun",
            FrameCmd::SetRate { .. } => "Frame::SetRate",
        },
        Command::Tag(c) => match c {
            TagCmd::Add { .. } => "Tag::Add",
            TagCmd::SetName { .. } => "Tag::SetName",
            TagCmd::Remove { .. } => "Tag::Remove",
            TagCmd::CycleDirection { .. } => "Tag::CycleDirection",
            TagCmd::SetRepeat { .. } => "Tag::SetRepeat",
            TagCmd::SetRange { .. } => "Tag::SetRange",
        },
        Command::Slice(c) => match c {
            SliceCmd::Add { .. } => "Slice::Add",
            SliceCmd::Set { .. } => "Slice::Set",
            SliceCmd::Remove { .. } => "Slice::Remove",
        },
        Command::Palette(c) => match c {
            PaletteCmd::AddSwatch { .. } => "Palette::AddSwatch",
            PaletteCmd::RemoveSwatch { .. } => "Palette::RemoveSwatch",
            PaletteCmd::Recolor { .. } => "Palette::Recolor",
            PaletteCmd::MoveSwatch { .. } => "Palette::MoveSwatch",
            PaletteCmd::Sort { .. } => "Palette::Sort",
            PaletteCmd::AddGroup { .. } => "Palette::AddGroup",
            PaletteCmd::RenameGroup { .. } => "Palette::RenameGroup",
            PaletteCmd::RemoveGroup { .. } => "Palette::RemoveGroup",
            PaletteCmd::AddToGroup { .. } => "Palette::AddToGroup",
            PaletteCmd::RemoveFromGroup { .. } => "Palette::RemoveFromGroup",
            PaletteCmd::AddRamp { .. } => "Palette::AddRamp",
            PaletteCmd::Remap(_) => "Palette::Remap",
            PaletteCmd::ReplaceColor { .. } => "Palette::ReplaceColor",
            PaletteCmd::Load { .. } => "Palette::Load",
        },
        Command::Step(c) => match c {
            StepCmd::EndDrag => "Step::EndDrag",
            StepCmd::AbortJoin => "Step::AbortJoin",
            StepCmd::Undo => "Step::Undo",
            StepCmd::Redo => "Step::Redo",
            StepCmd::ScrubTo { .. } => "Step::ScrubTo",
        },
        Command::Draw(c) => match c {
            DrawCmd::Begin { .. } => "Draw::Begin",
            DrawCmd::To { .. } => "Draw::To",
            DrawCmd::Commit => "Draw::Commit",
            DrawCmd::Cancel => "Draw::Cancel",
            DrawCmd::Stroke { .. } => "Draw::Stroke",
            DrawCmd::EraseCell { .. } => "Draw::EraseCell",
            DrawCmd::DeleteSelected => "Draw::DeleteSelected",
        },
        Command::Selection(c) => match c {
            SelectionCmd::Select { .. } => "Selection::Select",
            SelectionCmd::Temporary { .. } => "Selection::Temporary",
            SelectionCmd::All => "Selection::All",
            SelectionCmd::None => "Selection::None",
            SelectionCmd::Invert => "Selection::Invert",
        },
        Command::Floating(c) => match c {
            FloatingCmd::Lift { .. } => "Floating::Lift",
            FloatingCmd::Move { .. } => "Floating::Move",
            FloatingCmd::Scale { .. } => "Floating::Scale",
            FloatingCmd::FlipHorizontal => "Floating::FlipHorizontal",
            FloatingCmd::FlipVertical => "Floating::FlipVertical",
            FloatingCmd::Rotate { .. } => "Floating::Rotate",
            FloatingCmd::Anchor => "Floating::Anchor",
            FloatingCmd::Revert => "Floating::Revert",
            FloatingCmd::Paste { .. } => "Floating::Paste",
            FloatingCmd::ReplacePasted { .. } => "Floating::ReplacePasted",
            FloatingCmd::GestureBegin => "Floating::GestureBegin",
            FloatingCmd::GestureEnd => "Floating::GestureEnd",
            FloatingCmd::GestureCancel => "Floating::GestureCancel",
        },
        Command::Cel(c) => match c {
            CelCmd::Move { .. } => "Cel::Move",
            CelCmd::MoveOne { .. } => "Cel::MoveOne",
            CelCmd::Turn { .. } => "Cel::Turn",
            CelCmd::Drag(_) => "Cel::Drag",
            CelCmd::Tween { .. } => "Cel::Tween",
        },
        Command::Tile(c) => match c {
            TileCmd::NewTileset { .. } => "Tile::NewTileset",
            TileCmd::RenameTileset { .. } => "Tile::RenameTileset",
            TileCmd::RemoveTileset { .. } => "Tile::RemoveTileset",
            TileCmd::AddTile { .. } => "Tile::AddTile",
            TileCmd::DuplicateTile { .. } => "Tile::DuplicateTile",
            TileCmd::RemoveTile { .. } => "Tile::RemoveTile",
            TileCmd::MoveTile { .. } => "Tile::MoveTile",
            TileCmd::SetTileImage { .. } => "Tile::SetTileImage",
            TileCmd::NewLayer { .. } => "Tile::NewLayer",
            TileCmd::NewMapLayer { .. } => "Tile::NewMapLayer",
            TileCmd::CopyTilesetFor { .. } => "Tile::CopyTilesetFor",
            TileCmd::Begin { .. } => "Tile::Begin",
            TileCmd::To { .. } => "Tile::To",
            TileCmd::Commit => "Tile::Commit",
            TileCmd::Cancel => "Tile::Cancel",
            TileCmd::Stroke { .. } => "Tile::Stroke",
            TileCmd::DeleteSelected => "Tile::DeleteSelected",
        },
        Command::Terrain(c) => match c {
            TerrainCmd::New { .. } => "Terrain::New",
            TerrainCmd::Rename { .. } => "Terrain::Rename",
            TerrainCmd::Remove { .. } => "Terrain::Remove",
            TerrainCmd::Assign { .. } => "Terrain::Assign",
            TerrainCmd::Clear { .. } => "Terrain::Clear",
        },
    }
}

/// Runs the case at `index` on a fresh scene and checks it against what it should do.
fn audit(index: usize) {
    let mut e = scene();
    let (setup, command, expect) = cases(&e).swap_remove(index);
    setup(&mut e);
    let (before, depth) = (e.doc().clone(), e.history().undo_depth());
    let name = variant(&command);
    let result = e.apply(command);
    assert_eq!(e.doc().check_links(), Ok(()), "{name}");
    assert_eq!(e.doc().check_tilemaps(), Ok(()), "{name}");
    match (expect, &result) {
        (Refused(want), got) => {
            assert_eq!(got, &Err(want), "{name}");
            assert_eq!(e.doc(), &before, "{name} refused but changed the document");
            assert_eq!(
                e.history().undo_depth(),
                depth,
                "{name} refused but made a step"
            );
        }
        (Works, Ok(out)) => {
            assert!(out.changed, "{name} did nothing");
            undoes_and_redoes(&mut e, &before, depth, name);
        }
        (Nothing, Ok(out)) => {
            assert!(!out.changed, "{name} changed something");
            assert_eq!(e.doc(), &before, "{name}");
            assert_eq!(e.history().undo_depth(), depth, "{name}");
        }
        (Any, Ok(_)) => undoes_and_redoes(&mut e, &before, depth, name),
        (Any, Err(_)) => assert_eq!(e.doc(), &before, "{name} refused but changed the document"),
        (Works | Nothing, Err(error)) => panic!("{name} was refused: {error}"),
    }
}

/// If the command made one step, undoing it gives the document before and redoing it the one
/// after. Visibility and locks are never steps, so such a command has nothing to undo.
fn undoes_and_redoes(e: &mut Editor, before: &crate::Document, depth: usize, name: &str) {
    let after = e.doc().clone();
    if e.history().undo_depth() != depth + 1 {
        return;
    }
    assert!(e.undo(), "{name}");
    assert_eq!(e.doc().check_tilemaps(), Ok(()), "{name} undone");
    assert_eq!(e.doc().check_links(), Ok(()), "{name} undone");
    assert_eq!(e.doc(), before, "{name} undone");
    assert!(e.redo(), "{name}");
    assert_eq!(e.doc(), &after, "{name} redone");
}

#[test]
fn every_command_on_a_tilemap_layer_is_refused_with_a_message_or_correct() {
    let count = cases(&scene()).len();
    (0..count).for_each(audit);
}

#[test]
fn every_command_variant_has_a_case() {
    let e = scene();
    let mut names: Vec<&str> = cases(&e).iter().map(|(_, c, _)| variant(c)).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), VARIANTS, "{names:?}");
}

#[test]
fn the_refusals_name_what_is_wrong_in_words() {
    for error in [
        LimitError::Tilemap,
        LimitError::TilemapMode,
        LimitError::TilemapMerge,
        LimitError::TilemapCels,
        LimitError::LinkTileset,
        LimitError::NotTilemap,
    ] {
        let text = error.to_string();
        assert!(text.len() > 20, "{text}");
    }
}
