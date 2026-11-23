//! One test for each rule of section 4 of `docs/links-and-tween.md`: what every command does to
//! a link that reaches across layers, and what it refuses.

use crate::cel_drag::{CelDrag, DragKind, Landing};
use crate::color::rgba;
use crate::command::{CanvasCmd, CelCmd, Command, DrawCmd, FrameCmd, LayerCmd, LayerId};
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::{Editor, Tool};
use crate::layer::{LayerKind, Turn};

const INK: u32 = rgba(10, 200, 30, 255);

/// Three RGBA layers of three frames, every cel with a dot of its own.
fn session() -> Editor {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, ColorMode::Rgba).expect("room");
    doc.add_layer(2, ColorMode::Rgba).expect("room");
    let mut e = Editor::new(doc);
    for at in 1..3 {
        e.transact(|d| d.add_frame(at, false)).expect("room");
    }
    e.pixel_perfect = false;
    e.color = INK;
    for layer in 0..3 {
        for frame in 0..3 {
            dot(&mut e, layer, frame, layer as i32 * 4 + frame as i32);
        }
    }
    e
}

fn dot(e: &mut Editor, layer: usize, frame: usize, x: i32) {
    (e.layer, e.frame) = (layer, frame);
    e.begin(Tool::Pencil, x, 3).expect("begin");
    e.commit();
}

fn drag(from: (usize, usize), to: (usize, usize), kind: DragKind) -> Command {
    CelCmd::Drag(CelDrag {
        layers: (from.0, from.0),
        frames: (from.1, from.1),
        to_layer: to.0,
        to_frame: to.1,
        kind,
        landing: Landing::Overwrite,
    })
    .into()
}

/// Links the first cel of layer 0 to the second of layer 2, reaching across layer 1.
fn linked() -> Editor {
    let mut e = session();
    e.apply(drag((0, 0), (2, 1), DragKind::Link))
        .expect("linked");
    e
}

fn link(e: &Editor, layer: usize, frame: usize) -> u32 {
    e.doc.layers[layer].link(frame)
}

fn shares(e: &Editor, a: (usize, usize), b: (usize, usize)) -> bool {
    e.doc.layers[a.0]
        .cel(a.1)
        .same_image(e.doc.layers[b.0].cel(b.1))
}

fn id(e: &Editor, index: usize) -> LayerId {
    e.layer_id(index).expect("layer")
}

fn begin(x: i32, y: i32) -> DrawCmd {
    DrawCmd::Begin {
        tool: Tool::Pencil,
        x,
        y,
    }
}

fn lock(e: &mut Editor, index: usize) {
    let layer = id(e, index);
    e.apply(LayerCmd::SetLocked {
        layer,
        locked: true,
    })
    .expect("locked");
}

#[test]
fn a_link_across_layers_takes_the_whole_source_cel() {
    let mut e = session();
    e.doc.layers[0].move_cels(&[0], 3, 1);
    e.apply(drag((0, 0), (2, 1), DragKind::Link))
        .expect("linked");
    assert_ne!(link(&e, 0, 0), 0);
    assert_eq!(link(&e, 2, 1), link(&e, 0, 0));
    assert!(shares(&e, (0, 0), (2, 1)));
    assert_eq!(e.doc.layers[2].cel(1).position(), (3, 1), "offset included");
    assert_eq!((e.layer, e.frame), (2, 1));
    assert_eq!(e.doc.check_links(), Ok(()));
    assert!(e.undo());
    assert_eq!((link(&e, 0, 0), link(&e, 2, 1)), (0, 0));
    assert!(!shares(&e, (0, 0), (2, 1)));
}

#[test]
fn a_link_joins_only_layers_of_one_color_mode() {
    let mut e = session();
    e.transact(|d| d.set_layer_mode(2, ColorMode::Palette))
        .expect("converted");
    let refused = e.apply(drag((0, 0), (2, 1), DragKind::Link));
    assert_eq!(refused.err(), Some(LimitError::LinkModes));
    let copied = e.apply(drag((0, 0), (2, 1), DragKind::Copy));
    assert_eq!(
        copied.err(),
        Some(LimitError::CelPlacement),
        "as for any cel"
    );
}

