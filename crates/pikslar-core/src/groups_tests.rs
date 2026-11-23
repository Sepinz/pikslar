//! Layer groups: what a group holds, what it hides and locks, and the commands that make,
//! undo and move them.

use crate::command::{DrawCmd, LayerCmd, StepCmd};
use crate::document::{ColorMode, Document, LimitError, MAX_LAYERS};
use crate::editor::Editor;
use crate::groups::MAX_GROUP_DEPTH;
use crate::layer::LayerKind;

/// A document from rows listed top first. Leading spaces give the depth; a name starting
/// with `G` is a group.
pub(crate) fn layout(rows: &[&str]) -> Document {
    let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("valid");
    for _ in 1..rows.len() {
        doc.add_layer(0, ColorMode::Rgba).expect("layer");
    }
    for (index, row) in rows.iter().rev().enumerate() {
        let name = row.trim_start();
        let layer = &mut doc.layers[index];
        layer.depth = (row.len() - name.len()) as u8;
        layer.name = name.into();
        if name.starts_with('G') {
            layer.kind = LayerKind::Group;
        }
    }
    doc.check_groups().expect("a valid layout");
    doc
}

/// The rows top first, written as `layout` takes them.
fn rows(doc: &Document) -> Vec<String> {
    let row = |l: &crate::layer::Layer| format!("{}{}", " ".repeat(l.depth.into()), l.name);
    doc.layers.iter().rev().map(row).collect()
}

fn index_of(doc: &Document, name: &str) -> usize {
    doc.layers
        .iter()
        .position(|l| l.name == name)
        .expect("named")
}

fn editor(rows: &[&str], active: &str) -> Editor {
    let doc = layout(rows);
    let mut e = Editor::new(doc);
    e.layer = index_of(&e.doc, active);
    e
}

fn id(e: &Editor, name: &str) -> crate::command::LayerId {
    e.layer_id(index_of(&e.doc, name)).expect("layer")
}

fn step(e: &mut Editor, name: &str, up: bool) -> Result<(), LimitError> {
    let layer = id(e, name);
    e.apply(LayerCmd::Step { layer, up }).map(|_| ())
}

const NESTED: [&str; 6] = ["top", "G1", " a", " G2", "  b", "low"];

#[test]
fn a_group_holds_the_deeper_rows_straight_below_it() {
    let doc = layout(&NESTED);
    let at = |n| index_of(&doc, n);
    assert_eq!(doc.block(at("G1")), (at("low") + 1, at("G1")));
    assert_eq!(doc.block(at("G2")), (at("b"), at("G2")));
    assert_eq!(doc.block(at("a")), (at("a"), at("a")));
    assert_eq!(doc.parent(at("b")), Some(at("G2")));
    assert_eq!(doc.parent(at("G2")), Some(at("G1")));
    assert_eq!(doc.parent(at("low")), None);
    assert_eq!(doc.parent(at("top")), None);
}

#[test]
fn the_order_rule_refuses_orphans_gaps_and_depths_past_the_limit() {
    let mut doc = layout(&NESTED);
    let top = doc.layers.len() - 1;
    doc.layers[top].depth = 1;
    assert_eq!(doc.check_groups(), Err(LimitError::GroupOrder));
    let mut doc = layout(&NESTED);
    let a = index_of(&doc, "a");
    doc.layers[a].depth = 2;
    assert_eq!(doc.check_groups(), Err(LimitError::GroupOrder), "a gap");
    doc.layers[a].depth = MAX_GROUP_DEPTH + 1;
    assert_eq!(doc.check_groups(), Err(LimitError::TooDeep));
    let deepest = ["G1", " G2", "  G3", "   G4", "    x"];
    assert!(layout(&deepest).check_groups().is_ok());
}

