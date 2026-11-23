//! What commands promise, whichever one is run.

use super::*;
use crate::color::rgba;
use crate::document::{BlendMode, Document};
use crate::Tool;

const INK: u32 = crate::color::rgba(200, 40, 20, 255);

fn editor() -> Editor {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Rgba).expect("valid"));
    e.color = INK;
    e
}

fn add_layer(e: &mut Editor, at: usize) {
    e.apply(LayerCmd::Add {
        at,
        mode: ColorMode::Rgba,
    })
    .expect("room");
}

#[test]
fn a_layer_is_found_by_its_name_after_others_move() {
    let mut e = editor();
    let first = e.layer_id(0).expect("exists");
    add_layer(&mut e, 0);
    e.apply(LayerCmd::SetBlend {
        layer: first,
        blend: BlendMode::Multiply,
    })
    .expect("found");
    assert_eq!(e.doc.layers[1].blend, BlendMode::Multiply);
    assert_eq!(e.doc.layers[0].blend, BlendMode::Normal);
}

#[test]
fn a_new_layer_becomes_the_active_one_and_a_moved_layer_stays_active() {
    let mut e = editor();
    add_layer(&mut e, 1);
    assert_eq!(e.layer, 1);
    let top = e.layer_id(1).expect("exists");
    e.apply(LayerCmd::Move { layer: top, to: 0 })
        .expect("moves");
    assert_eq!(e.layer, 0);
}

#[test]
fn a_layer_that_is_gone_is_an_error_and_changes_nothing() {
    let mut e = editor();
    add_layer(&mut e, 1);
    let gone = e.layer_id(1).expect("exists");
    e.apply(LayerCmd::Remove { layer: gone }).expect("removed");
    let depth = e.history.undo_depth();
    let revision = e.revision;
    let result = e.apply(LayerCmd::SetBlend {
        layer: gone,
        blend: BlendMode::Multiply,
    });
    assert_eq!(result, Err(LimitError::NoSuchItem));
    assert_eq!(e.history.undo_depth(), depth);
    assert_eq!(e.revision, revision);
}

#[test]
fn removing_the_last_layer_is_refused_with_no_step() {
    let mut e = editor();
    let only = e.layer_id(0).expect("exists");
    assert_eq!(
        e.apply(LayerCmd::Remove { layer: only }),
        Err(LimitError::LastLayer)
    );
    assert!(!e.history.can_undo());
}

#[test]
fn visibility_is_saved_but_is_never_a_step_and_undo_leaves_it_alone() {
    let mut e = editor();
    let layer = e.layer_id(0).expect("exists");
    e.begin(Tool::Pencil, 1, 1).expect("begin");
    e.commit();
    let (depth, revision) = (e.history.undo_depth(), e.revision);
    e.apply(LayerCmd::SetVisible {
        layer,
        visible: false,
    })
    .expect("found");
    assert_eq!(e.history.undo_depth(), depth);
    assert_ne!(e.revision, revision);
    e.undo();
    assert!(!e.doc.layers[0].visible);
}

#[test]
fn a_change_that_fails_halfway_is_taken_back_whole() {
    let mut e = editor();
    let (depth, revision) = (e.history.undo_depth(), e.revision);
    let before = e.doc.clone();
    let result: Result<(), LimitError> = e.transact(|d| {
        d.add_frame(1, false)?;
        d.layers[0].opacity = 3;
        d.frames[0].duration_ms = 999;
        Err(LimitError::NoSuchItem)
    });
    assert_eq!(result, Err(LimitError::NoSuchItem));
    assert_eq!(e.doc, before);
    assert_eq!(e.history.undo_depth(), depth);
    assert_eq!(e.revision, revision);
}

fn lifted_and_moved() -> Editor {
    let mut e = painted();
    e.select(Mask::rect(0, 0, 4, 4), SelectOp::Replace);
    e.lift(false);
    e.move_floating(2, 1);
    e
}

fn painted() -> Editor {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Palette).expect("valid"));
    e.color = rgba(200, 10, 10, 255);
    e.begin(Tool::Pencil, 1, 1).expect("begin");
    e.commit();
    e
}

#[test]
fn floating_pixels_and_the_change_after_them_are_one_step() {
    let mut e = lifted_and_moved();
    let (before, depth) = (e.doc.clone(), e.history.undo_depth());
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("room");
    assert!(!e.is_floating());
    assert_eq!(e.history.undo_depth(), depth + 1);
    assert_eq!(e.color_at(3, 2), e.color, "the pixels landed");
    assert!(e.undo());
    assert_eq!(e.doc, before);
    assert_eq!(e.selection().map(|m| m.bounds()), Some((0, 0, 4, 4)));
}