#[test]
fn only_paint_layers_share_a_drawing() {
    for kind in [LayerKind::Reference, LayerKind::Group] {
        let mut e = session();
        e.doc.layers[2].kind = kind;
        let to = e.apply(drag((0, 0), (2, 1), DragKind::Link));
        assert_eq!(to.err(), Some(LimitError::LinkKind), "{kind:?} landing");
        let from = e.apply(drag((2, 1), (0, 0), DragKind::Link));
        assert_eq!(from.err(), Some(LimitError::LinkKind), "{kind:?} source");
    }
    let mut e = session();
    e.doc.layers[2].kind = LayerKind::Reference;
    e.apply(drag((2, 1), (0, 0), DragKind::Copy))
        .expect("a reference can still be copied");
}

#[test]
fn a_locked_row_refuses_a_link_across_layers() {
    let mut e = session();
    e.doc.layers[0].locked = true;
    let refused = e.apply(drag((0, 0), (2, 1), DragKind::Link));
    assert_eq!(refused.err(), Some(LimitError::Locked), "a locked source");
    e.doc.layers[0].locked = false;
    e.doc.layers[2].locked = true;
    let refused = e.apply(drag((0, 0), (2, 1), DragKind::Link));
    assert_eq!(refused.err(), Some(LimitError::Locked), "a locked landing");
}

#[test]
fn drawing_on_a_link_with_a_locked_member_is_refused() {
    let mut e = linked();
    lock(&mut e, 2);
    (e.layer, e.frame) = (0, 0);
    assert_eq!(e.apply(begin(9, 9)).err(), Some(LimitError::SharedLocked));
    let erase = DrawCmd::EraseCell { x: 0, y: 3 };
    assert_eq!(e.apply(erase).err(), Some(LimitError::SharedLocked));
    let turn = CelCmd::Turn {
        layer: id(&e, 0),
        frame: 0,
        turn: Turn::FlipHorizontal,
    };
    assert_eq!(e.apply(turn).err(), Some(LimitError::SharedLocked));
    e.frame = 1;
    e.apply(begin(9, 9)).expect("a cel in no link is drawn on");
    e.commit();
}

#[test]
fn a_hidden_member_does_not_refuse_drawing() {
    let mut e = linked();
    e.doc.layers[2].visible = false;
    dot(&mut e, 0, 0, 9);
    assert_ne!(e.doc.layers[2].cel(1).get(9, 3), 0);
}

#[test]
fn locking_a_member_layer_puts_down_a_stroke_in_progress() {
    let mut e = linked();
    (e.layer, e.frame) = (0, 0);
    e.begin(Tool::Pencil, 9, 9).expect("begin");
    lock(&mut e, 2);
    assert!(!e.is_stroking());
    assert_ne!(
        e.doc.layers[2].cel(1).get(9, 9),
        0,
        "written before the lock"
    );
}

#[test]
fn holding_a_frame_that_was_in_a_link_elsewhere_leaves_it() {
    let mut e = linked();
    let layer = id(&e, 2);
    e.apply(FrameCmd::HoldPrevious { layer, frame: 1 })
        .expect("held");
    assert_eq!(link(&e, 0, 0), 0, "the link left with one cel ends");
    assert!(shares(&e, (2, 0), (2, 1)));
    assert_eq!(e.doc.check_links(), Ok(()));
}

#[test]
fn releasing_takes_the_cel_out_whichever_layers_the_link_reaches() {
    let mut e = linked();
    let layer = id(&e, 0);
    e.apply(FrameCmd::Release {
        layer,
        from: 0,
        to: 0,
    })
    .expect("released");
    assert_eq!((link(&e, 0, 0), link(&e, 2, 1)), (0, 0));
    assert!(
        shares(&e, (0, 0), (2, 1)),
        "it shows the drawing until drawn on"
    );
    dot(&mut e, 0, 0, 12);
    assert_eq!(e.doc.layers[2].cel(1).get(12, 3), 0);
}

