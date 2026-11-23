//! The cel matrix through the window's own entry points.

use crate::app::App;
use crate::layout::{matrix_row, Layout, MATRIX_ROWS};
use crate::test_app::{app, click, dot, ALT, PLAIN, SHIFT, WINDOW};
use pikslar_core::{FrameCmd, StepCmd};
use pikslar_platform::{Event, Handler, Window};
use pikslar_render::Surface;

fn frame(a: &mut App) {
    let mut px = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
    let mut surface = Surface {
        px: &mut px,
        w: WINDOW.0,
        h: WINDOW.1,
    };
    a.frame(&mut surface, Window::detached(), &mut Vec::new());
}

/// An app with `layers` layers and `frames` frames, drawn once.
fn sprite(layers: usize, frames: usize) -> App {
    let mut a = app(frames);
    for _ in 1..layers {
        a.layer_action(0);
    }
    a.ui.record_names();
    a.ui_dirty = true;
    frame(&mut a);
    a
}

fn open(a: &mut App) {
    a.toggle_matrix();
    frame(a);
    frame(a);
}

/// The middle of the cell for matrix row `row` (0 is the top shown) and strip column `col`.
fn cell(a: &App, row: i32, col: i32) -> (i32, i32) {
    let x = a.timeline.strip.x + col * 34 + 10;
    (x, a.layout.matrix.y + row * matrix_row() + matrix_row() / 2)
}

fn names(a: &App) -> String {
    a.ui.layout().expect("names are kept")
}

#[test]
fn the_matrix_starts_folded_and_opening_it_gives_each_layer_a_row() {
    let mut a = sprite(3, 2);
    assert_eq!(a.layout, Layout::compute(WINDOW.0, WINDOW.1, 1));
    assert!(!names(&a).contains("Cel "));
    let folded_canvas = a.layout.canvas;
    open(&mut a);
    assert_eq!(a.layout.matrix_rows(1), 3);
    assert_eq!(a.layout.canvas.h, folded_canvas.h - 3 * matrix_row() - 4);
    for (layer, frame) in [(1, 1), (3, 1), (3, 2)] {
        assert!(
            names(&a).contains(&format!("\tCel {layer} {frame}\t")),
            "{layer} {frame}"
        );
    }
    a.toggle_matrix();
    frame(&mut a);
    assert_eq!(
        a.layout.canvas, folded_canvas,
        "folding gives the canvas back"
    );
}

#[test]
fn opening_and_folding_is_not_an_edit() {
    let mut a = sprite(2, 2);
    let depth = a.editor.history().undo_depth();
    let revision = a.editor.revision();
    open(&mut a);
    a.toggle_matrix();
    frame(&mut a);
    assert_eq!(a.editor.history().undo_depth(), depth);
    assert_eq!(a.editor.revision(), revision, "the document did not change");
}

#[test]
fn a_click_on_a_cel_makes_its_layer_and_frame_current() {
    let mut a = sprite(3, 3);
    open(&mut a);
    assert_eq!((a.editor.layer, a.editor.frame), (2, 0));
    // The bottom row is the bottom layer.
    let at = cell(&a, 2, 1);
    click(&mut a, at, PLAIN);
    assert_eq!((a.editor.layer, a.editor.frame), (0, 1));
    let at = cell(&a, 0, 2);
    click(&mut a, at, PLAIN);
    assert_eq!((a.editor.layer, a.editor.frame), (2, 2));
    let past = cell(&a, 0, 5);
    click(&mut a, past, PLAIN);
    assert_eq!(
        (a.editor.layer, a.editor.frame),
        (2, 2),
        "past the last frame is no cel"
    );
}

#[test]
fn a_new_layer_gets_a_row_while_the_matrix_is_open() {
    let mut a = sprite(2, 2);
    open(&mut a);
    assert_eq!(a.layout.matrix_rows(1), 2);
    a.layer_action(0);
    frame(&mut a);
    assert_eq!(a.layout.matrix_rows(1), 3);
}