#[test]
fn hiding_or_locking_a_group_reaches_everything_in_it() {
    let mut doc = layout(&NESTED);
    let at = |d: &Document, n| index_of(d, n);
    let g2 = at(&doc, "G2");
    doc.layers[g2].visible = false;
    let g1 = at(&doc, "G1");
    doc.layers[g1].locked = true;
    let shown = doc.shown_layers();
    assert!(!shown[at(&doc, "b")]);
    assert!(shown[at(&doc, "a")] && shown[at(&doc, "low")]);
    let locked = doc.locked_layers();
    for name in ["a", "G2", "b"] {
        assert!(locked[at(&doc, name)], "{name}");
        assert!(doc.locked_through(at(&doc, name)));
    }
    assert!(!locked[at(&doc, "top")] && !locked[at(&doc, "low")]);
    let picture = doc.picture_layers();
    assert!(!picture[g1] && !picture[at(&doc, "b")] && picture[at(&doc, "a")]);
    let screen = doc.screen_layers();
    assert_eq!(screen, picture, "no references here");
}

#[test]
fn grouping_wraps_the_layer_and_is_one_step() {
    let mut e = editor(&NESTED, "a");
    let layer = id(&e, "a");
    e.apply(LayerCmd::Group { layer }).expect("grouped");
    assert_eq!(e.doc.layers[e.layer].kind, LayerKind::Group);
    let name = e.doc.layers[e.layer].name.clone();
    let wrapped = [
        "top".to_string(),
        "G1".into(),
        format!(" {name}"),
        "  a".into(),
        " G2".into(),
        "  b".into(),
        "low".into(),
    ];
    assert_eq!(rows(&e.doc), wrapped);
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(rows(&e.doc), NESTED);
    e.apply(StepCmd::Redo).expect("redo");
    assert_eq!(rows(&e.doc), wrapped);
}

#[test]
fn grouping_a_group_takes_what_it_holds_along() {
    let mut e = editor(&NESTED, "G2");
    let layer = id(&e, "G2");
    e.apply(LayerCmd::Group { layer }).expect("grouped");
    let r = rows(&e.doc);
    assert_eq!(&r[3..], [" Group 3", "  G2", "   b", "low"]);
}

#[test]
fn grouping_past_the_depth_or_layer_limit_is_refused() {
    let mut e = editor(&["G1", " G2", "  G3", "   G4", "    x"], "x");
    let layer = id(&e, "x");
    assert_eq!(
        e.apply(LayerCmd::Group { layer }).map(|_| ()),
        Err(LimitError::TooDeep)
    );
    let mut e = editor(&["a"], "a");
    while e.doc.layers.len() < MAX_LAYERS {
        e.doc.add_layer(0, ColorMode::Rgba).expect("layer");
    }
    let layer = id(&e, "a");
    assert_eq!(
        e.apply(LayerCmd::Group { layer }).map(|_| ()),
        Err(LimitError::TooManyLayers)
    );
}

#[test]
fn ungrouping_lifts_the_rows_out_with_their_own_eye_and_lock() {
    let mut e = editor(&NESTED, "G1");
    let g1 = index_of(&e.doc, "G1");
    e.doc.layers[g1].visible = false;
    e.doc.layers[g1].locked = true;
    let layer = id(&e, "G1");
    e.apply(LayerCmd::Ungroup { layer }).expect("ungrouped");
    assert_eq!(rows(&e.doc), ["top", "a", "G2", " b", "low"]);
    let a = &e.doc.layers[index_of(&e.doc, "a")];
    assert!(a.visible && !a.locked, "never undone, so not changed");
    assert_eq!(e.doc.layers[e.layer].name, "a");
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(rows(&e.doc), NESTED);
    let layer = id(&e, "a");
    assert_eq!(
        e.apply(LayerCmd::Ungroup { layer }).map(|_| ()),
        Err(LimitError::NoSuchItem)
    );
}

#[test]
fn an_empty_group_that_is_the_only_layer_is_not_ungrouped() {
    let mut e = editor(&["G", " a"], "a");
    let layer = id(&e, "a");
    e.apply(LayerCmd::Remove { layer }).expect("removed");
    let layer = id(&e, "G");
    assert_eq!(
        e.apply(LayerCmd::Ungroup { layer }).map(|_| ()),
        Err(LimitError::LastLayer)
    );
    assert_eq!(rows(&e.doc), ["G"]);
}