#[test]
fn moving_a_cel_moves_its_hold_and_leaves_other_layers_in_place() {
    let mut e = linked();
    let layer = id(&e, 0);
    e.apply(FrameCmd::HoldPrevious { layer, frame: 1 })
        .expect("held");
    let moved = CelCmd::Move {
        layer,
        frame: 0,
        dx: 2,
        dy: 0,
    };
    e.apply(moved).expect("moved");
    assert_eq!(e.doc.layers[0].cel(1).position(), (2, 0));
    assert_eq!(e.doc.layers[2].cel(1).position(), (0, 0));
    assert!(shares(&e, (0, 1), (2, 1)), "still one drawing");
}

#[test]
fn turning_a_drawing_turns_every_member_and_needs_them_in_one_place() {
    let mut e = linked();
    let turn = |e: &Editor| CelCmd::Turn {
        layer: id(e, 2),
        frame: 1,
        turn: Turn::FlipHorizontal,
    };
    e.apply(turn(&e)).expect("turned");
    assert_ne!(e.doc.layers[0].cel(0).get(15, 3), 0);
    assert!(shares(&e, (0, 0), (2, 1)));
    assert_eq!(link(&e, 0, 0), link(&e, 2, 1));
    e.doc.layers[0].move_cels(&[0], 1, 0);
    assert_eq!(e.apply(turn(&e)).err(), Some(LimitError::UnequalHold));
}

#[test]
fn a_new_frame_after_a_linked_cel_joins_its_link() {
    let mut e = linked();
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    assert_eq!(link(&e, 0, 1), link(&e, 0, 0));
    assert_eq!(
        link(&e, 2, 2),
        link(&e, 0, 0),
        "the landing cel is now frame 2"
    );
    assert_eq!(e.doc.check_links(), Ok(()));
}

#[test]
fn duplicated_frames_copy_a_link_wholly_inside_and_grow_one_reaching_out() {
    let mut e = session();
    e.apply(drag((0, 1), (2, 1), DragKind::Link))
        .expect("inside");
    e.apply(drag((1, 0), (2, 2), DragKind::Link))
        .expect("outside");
    e.apply(FrameCmd::Duplicate { from: 1, to: 2 })
        .expect("room");
    let (inside, outside) = (link(&e, 0, 1), link(&e, 1, 0));
    assert_ne!(link(&e, 0, 3), inside, "a link of its own");
    assert_ne!(link(&e, 0, 3), 0);
    assert_eq!(link(&e, 0, 3), link(&e, 2, 3), "shared by the copies");
    assert_eq!(link(&e, 2, 4), outside, "grows over the copies");
    assert!(shares(&e, (0, 3), (0, 1)), "copies share every drawing");
    assert_eq!(e.doc.check_links(), Ok(()));
}

#[test]
fn a_duplicated_layer_leaves_a_link_with_other_layers_to_the_original() {
    let mut e = linked();
    let layer = id(&e, 0);
    e.apply(LayerCmd::Duplicate { layer }).expect("room");
    assert_eq!(link(&e, 1, 0), 0, "the copy is in no link");
    assert!(
        shares(&e, (1, 0), (0, 0)),
        "and shares the drawing until drawn on"
    );
    assert_ne!(link(&e, 0, 0), 0);
    assert_eq!(link(&e, 0, 0), link(&e, 3, 1));
    assert_eq!(e.doc.check_links(), Ok(()));
}

