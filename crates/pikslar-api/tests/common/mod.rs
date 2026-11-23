//! What the contract tests share: a project to script, and saving and opening it.
#![allow(dead_code)]

use pikslar_api::{run_ops, Op, Reply, API_VERSION};
use pikslar_core::{
    rgba, ColorMode, Document, DrawCmd, Editor, FrameCmd, LayerCmd, PaletteCmd, StepCmd, Tool,
};
use pikslar_format::piks;

pub const RED: u32 = rgba(220, 20, 20, 255);
pub const BLUE: u32 = rgba(20, 20, 220, 255);

/// Two layers, RGBA below and palette above, two frames, a swatch and a dot.
pub fn project() -> Editor {
    let mut e = Editor::new(Document::new(16, 12, ColorMode::Rgba).expect("a small canvas"));
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Palette,
    })
    .expect("room");
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    e.apply(PaletteCmd::AddSwatch { color: BLUE })
        .expect("room");
    (e.layer, e.frame, e.color) = (0, 0, RED);
    e.apply(DrawCmd::Stroke {
        tool: Tool::Pencil,
        path: vec![(2, 3)],
    })
    .expect("a dot");
    e.apply(StepCmd::EndDrag).expect("ends");
    e
}

pub fn save(e: &Editor) -> Vec<u8> {
    piks::try_save(e.doc()).expect("saves")
}

/// What saving writes but for the section of frame ids and id floors, which keeps the ids an
/// undone or failed change used up.
pub fn save_but_ids(e: &Editor) -> Vec<u8> {
    piks::save_without_ids(e.doc())
}

pub fn reopen(bytes: &[u8]) -> Editor {
    Editor::new(piks::load(bytes).expect("opens"))
}

/// The persisted ids of the layers, from the bottom.
pub fn layer_ids(e: &Editor) -> Vec<i64> {
    e.doc()
        .layers()
        .iter()
        .map(|l| i64::from(l.persisted_id().get()))
        .collect()
}

/// The handles of the frames, in order.
pub fn frames(e: &Editor) -> Vec<i64> {
    e.doc()
        .frames
        .iter()
        .map(|f| i64::try_from(f.key().get()).expect("a handle in range"))
        .collect()
}

/// Runs operations as one script, which must succeed.
pub fn ops(e: &mut Editor, list: &[Op]) -> Vec<Reply> {
    run_ops(e, API_VERSION, list).expect("the script runs")
}

/// Runs one operation as a script of its own.
pub fn op(e: &mut Editor, one: Op) -> Reply {
    ops(e, &[one]).remove(0)
}

pub fn created(reply: &Reply) -> i64 {
    match reply {
        Reply::Created(id) => i64::from(*id),
        other => panic!("expected an id, got {other:?}"),
    }
}

pub fn made_frame(reply: &Reply) -> i64 {
    match reply {
        Reply::Frame(handle) => i64::try_from(*handle).expect("a handle in range"),
        other => panic!("expected a frame handle, got {other:?}"),
    }
}

pub fn undo(e: &mut Editor) {
    e.apply(StepCmd::Undo).expect("undoes");
}

pub fn redo(e: &mut Editor) {
    e.apply(StepCmd::Redo).expect("redoes");
}