#[test]
fn removing_a_group_removes_what_it_holds() {
    let mut e = editor(&NESTED, "G1");
    let layer = id(&e, "G1");
    e.apply(LayerCmd::Remove { layer }).expect("removed");
    assert_eq!(rows(&e.doc), ["top", "low"]);
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(rows(&e.doc), NESTED);
    let mut e = editor(&["G", " a"], "G");
    let layer = id(&e, "G");
    assert_eq!(
        e.apply(LayerCmd::Remove { layer }).map(|_| ()),
        Err(LimitError::LastLayer)
    );
}

#[test]
fn duplicating_a_group_copies_it_whole_above_itself() {
    let mut e = editor(&NESTED, "G2");
    let layer = id(&e, "G2");
    e.apply(LayerCmd::Duplicate { layer }).expect("duplicated");
    let r = rows(&e.doc);
    assert_eq!(&r[2..], [" a", " G2 copy", "  b copy", " G2", "  b", "low"]);
    assert_eq!(e.doc.layers[e.layer].name, "G2 copy");
    e.doc.check_groups().expect("still in order");
}

#[test]
fn stepping_up_swaps_with_a_sibling_and_goes_into_and_out_of_groups() {
    let mut e = editor(&NESTED, "low");
    step(&mut e, "low", true).expect("into G1 at its bottom");
    assert_eq!(&rows(&e.doc)[3..], [" G2", "  b", " low"]);
    step(&mut e, "low", true).expect("into G2");
    assert_eq!(&rows(&e.doc)[3..], [" G2", "  b", "  low"]);
    step(&mut e, "low", true).expect("past b");
    assert_eq!(&rows(&e.doc)[3..], [" G2", "  low", "  b"]);
    step(&mut e, "low", true).expect("out of G2");
    assert_eq!(&rows(&e.doc)[2..], [" a", " low", " G2", "  b"]);
    assert_eq!(e.doc.layers[e.layer].name, "low");
    step(&mut e, "low", true).expect("past a");
    step(&mut e, "low", true).expect("out of G1");
    assert_eq!(&rows(&e.doc)[..3], ["top", "low", "G1"]);
    step(&mut e, "low", true).expect("past top");
    assert_eq!(
        step(&mut e, "low", true),
        Err(LimitError::NoSuchItem),
        "nothing above"
    );
    for _ in 0..7 {
        e.apply(StepCmd::Undo).expect("undo");
    }
    assert_eq!(rows(&e.doc), NESTED);
}

#[test]
fn stepping_down_mirrors_stepping_up() {
    let mut e = editor(&NESTED, "top");
    step(&mut e, "top", false).expect("into G1 at its top");
    assert_eq!(&rows(&e.doc)[..3], ["G1", " top", " a"]);
    step(&mut e, "top", false).expect("past a");
    step(&mut e, "top", false).expect("into G2");
    assert_eq!(&rows(&e.doc)[2..5], [" G2", "  top", "  b"]);
    step(&mut e, "top", false).expect("past b");
    step(&mut e, "top", false).expect("out of G2");
    step(&mut e, "top", false).expect("out of G1");
    assert_eq!(&rows(&e.doc)[3..], ["  b", "top", "low"]);
    step(&mut e, "top", false).expect("past low");
    assert_eq!(step(&mut e, "top", false), Err(LimitError::NoSuchItem));
}

#[test]
fn a_folded_group_is_stepped_over_whole_and_a_group_steps_with_its_rows() {
    let mut e = editor(&NESTED, "low");
    let layer = id(&e, "G1");
    e.apply(LayerCmd::SetFolded {
        layer,
        folded: true,
    })
    .expect("folded");
    step(&mut e, "low", true).expect("over G1");
    assert_eq!(rows(&e.doc), ["top", "low", "G1", " a", " G2", "  b"]);
    step(&mut e, "G1", true).expect("G1 up with its rows");
    assert_eq!(rows(&e.doc), ["top", "G1", " a", " G2", "  b", "low"]);
    step(&mut e, "G2", true).expect("G2 past a");
    assert_eq!(&rows(&e.doc)[2..5], [" G2", "  b", " a"]);
}