#[test]
fn a_duplicated_group_copies_a_link_wholly_inside_it() {
    let mut e = session();
    e.apply(drag((0, 0), (1, 1), DragKind::Link))
        .expect("linked");
    e.transact(|d| {
        d.group_layer(0)?;
        d.step_layer(2, false).map(drop)
    })
    .expect("grouped");
    let group = e.doc.layers.iter().position(|l| l.kind == LayerKind::Group);
    let (low, high) = e.doc.block(group.expect("a group"));
    let in_link = |l: usize| link(&e, l, 0) != 0 || link(&e, l, 1) != 0;
    let linked: Vec<usize> = (low..=high).filter(|&l| in_link(l)).collect();
    assert_eq!(
        linked.len(),
        2,
        "both linked layers are in the group {low}..={high}"
    );
    let layer = id(&e, high);
    e.apply(LayerCmd::Duplicate { layer }).expect("room");
    let shift = high - low + 1;
    let (a, b) = (linked[0], linked[1]);
    let copy = link(&e, a + shift, 0);
    assert_ne!(copy, 0);
    assert_ne!(copy, link(&e, a, 0));
    assert_eq!(link(&e, b + shift, 1), copy);
    assert_eq!(e.doc.check_links(), Ok(()));
}

#[test]
fn removing_a_member_ends_a_link_left_with_one_cel() {
    let mut e = linked();
    let layer = id(&e, 2);
    e.apply(LayerCmd::Remove { layer }).expect("removed");
    assert_eq!(link(&e, 0, 0), 0);
    let mut e = linked();
    e.apply(FrameCmd::Remove { from: 1, to: 1 })
        .expect("removed");
    assert_eq!(link(&e, 0, 0), 0);
    assert_eq!(e.doc.check_links(), Ok(()));
}

#[test]
fn moving_frames_and_layers_keeps_every_link() {
    let mut e = linked();
    e.apply(FrameCmd::Move { from: 0, to: 2 }).expect("moved");
    let layer = id(&e, 0);
    e.apply(LayerCmd::Move { layer, to: 1 }).expect("moved");
    let table = e.doc.link_table();
    assert_eq!(table.links().count(), 1);
    assert_eq!(table.all().len(), 2);
    assert_eq!(e.doc.check_links(), Ok(()));
}

#[test]
fn merging_ends_links_reaching_the_merged_layer_and_keeps_the_rest() {
    let mut e = linked();
    e.apply(drag((2, 0), (0, 2), DragKind::Link))
        .expect("linked");
    let layer = id(&e, 2);
    e.apply(LayerCmd::MergeDown { layer }).expect("merged");
    assert_eq!(link(&e, 0, 0), 0, "its link reached the merged layers");
    assert_eq!(link(&e, 0, 2), 0);
    assert_eq!(e.doc.check_links(), Ok(()));
}

#[test]
fn converting_a_layer_ends_its_links_with_other_layers_and_keeps_its_holds() {
    let mut e = linked();
    let layer = id(&e, 0);
    e.apply(FrameCmd::HoldPrevious { layer, frame: 1 })
        .expect("held");
    let before = e.doc.clone();
    e.apply(LayerCmd::SetMode {
        layer,
        mode: ColorMode::Palette,
    })
    .expect("converted");
    assert_ne!(link(&e, 0, 0), 0);
    assert_eq!(link(&e, 0, 0), link(&e, 0, 1), "its own hold stays");
    assert_eq!(link(&e, 2, 1), 0, "the link with layer 2 ended");
    assert_eq!(e.doc.check_links(), Ok(()));
    assert!(e.undo());
    assert_eq!(e.doc, before, "undo brings the link back");
}

#[test]
fn a_canvas_resize_keeps_links_and_offsets() {
    let mut e = linked();
    e.apply(CanvasCmd::Resize {
        width: 20,
        height: 20,
        dx: 2,
        dy: 1,
    })
    .expect("resized");
    assert_eq!(link(&e, 0, 0), link(&e, 2, 1));
    assert_eq!(e.doc.layers[2].cel(1).position(), (2, 1));
    assert_eq!(e.doc.check_links(), Ok(()));
}

#[test]
fn layer_settings_leave_links_alone() {
    let mut e = linked();
    let layer = id(&e, 2);
    e.apply(LayerCmd::SetVisible {
        layer,
        visible: false,
    })
    .expect("hidden");
    e.apply(LayerCmd::SetOpacity { layer, opacity: 40 })
        .expect("set");
    e.apply(LayerCmd::Rename {
        layer,
        name: "x".into(),
    })
    .expect("named");
    assert_ne!(link(&e, 0, 0), 0);
    assert_eq!(link(&e, 0, 0), link(&e, 2, 1));
}