#[test]
fn a_change_that_fails_gives_floating_pixels_back() {
    let mut e = lifted_and_moved();
    let (before, depth) = (e.doc.clone(), e.history.undo_depth());
    let shown = e.selection().cloned();
    let layer = e.layer_id(0).expect("layer");
    assert!(e.apply(LayerCmd::Remove { layer }).is_err());
    assert!(e.is_floating());
    assert_eq!(e.doc, before);
    assert_eq!(e.selection().cloned(), shown);
    assert_eq!(e.history.undo_depth(), depth);
    e.settle_floating();
    e.anchor();
    assert_eq!(e.color_at(3, 2), e.color, "they can still be applied");
}

#[test]
fn a_command_that_changes_nothing_is_no_step() {
    let mut e = painted();
    let depth = e.history.undo_depth();
    let outcome = e.apply(FrameCmd::Move { from: 0, to: 0 }).expect("fine");
    assert!(!outcome.changed);
    assert_eq!(e.history.undo_depth(), depth);
}

#[test]
fn a_value_dragged_back_to_where_it_began_is_no_step() {
    let mut e = painted();
    let layer = e.layer_id(0).expect("layer");
    let depth = e.history.undo_depth();
    let opacity = e.doc.layers[0].opacity;
    e.apply(LayerCmd::SetOpacity { layer, opacity: 3 })
        .expect("set");
    e.apply(LayerCmd::SetOpacity { layer, opacity })
        .expect("set");
    e.apply(StepCmd::EndDrag).expect("end");
    assert_eq!(e.history.undo_depth(), depth);
}

#[test]
fn a_cancelled_stroke_leaves_the_color_table_as_it_was() {
    let mut e = painted();
    let table = e.doc.colors.len();
    e.color = rgba(1, 2, 3, 255);
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 5,
        y: 5,
    })
    .expect("begin");
    assert_eq!(e.doc.colors.len(), table + 1);
    e.apply(DrawCmd::Cancel).expect("cancel");
    assert_eq!(e.doc.colors.len(), table);
}

#[test]
fn undoing_a_stroke_takes_back_the_color_it_added() {
    let mut e = painted();
    let table = e.doc.colors.len();
    e.color = rgba(1, 2, 3, 255);
    e.apply(DrawCmd::Stroke {
        tool: Tool::Pencil,
        path: vec![(5, 5), (6, 5)],
    })
    .expect("stroke");
    assert_eq!(e.doc.colors.len(), table + 1);
    assert!(e.undo());
    assert_eq!(e.doc.colors.len(), table);
    assert!(e.redo());
    assert_eq!(e.color_at(5, 5), rgba(1, 2, 3, 255));
}

#[test]
fn a_gesture_that_goes_nowhere_leaves_no_stop_and_no_change() {
    let mut e = painted();
    e.select(Mask::rect(0, 0, 4, 4), SelectOp::Replace);
    e.lift(false);
    e.gesture_begin();
    e.move_floating(3, 0);
    e.move_floating(-3, 0);
    e.gesture_end();
    assert_eq!(e.floating_stamp(), 0, "nothing to protect");
    let depth = e.history.undo_depth();
    e.anchor();
    assert_eq!(e.history.undo_depth(), depth);
}

#[test]
fn undo_redo_and_scrub_say_whether_they_moved() {
    let mut e = editor();
    assert!(!e.apply(StepCmd::Undo).expect("undo").changed);
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Palette,
    })
    .expect("add");
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Palette,
    })
    .expect("add");
    assert!(e.apply(StepCmd::Undo).expect("undo").changed);
    assert_eq!(e.doc.layers.len(), 2);
    assert!(e.apply(StepCmd::Redo).expect("redo").changed);
    assert!(!e.apply(StepCmd::Redo).expect("redo").changed);
    assert!(
        e.apply(StepCmd::ScrubTo { depth: 0 })
            .expect("scrub")
            .changed
    );
    assert_eq!(e.doc.layers.len(), 1);
    assert!(
        !e.apply(StepCmd::ScrubTo { depth: 0 })
            .expect("scrub")
            .changed
    );
    assert!(
        e.apply(StepCmd::ScrubTo { depth: 99 })
            .expect("scrub")
            .changed
    );
    assert_eq!(e.doc.layers.len(), 3);
}

type Seen = (
    Document,
    Option<std::sync::Arc<Mask>>,
    usize,
    usize,
    u64,
    usize,
    usize,
    bool,
);

fn seen(e: &Editor) -> Seen {
    (
        e.doc.clone(),
        e.selection().cloned(),
        e.history.undo_depth(),
        e.history.redo_depth(),
        e.revision,
        e.layer,
        e.frame,
        e.is_floating(),
    )
}