#[test]
fn many_layers_scroll_and_the_active_one_stays_in_view() {
    let mut a = sprite(20, 2);
    open(&mut a);
    assert_eq!(a.layout.matrix_rows(1), MATRIX_ROWS);
    assert!(
        names(&a).contains("\tCel 20 1\t"),
        "the active top layer is shown"
    );
    assert!(!names(&a).contains("\tCel 1 1\t"));
    let m = a.layout.matrix;
    let wheel = Event::Wheel {
        x: m.x + m.w / 2,
        y: m.y + 2,
        ticks: -30,
    };
    a.event(wheel, Window::detached());
    frame(&mut a);
    assert!(
        names(&a).contains("\tCel 1 1\t"),
        "the wheel reaches the bottom layer"
    );
    assert_eq!(a.editor.layer, 19, "scrolling does not change the layer");
}

#[test]
fn a_cel_with_pixels_is_marked_and_an_empty_one_is_not() {
    let mut a = sprite(2, 2);
    dot(&mut a, 1, 1);
    open(&mut a);
    let mut px = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
    let mut surface = Surface {
        px: &mut px,
        w: WINDOW.0,
        h: WINDOW.1,
    };
    a.ui_dirty = true;
    a.frame(&mut surface, Window::detached(), &mut Vec::new());
    // A point inside a drawing's 6-pixel mark but inside the hole of an empty cel's ring.
    let near_middle = |row: i32, col: i32| {
        let x = a.timeline.strip.x + col * 34 + 16 + 1;
        let y = a.layout.matrix.y + row * matrix_row() + (matrix_row() - 2) / 2 + 1;
        px[(y * WINDOW.0 + x) as usize]
    };
    use pikslar_ui::theme;
    assert_eq!(
        near_middle(0, 0),
        theme::on_accent(),
        "the current cel, drawn on"
    );
    assert_eq!(near_middle(0, 1), theme::well(), "an empty cel");
    assert_eq!(
        near_middle(1, 0),
        theme::raised(),
        "the current frame on another layer, empty"
    );
}

/// Presses on one cel, moves over another and lets go, as a hand would. Cels are given as
/// matrix row and strip column.
fn drag(a: &mut App, from: (i32, i32), to: (i32, i32), mods: pikslar_platform::Mods) {
    let (from, to) = (cell(a, from.0, from.1), cell(a, to.0, to.1));
    use pikslar_platform::Button;
    a.pointer_down(from.0, from.1, Button::Left, mods);
    frame(a);
    a.event(Event::Move { x: to.0, y: to.1 }, Window::detached());
    frame(a);
    a.pointer_up(to.0, to.1, Button::Left);
    frame(a);
    frame(a);
}

/// The drawing each frame of a layer shows, named by first appearance; 0 for none.
fn drawings(a: &App, layer: usize) -> Vec<usize> {
    let mut seen: Vec<usize> = Vec::new();
    let cels = a.editor.doc().layers()[layer].cels();
    cels.iter()
        .map(|cel| {
            if cel.is_blank() {
                return 0;
            }
            let id = cel.image_id();
            let at = seen.iter().position(|&s| s == id).unwrap_or_else(|| {
                seen.push(id);
                seen.len() - 1
            });
            at + 1
        })
        .collect()
}

/// A sprite of `layers` layers and `frames` frames, every cel drawn on, with the matrix open.
fn drawn(layers: usize, frames: usize) -> App {
    let mut a = sprite(layers, frames);
    for layer in 0..layers {
        for f in 0..frames {
            a.editor.layer = layer;
            a.editor.frame = f;
            dot(&mut a, f as i32, layer as i32);
        }
    }
    a.editor.layer = layers - 1;
    a.editor.frame = 0;
    open(&mut a);
    a
}