#[test]
fn moving_only_this_cel_leaves_the_rest_of_its_link_in_place() {
    let mut e = linked();
    let layer = id(&e, 0);
    e.apply(FrameCmd::HoldPrevious { layer, frame: 1 })
        .expect("held");
    let one = |dx| CelCmd::MoveOne {
        layer,
        frame: 1,
        dx,
        dy: 0,
    };
    let depth = e.history().undo_depth();
    e.apply(one(2)).expect("moved");
    e.apply(one(1)).expect("moved");
    assert_eq!(e.doc.layers[0].cel(1).position(), (3, 0));
    assert_eq!(e.doc.layers[0].cel(0).position(), (0, 0));
    assert_eq!(e.doc.layers[2].cel(1).position(), (0, 0));
    assert!(
        shares(&e, (0, 0), (0, 1)),
        "still one drawing, at two places"
    );
    assert_eq!(link(&e, 0, 1), link(&e, 0, 0));
    assert_eq!(e.history().undo_depth(), depth + 1, "one drag");
    let hold = CelCmd::Move {
        layer,
        frame: 1,
        dx: 1,
        dy: 0,
    };
    e.apply(hold).expect("moved");
    assert_eq!(
        e.history().undo_depth(),
        depth + 2,
        "moving the hold is another drag"
    );
    assert_eq!(e.doc.layers[0].cel(0).position(), (1, 0));
    assert_eq!(e.doc.layers[0].cel(1).position(), (4, 0));
    assert!(e.undo());
    assert!(e.undo());
    assert_eq!(e.doc.layers[0].cel(1).position(), (0, 0));
}

#[test]
fn a_stroke_reaches_members_at_every_offset() {
    let mut e = linked();
    let layer = id(&e, 2);
    let one = CelCmd::MoveOne {
        layer,
        frame: 1,
        dx: 0,
        dy: 5,
    };
    e.apply(one).expect("moved");
    dot(&mut e, 0, 0, 13);
    assert_ne!(e.doc.layers[2].cel(1).get(13, 8), 0);
    assert_eq!(e.doc.layers[2].cel(1).get(13, 3), 0);
}

#[test]
fn a_reference_moves_whole_even_when_one_cel_is_asked() {
    let mut e = session();
    e.doc.layers[1].kind = LayerKind::Reference;
    e.doc
        .hold_across(1, 0, 0, e.doc.frames.len() - 1)
        .expect("frames");
    let layer = id(&e, 1);
    let one = CelCmd::MoveOne {
        layer,
        frame: 1,
        dx: 3,
        dy: -2,
    };
    e.apply(one).expect("moved");
    let places: Vec<(i16, i16)> = e.doc.layers[1]
        .cels()
        .iter()
        .map(|c| c.position())
        .collect();
    assert!(places.iter().all(|&p| p == (3, -2)), "{places:?}");
}

#[test]
fn a_recovered_floating_on_a_drawing_shared_with_a_locked_layer_is_refused() {
    use crate::command::{FloatingCmd, SelectionCmd};
    let mut e = linked();
    (e.layer, e.frame) = (0, 0);
    e.apply(SelectionCmd::All).expect("selected");
    e.apply(FloatingCmd::Lift { copy: false }).expect("lifted");
    let shift = FloatingCmd::Move {
        dx: 4,
        dy: 4,
        join: false,
    };
    e.apply(shift).expect("moved");
    let snapshot = e.floating_snapshot().expect("floating");
    let other = id(&e, 2);
    e.apply(LayerCmd::SetLocked {
        layer: other,
        locked: true,
    })
    .expect("locked");
    assert!(!e.is_floating(), "locking puts the floating down");
    let before = e.doc.clone();
    assert_eq!(e.restore_floating(snapshot), Err(LimitError::SharedLocked));
    assert!(!e.is_floating());
    assert_eq!(e.doc.layers, before.layers, "nothing changed");
}