fn refused_changes_nothing(e: &mut Editor, command: Command) {
    let before = seen(e);
    e.set_memory_check(|_| false);
    assert_eq!(
        e.apply(command.clone()),
        Err(LimitError::NoMemory),
        "{command:?}"
    );
    e.set_memory_check(|_| true);
    assert!(seen(e) == before, "{command:?} changed something");
}

fn commands_for(e: &Editor) -> Vec<Command> {
    let layer = e.layer_id(0).expect("layer");
    let mut commands = layer_commands(layer);
    commands.extend(frame_commands(layer));
    commands.extend(canvas_commands());
    commands
}

fn layer_commands(layer: LayerId) -> Vec<Command> {
    vec![
        LayerCmd::Add {
            at: 0,
            mode: ColorMode::Rgba,
        }
        .into(),
        LayerCmd::Remove { layer }.into(),
        LayerCmd::Move { layer, to: 0 }.into(),
        LayerCmd::SetOpacity { layer, opacity: 3 }.into(),
        LayerCmd::SetVisible {
            layer,
            visible: false,
        }
        .into(),
        LayerCmd::SetBlend {
            layer,
            blend: BlendMode::Multiply,
        }
        .into(),
        LayerCmd::SetMode {
            layer,
            mode: ColorMode::Rgba,
        }
        .into(),
    ]
}

fn frame_commands(layer: LayerId) -> Vec<Command> {
    vec![
        FrameCmd::Add { at: 1, copy: true }.into(),
        FrameCmd::Remove { from: 0, to: 0 }.into(),
        FrameCmd::Move { from: 0, to: 0 }.into(),
        FrameCmd::Retime {
            from: 0,
            durations: vec![77],
        }
        .into(),
        FrameCmd::HoldPrevious { layer, frame: 0 }.into(),
        FrameCmd::HoldAcross {
            layer,
            source: 0,
            from: 0,
            to: 0,
        }
        .into(),
        FrameCmd::Release {
            layer,
            from: 0,
            to: 0,
        }
        .into(),
    ]
}

fn canvas_commands() -> Vec<Command> {
    let shape = || Box::new(Mask::rect(0, 0, 4, 4));
    let clip = std::sync::Arc::new(Clip::from_image(1, 1, vec![rgba(9, 9, 9, 255)]));
    let (color, tool) = (rgba(7, 7, 7, 255), Tool::Pencil);
    vec![
        TagCmd::Add {
            name: "x".into(),
            from: 0,
            to: 0,
        }
        .into(),
        PaletteCmd::AddSwatch { color }.into(),
        PaletteCmd::RemoveSwatch { id: 1 }.into(),
        PaletteCmd::Recolor { id: 1, color }.into(),
        DrawCmd::Begin { tool, x: 2, y: 2 }.into(),
        DrawCmd::Stroke {
            tool,
            path: vec![(1, 1), (2, 2)],
        }
        .into(),
        DrawCmd::EraseCell { x: 1, y: 1 }.into(),
        SelectionCmd::All.into(),
        SelectionCmd::Invert.into(),
        FloatingCmd::Paste { clip, x: 1, y: 1 }.into(),
        FloatingCmd::Scale {
            x: 0,
            y: 0,
            w: 2,
            h: 2,
        }
        .into(),
        SelectionCmd::Select {
            shape: shape(),
            op: SelectOp::Replace,
        }
        .into(),
    ]
}

#[test]
fn a_command_refused_by_the_memory_check_changes_nothing() {
    let mut plain = painted();
    plain
        .apply(SelectionCmd::Select {
            shape: Box::new(Mask::rect(0, 0, 4, 4)),
            op: SelectOp::Replace,
        })
        .expect("select");
    let mut lifted = lifted_and_moved();
    for command in commands_for(&plain) {
        let needs_floating = matches!(command, Command::Floating(FloatingCmd::Scale { .. }));
        let selection_only = matches!(
            command,
            Command::Selection(SelectionCmd::Select { .. } | SelectionCmd::None)
        );
        if needs_floating || selection_only {
            continue;
        }
        refused_changes_nothing(&mut plain, command);
    }
    for command in commands_for(&lifted) {
        refused_changes_nothing(&mut lifted, command);
    }
    refused_changes_nothing(&mut plain, FloatingCmd::Lift { copy: false }.into());
    refused_changes_nothing(&mut plain, FloatingCmd::FlipHorizontal.into());
    refused_changes_nothing(&mut plain, FloatingCmd::Rotate { clockwise: true }.into());
    refused_changes_nothing(&mut plain, DrawCmd::DeleteSelected.into());
    refused_changes_nothing(&mut plain, FloatingCmd::FlipVertical.into());
    refused_changes_nothing(&mut lifted, FloatingCmd::Anchor.into());
    let temporary = SelectionCmd::Temporary {
        shape: Box::new(Mask::rect(0, 0, 2, 2)),
    };
    refused_changes_nothing(&mut lifted, temporary.into());
}