#[test]
fn dragging_a_cel_moves_it_as_one_step_that_undoes() {
    let mut a = drawn(1, 3);
    assert_eq!(drawings(&a, 0), [1, 2, 3]);
    let ids = |a: &App| -> Vec<usize> {
        let cels = a.editor.doc().layers()[0].cels();
        cels.iter().map(|c| c.image_id()).collect()
    };
    let before = ids(&a);
    let depth = a.editor.history().undo_depth();
    drag(&mut a, (0, 0), (0, 2), PLAIN);
    let after = ids(&a);
    assert!(
        a.editor.doc().layers()[0].cel(0).is_blank(),
        "frame 1 left empty"
    );
    assert_eq!(
        after[1..],
        [before[1], before[0]],
        "frame 1's drawing over frame 3"
    );
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
    assert_eq!(a.editor.frame, 2, "the landing cel is current");
    a.editor.apply(StepCmd::Undo).expect("undo");
    assert_eq!(drawings(&a, 0), [1, 2, 3]);
}

#[test]
fn shift_click_picks_a_block_and_ctrl_drag_copies_it() {
    let mut a = drawn(2, 4);
    // The current cel is the top layer's first; Shift-click the bottom layer's second.
    let at = cell(&a, 1, 1);
    click(&mut a, at, SHIFT);
    let picked = a.matrix.selection.expect("a block");
    assert_eq!((picked.layers, picked.frames), ((0, 1), (0, 1)));
    let ctrl = pikslar_platform::Mods {
        ctrl: true,
        ..PLAIN
    };
    drag(&mut a, (0, 0), (0, 2), ctrl);
    for layer in 0..2 {
        let d = drawings(&a, layer);
        assert_eq!(
            d,
            [1, 2, 1, 2],
            "layer {layer}: both frames copied, sources kept"
        );
    }
    let landed = a.matrix.selection.expect("still picked");
    assert_eq!(landed.frames, (2, 3), "the landed cels stay picked");
}

#[test]
fn alt_drag_links_and_shift_drag_inserts() {
    let mut a = drawn(1, 2);
    drag(&mut a, (0, 0), (0, 1), ALT);
    let layer = &a.editor.doc().layers()[0];
    assert!(
        layer.link(0) != 0 && layer.link(0) == layer.link(1),
        "one hold"
    );
    let mut a = drawn(1, 2);
    drag(&mut a, (0, 1), (0, 0), SHIFT);
    assert_eq!(a.editor.doc().frames.len(), 3, "a new frame");
    assert_eq!(
        drawings(&a, 0),
        [1, 2, 0],
        "landed before frame 1, its place left empty"
    );
}

#[test]
fn alt_drag_to_another_layer_links_the_cels_across_layers() {
    let mut a = drawn(2, 2);
    drag(&mut a, (0, 0), (1, 1), ALT);
    let doc = a.editor.doc();
    let (top, bottom) = (&doc.layers()[1], &doc.layers()[0]);
    let pair = [(top.link(0), top.cel(0)), (bottom.link(1), bottom.cel(1))];
    let other = [(bottom.link(0), bottom.cel(0)), (top.link(1), top.cel(1))];
    let linked =
        |[(a, x), (b, y)]: [(u32, &pikslar_core::Cel); 2]| a != 0 && a == b && x.same_image(y);
    assert!(
        linked(pair) || linked(other),
        "one link across the two layers"
    );
    assert_eq!(doc.check_links(), Ok(()));
}

#[test]
fn a_notch_marks_each_cel_sharing_a_drawing_with_another_layer() {
    let mut a = drawn(2, 2);
    let notches = |a: &App| names(a).matches("\tShared ").count();
    assert_eq!(notches(&a), 0);
    drag(&mut a, (0, 0), (1, 1), ALT);
    frame(&mut a);
    assert_eq!(notches(&a), 2, "both cels of the link");
}