#[test]
fn stepping_into_a_group_past_the_depth_limit_is_refused() {
    let rows_at = ["G1", " G2", "  G3", "   G4", "    x", "   Gy", "    z"];
    let mut e = editor(&rows_at, "Gy");
    assert_eq!(step(&mut e, "Gy", true), Err(LimitError::TooDeep));
    assert_eq!(rows(&e.doc), rows_at);
    let mut e = editor(&["G", "a", "b"], "b");
    step(&mut e, "a", true).expect("into G");
    assert_eq!(rows(&e.doc), ["G", " a", "b"]);
}

#[test]
fn a_plain_move_that_splits_a_group_is_refused() {
    let mut e = editor(&NESTED, "G2");
    let layer = id(&e, "G2");
    let to = index_of(&e.doc, "top");
    assert_eq!(
        e.apply(LayerCmd::Move { layer, to }).map(|_| ()),
        Err(LimitError::GroupOrder)
    );
    assert_eq!(rows(&e.doc), NESTED);
}

#[test]
fn merging_is_refused_on_a_group_and_across_groups() {
    let mut e = editor(&NESTED, "a");
    let layer = id(&e, "a");
    assert_eq!(
        e.apply(LayerCmd::MergeDown { layer }).map(|_| ()),
        Err(LimitError::Group),
        "G2 is below a"
    );
    let layer = id(&e, "G1");
    assert_eq!(
        e.apply(LayerCmd::MergeDown { layer }).map(|_| ()),
        Err(LimitError::Group)
    );
    let mut e = editor(&["G", " a", "b"], "a");
    let layer = id(&e, "a");
    assert_eq!(
        e.apply(LayerCmd::MergeDown { layer }).map(|_| ()),
        Err(LimitError::MergeAcrossGroups)
    );
    let mut e = editor(&["G", " a", " b"], "a");
    let layer = id(&e, "a");
    e.apply(LayerCmd::MergeDown { layer })
        .expect("siblings merge");
    assert_eq!(rows(&e.doc), ["G", " b"]);
}

#[test]
fn a_group_is_not_drawn_on_and_a_locked_group_locks_its_rows() {
    let mut e = editor(&NESTED, "G1");
    let refused = e.apply(DrawCmd::EraseCell { x: 1, y: 1 }).map(|_| ());
    assert_eq!(refused, Err(LimitError::Group));
    let layer = id(&e, "G1");
    let opacity = LayerCmd::SetOpacity { layer, opacity: 9 };
    assert_eq!(e.apply(opacity).map(|_| ()), Err(LimitError::Group));
    let blend = LayerCmd::SetBlend {
        layer,
        blend: crate::document::BlendMode::Multiply,
    };
    assert_eq!(e.apply(blend).map(|_| ()), Err(LimitError::Group));
    let g1 = index_of(&e.doc, "G1");
    e.doc.layers[g1].locked = true;
    e.layer = index_of(&e.doc, "b");
    let refused = e.apply(DrawCmd::EraseCell { x: 1, y: 1 }).map(|_| ());
    assert_eq!(refused, Err(LimitError::Locked));
    e.layer = index_of(&e.doc, "low");
    e.apply(DrawCmd::EraseCell { x: 1, y: 1 })
        .expect("outside the group");
}

#[test]
fn a_layer_added_inside_a_group_joins_it() {
    let mut e = editor(&NESTED, "a");
    let at = index_of(&e.doc, "a") + 1;
    e.apply(LayerCmd::Add {
        at,
        mode: ColorMode::Rgba,
    })
    .expect("added");
    assert_eq!(rows(&e.doc)[2], " Layer 7");
    e.doc.check_groups().expect("in order");
}

