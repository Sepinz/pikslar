//! Floating pixels from a damaged recovery copy are refused, however odd their numbers.

use pikslar_core::{rgba, ColorMode, Document, Editor, FloatingSnapshot, Mask, SelectOp, Tool};
use pikslar_core::{DrawCmd, FloatingCmd, SelectionCmd};

fn lifted() -> (Editor, FloatingSnapshot) {
    let mut editor = Editor::new(Document::new(32, 32, ColorMode::Rgba).expect("valid"));
    editor.color = rgba(10, 20, 30, 255);
    editor
        .apply(DrawCmd::Begin {
            tool: Tool::Line,
            x: 0,
            y: 0,
        })
        .expect("begin");
    editor.apply(DrawCmd::To { x: 31, y: 31 }).expect("drag");
    editor.apply(DrawCmd::Commit).expect("commit");
    editor
        .apply(SelectionCmd::Select {
            shape: Box::new(Mask::rect(2, 2, 8, 8)),
            op: SelectOp::Replace,
        })
        .expect("select");
    editor
        .apply(FloatingCmd::Lift { copy: false })
        .expect("lift");
    editor
        .apply(FloatingCmd::Move {
            dx: 1,
            dy: 1,
            join: true,
        })
        .expect("move");
    let snapshot = editor.floating_snapshot().expect("floating");
    editor.apply(FloatingCmd::Revert).expect("revert");
    (editor, snapshot)
}

#[test]
fn the_most_negative_position_is_refused() {
    for odd in [i32::MIN, i32::MIN + 1, i32::MAX] {
        let (mut editor, snapshot) = lifted();
        let bad = FloatingSnapshot {
            x: odd,
            ..snapshot.clone()
        };
        assert!(editor.restore_floating(bad).is_err(), "x = {odd}");
        let bad = FloatingSnapshot { y: odd, ..snapshot };
        assert!(editor.restore_floating(bad).is_err(), "y = {odd}");
    }
}

#[test]
fn an_ordinary_position_is_accepted() {
    let (mut editor, snapshot) = lifted();
    assert!(editor.restore_floating(snapshot).is_ok());
}