#[test]
fn converting_a_linked_layer_says_its_links_ended() {
    use pikslar_core::ColorMode;
    let mut a = drawn(2, 2);
    drag(&mut a, (0, 0), (1, 1), ALT);
    let before = a.editor.doc().layers()[1].mode;
    let mode = match before {
        ColorMode::Rgba => ColorMode::Palette,
        ColorMode::Palette => ColorMode::Rgba,
    };
    a.set_layer_mode(1, mode);
    let said = a.message.as_ref().map(|(text, _)| text.as_str());
    assert_eq!(said, Some(crate::panel_right::LINKS_ENDED));
    let doc = a.editor.doc();
    assert!((0..2).all(|f| !doc.links_across(0, f) && !doc.links_across(1, f)));
    a.message = None;
    a.set_layer_mode(1, before);
    assert_eq!(a.message, None, "no links to end");
}

#[test]
fn a_drop_that_cannot_land_says_why_and_changes_nothing() {
    let mut a = drawn(2, 2);
    use pikslar_core::ColorMode;
    let layer = a.editor.layer_id(1).expect("layer");
    let mode = match a.editor.doc().layers()[0].mode {
        ColorMode::Rgba => ColorMode::Palette,
        ColorMode::Palette => ColorMode::Rgba,
    };
    a.editor
        .apply(pikslar_core::LayerCmd::SetMode { layer, mode })
        .expect("converted");
    let depth = a.editor.history().undo_depth();
    drag(&mut a, (0, 0), (1, 1), ALT);
    assert_eq!(a.editor.history().undo_depth(), depth);
    let said = a.message.as_ref().map(|(text, _)| text.clone());
    assert!(
        said.is_some_and(|t| t.contains("same color mode")),
        "{:?}",
        a.message
    );
}

#[test]
fn an_edit_unpicks_cels_that_may_have_moved() {
    let mut a = drawn(1, 3);
    drag(&mut a, (0, 0), (0, 1), PLAIN);
    assert!(a.matrix.selection.is_some(), "the landed cel is picked");
    a.editor.apply(StepCmd::Undo).expect("undo");
    frame(&mut a);
    assert_eq!(a.matrix.selection, None, "undo put other cels there");
}

#[test]
fn another_document_unpicks_the_cels() {
    let mut a = sprite(1, 3);
    open(&mut a);
    let at = cell(&a, 0, 1);
    click(&mut a, at, SHIFT);
    assert!(a.matrix.selection.is_some(), "a block is picked");
    let rev = a.editor.revision();
    let doc = a.editor.doc().clone();
    a.replace_document(doc, None);
    // The new document's own edits bring its count to the old one's before the next pass.
    for ms in 11.. {
        if a.editor.revision() >= rev {
            break;
        }
        let retime = FrameCmd::Retime {
            from: 0,
            durations: vec![ms],
        };
        a.editor.apply(retime).expect("retime");
    }
    assert_eq!(a.editor.revision(), rev, "the count matches by chance");
    frame(&mut a);
    assert_eq!(
        a.matrix.selection, None,
        "the other document's cels are not picked"
    );
}

#[test]
fn a_press_the_matrix_lost_never_lands_later() {
    use pikslar_platform::Button;
    let mut a = drawn(1, 3);
    let (from, to) = (cell(&a, 0, 0), cell(&a, 0, 2));
    let before = a.editor.history().undo_depth();
    a.pointer_down(from.0, from.1, Button::Left, PLAIN);
    frame(&mut a);
    a.event(Event::Move { x: to.0, y: to.1 }, Window::detached());
    frame(&mut a);
    // The matrix folds under the held button, and the button comes up while it is folded.
    a.toggle_matrix();
    frame(&mut a);
    a.pointer_up(to.0, to.1, Button::Left);
    frame(&mut a);
    open(&mut a);
    // A later click on another control must not finish the old drag.
    let strip = a.timeline.strip;
    click(&mut a, (strip.x + 34 + 10, strip.y + strip.h / 2), PLAIN);
    assert_eq!(a.editor.history().undo_depth(), before);
    assert!(a.matrix.press.is_none());
}