#[test]
fn folding_is_never_a_step_and_only_a_group_folds() {
    let mut e = editor(&NESTED, "a");
    let depth = e.history.undo_depth();
    let layer = id(&e, "G2");
    e.apply(LayerCmd::SetFolded {
        layer,
        folded: true,
    })
    .expect("folded");
    assert!(e.doc.layers[index_of(&e.doc, "G2")].folded);
    assert_eq!(e.history.undo_depth(), depth);
    let layer = id(&e, "a");
    let outcome = e
        .apply(LayerCmd::SetFolded {
            layer,
            folded: true,
        })
        .expect("nothing");
    assert_eq!(outcome, crate::command::Outcome::UNCHANGED);
}

#[test]
fn the_list_leaves_out_what_folded_groups_hold() {
    let mut doc = layout(&NESTED);
    let all: Vec<usize> = (0..doc.layers.len()).rev().collect();
    assert_eq!(doc.listed_layers(), all);
    let g2 = index_of(&doc, "G2");
    doc.layers[g2].folded = true;
    let names = |d: &Document| -> Vec<String> {
        let rows = d.listed_layers().into_iter();
        rows.map(|i| d.layers[i].name.clone()).collect()
    };
    assert_eq!(names(&doc), ["top", "G1", "a", "G2", "low"]);
    let g1 = index_of(&doc, "G1");
    doc.layers[g1].folded = true;
    assert_eq!(names(&doc), ["top", "G1", "low"]);
}

#[test]
fn only_a_group_lets_the_layer_below_go_one_deeper() {
    let mut doc = layout(&["top", "low"]);
    doc.layers[0].depth = 1;
    assert_eq!(doc.check_groups(), Err(LimitError::GroupOrder));
    doc.layers[1].kind = LayerKind::Group;
    assert_eq!(doc.check_groups(), Ok(()));
}

#[test]
fn a_layer_added_at_the_bottom_joins_the_group_that_ends_there() {
    let mut doc = layout(&["G1", " a"]);
    doc.add_layer(0, ColorMode::Rgba).expect("layer");
    assert_eq!(doc.layers[0].depth, 1);
    assert_eq!(doc.check_groups(), Ok(()));
}

#[test]
fn a_group_is_never_part_of_the_picture_or_drawn_on_screen() {
    let doc = layout(&["G1", " a"]);
    assert_eq!(doc.picture_layers(), [true, false]);
    assert_eq!(doc.screen_layers(), [true, false]);
}

/// `rows` with one pixel drawn at (1, 1) on the layer `on`, which is made active, and that
/// pixel lifted and moved one to the right.
fn floating_in(rows: &[&str], on: &str) -> Editor {
    use crate::command::{FloatingCmd, SelectionCmd};
    let mut e = editor(rows, on);
    let mut cells = vec![0; 64];
    cells[9] = crate::color::rgba(200, 10, 10, 255);
    let grid = crate::grid::Grid::from_cells(8, 8, &cells);
    e.doc
        .put_cel(e.layer, 0, crate::layer::Cel::new(grid))
        .expect("drawn");
    let shape = Box::new(crate::mask::Mask::rect(0, 0, 4, 4));
    let op = crate::selection::SelectOp::Replace;
    e.apply(SelectionCmd::Select { shape, op }).expect("select");
    e.apply(FloatingCmd::Lift { copy: false }).expect("lift");
    let (dx, dy, join) = (1, 0, false);
    e.apply(FloatingCmd::Move { dx, dy, join }).expect("float");
    e
}

#[test]
fn locking_a_group_puts_down_what_floats_on_a_layer_in_it() {
    let mut e = floating_in(&["G1", " a"], "a");
    assert!(e.is_floating());
    let layer = id(&e, "G1");
    e.apply(LayerCmd::SetLocked {
        layer,
        locked: true,
    })
    .expect("locked");
    assert!(!e.is_floating(), "put down before the lock");
    let a = index_of(&e.doc, "a");
    assert_ne!(e.doc.layers[a].cel(0).get(2, 1), 0, "where it was moved to");
    assert_eq!(e.paint_refusal(a), Some(LimitError::Locked));
}