#[test]
fn every_change_to_a_tag_can_be_refused() {
    let mut e = painted();
    e.apply(TagCmd::Add {
        name: "x".into(),
        from: 0,
        to: 0,
    })
    .expect("added");
    e.apply(StepCmd::EndDrag).expect("ended");
    let name = "y".to_string();
    for command in [
        TagCmd::SetName { tag: 0, name },
        TagCmd::CycleDirection { tag: 0 },
        TagCmd::Remove { tag: 0 },
    ] {
        refused_changes_nothing(&mut e, command.into());
    }
}

#[test]
fn steps_and_pointer_moves_on_an_ordinary_canvas_never_ask_for_memory() {
    let mut e = lifted_and_moved();
    e.set_memory_check(|_| false);
    let moved = FloatingCmd::Move {
        dx: 1,
        dy: 0,
        join: true,
    };
    for command in [
        Command::Step(StepCmd::Undo),
        Command::Step(StepCmd::Redo),
        Command::Step(StepCmd::EndDrag),
        Command::Floating(FloatingCmd::GestureBegin),
        Command::Floating(moved),
        Command::Floating(FloatingCmd::GestureEnd),
    ] {
        assert!(e.apply(command).is_ok());
    }
}

#[test]
fn a_stroke_once_begun_is_never_refused() {
    let mut e = painted();
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 2,
        y: 2,
    })
    .expect("begin");
    e.set_memory_check(|_| false);
    assert!(e.apply(DrawCmd::To { x: 5, y: 5 }).is_ok());
    assert!(e.apply(DrawCmd::Commit).is_ok());
    assert_eq!(e.color_at(5, 5), e.color);
    assert!(e.apply(DrawCmd::Cancel).is_ok());
}

#[test]
fn the_real_check_passes_small_requests_and_refuses_the_impossible() {
    assert!(super::system_has_room(64));
    assert!(super::system_has_room(2 << 20));
    assert!(!super::system_has_room(usize::MAX / 2));
}

#[test]
fn a_tag_keeps_its_id_through_renames_frame_edits_removal_and_undo() {
    let mut e = painted();
    e.apply(FrameCmd::Add { at: 1, copy: false })
        .expect("frame");
    e.apply(FrameCmd::Add { at: 2, copy: false })
        .expect("frame");
    for (name, at) in [("idle", 0), ("walk", 1), ("run", 2)] {
        e.apply(TagCmd::Add {
            name: name.into(),
            from: at,
            to: at,
        })
        .expect("tag");
        e.apply(StepCmd::EndDrag).expect("ended");
    }
    let ids: Vec<_> = e.doc().tags.iter().map(|t| t.id).collect();
    assert!(ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2]);
    let walk = ids[1];
    let walk_now = |e: &Editor| e.doc().tags.iter().find(|t| t.id == walk).cloned();
    e.apply(TagCmd::SetName {
        tag: 1,
        name: "stroll".into(),
    })
    .expect("renamed");
    e.apply(StepCmd::EndDrag).expect("ended");
    e.apply(FrameCmd::Add { at: 0, copy: false })
        .expect("frame");
    let moved = walk_now(&e).expect("still there");
    assert_eq!((moved.name.as_str(), moved.from), ("stroll", 2));
    e.apply(TagCmd::Remove { tag: 0 }).expect("removed");
    assert_eq!(e.doc().tags[0].id, walk);
    e.apply(TagCmd::Remove { tag: 0 }).expect("removed");
    assert!(walk_now(&e).is_none());
    e.apply(StepCmd::Undo).expect("undo");
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(e.doc().tags[1].id, walk, "undo brings back the same tag");
}

#[test]
fn tags_alike_in_everything_but_id_are_equal() {
    let tag = |id| crate::Tag {
        name: "walk".into(),
        from: 0,
        to: 1,
        direction: crate::Direction::Forward,
        repeat: 0,
        id,
    };
    assert_eq!(tag(crate::TagId::fresh()), tag(crate::TagId::fresh()));
    assert_ne!(crate::TagId::fresh(), crate::TagId::fresh());
}

#[test]
fn a_layer_name_loses_direction_and_zero_width_characters() {
    let mut e = editor();
    let layer = e.layer_id(0).expect("exists");
    e.apply(LayerCmd::Rename {
        layer,
        name: "Sk\u{202E}y\u{200B}\n".into(),
    })
    .expect("renames");
    assert_eq!(e.doc.layers[0].name, "Sky");
}