#[test]
fn floating_pixels_from_a_file_are_refused_on_a_layer_that_cannot_be_drawn_on() {
    let e = floating_in(&["G1", " a"], "a");
    let snapshot = e.floating_snapshot().expect("floating");
    let mut doc = e.doc.clone();
    let g1 = index_of(&doc, "G1");
    doc.layers[g1].locked = true;
    let mut locked = Editor::new(doc);
    assert_eq!(
        locked.restore_floating(snapshot.clone()),
        Err(LimitError::Locked)
    );
    let mut on_group = snapshot;
    on_group.layer = g1;
    let mut open = Editor::new(e.doc.clone());
    assert_eq!(open.restore_floating(on_group), Err(LimitError::Group));
}

#[test]
fn a_single_row_is_not_moved_while_the_list_has_groups() {
    let mut e = editor(&["G2", "G1", " a"], "a");
    let layer = id(&e, "G1");
    let before = rows(&e.doc);
    assert_eq!(
        e.apply(LayerCmd::Move { layer, to: 2 }),
        Err(LimitError::GroupOrder)
    );
    assert_eq!(rows(&e.doc), before, "a would have gone into G2");
}

#[test]
fn a_layer_added_past_the_top_is_in_no_group() {
    let mut doc = layout(&["G1", " a"]);
    doc.add_layer(99, ColorMode::Rgba).expect("layer");
    assert_eq!(doc.layers.last().map(|l| l.depth), Some(0));
    assert_eq!(doc.check_groups(), Ok(()));
}

#[test]
fn a_drag_naming_rows_past_the_list_is_refused_without_walking_them() {
    use crate::cel_drag::{CelDrag, DragKind, Landing};
    let mut e = editor(&["top", "low"], "low");
    let drag = CelDrag {
        layers: (0, usize::MAX - 1),
        frames: (0, 0),
        to_layer: usize::MAX - 2,
        to_frame: 0,
        kind: DragKind::Move,
        landing: Landing::Overwrite,
    };
    let before = rows(&e.doc);
    assert!(e.apply(crate::command::CelCmd::Drag(drag)).is_err());
    assert_eq!(rows(&e.doc), before);
}

#[test]
fn a_drag_landing_far_past_the_list_is_refused() {
    use crate::cel_drag::{CelDrag, DragKind, Landing};
    let mut e = editor(&["top", "low"], "low");
    for (to_layer, to_frame) in [(usize::MAX, 0), (0, usize::MAX)] {
        let drag = CelDrag {
            layers: (0, 0),
            frames: (0, 0),
            to_layer,
            to_frame,
            kind: DragKind::Copy,
            landing: Landing::Overwrite,
        };
        let refused = e.apply(crate::command::CelCmd::Drag(drag));
        assert_eq!(refused, Err(LimitError::NoSuchItem));
    }
}

/// Every way of setting each row's lock and eye on a nested list.
fn every_setting<'a>(rows: &'a [&'a str]) -> impl Iterator<Item = Document> + 'a {
    let n = rows.len();
    (0..1u32 << (2 * n)).map(move |bits| {
        let mut doc = layout(rows);
        for (i, layer) in doc.layers.iter_mut().enumerate() {
            layer.locked = bits >> i & 1 == 1;
            layer.visible = bits >> (n + i) & 1 == 1;
        }
        doc
    })
}

#[test]
fn a_lock_asked_of_one_layer_is_the_one_the_whole_list_gives() {
    for doc in every_setting(&["top", "G1", " a", " G2", "  G3", "   b", "  c", "low"]) {
        let all = doc.locked_layers();
        for (index, &locked) in all.iter().enumerate() {
            assert_eq!(doc.locked_through(index), locked, "{index}");
        }
        assert!(!doc.locked_through(all.len()), "no such layer");
    }
}

#[test]
fn the_layers_the_screen_draws_are_written_into_a_buffer_kept_between_frames() {
    let mut kept = vec![true; 40];
    for doc in every_setting(&NESTED) {
        doc.screen_layers_into(&mut kept);
        assert_eq!(kept, doc.screen_layers());
    }
    assert!(kept.capacity() >= 40, "the buffer is reused, not replaced");
}
