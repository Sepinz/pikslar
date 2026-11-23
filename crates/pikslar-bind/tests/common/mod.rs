//! What the binding tests share: a project, running a script on it, and the state a failed
//! run must leave exactly as it was.
#![allow(dead_code)]

use pikslar_bind::{run_script, Capabilities, Outcome, ScriptLimits};
use pikslar_core::{
    rgba, ColorMode, Document, DrawCmd, Editor, FrameCmd, LayerCmd, PaletteCmd, StepCmd, Tool,
};
use pikslar_format::piks;
use std::sync::atomic::AtomicBool;

pub const RED: u32 = rgba(220, 20, 20, 255);
pub const BLUE: u32 = rgba(20, 20, 220, 255);

/// Two layers, RGBA below and palette above, two frames, a swatch and a dot, with one step
/// undone so that redo has something to lose.
pub fn project() -> Editor {
    let mut e = Editor::new(Document::new(32, 24, ColorMode::Rgba).expect("a small canvas"));
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
    e.apply(FrameCmd::Add { at: 2, copy: false }).expect("room");
    e.apply(StepCmd::Undo).expect("undoes");
    e
}

pub fn save(e: &Editor) -> Vec<u8> {
    piks::try_save(e.doc()).expect("saves")
}

/// Everything a failed run must leave as it was.
#[derive(Debug, PartialEq, Eq)]
pub struct State {
    pub bytes: Vec<u8>,
    pub revision: u64,
    pub undo: usize,
    pub redo: usize,
    pub pushes: u64,
}

pub fn state(e: &Editor) -> State {
    State {
        bytes: save(e),
        revision: e.revision(),
        undo: e.history().undo_depth(),
        redo: e.history().redo_depth(),
        pushes: e.history().pushes(),
    }
}

/// The state but the saved ids. A failed or stopped run has used up the ids of the layers and
/// frames it made, and that is saved (the `Nids` section), so none is given again after
/// reopening; everything else must be as it was.
pub fn but_ids(e: &Editor) -> State {
    State {
        bytes: piks::save_without_ids(e.doc()),
        ..state(e)
    }
}

pub fn run_with(e: &mut Editor, source: &str, caps: Capabilities, limits: ScriptLimits) -> Outcome {
    run_script(e, source, caps, limits, &AtomicBool::new(false))
}

pub fn run(e: &mut Editor, source: &str) -> Outcome {
    run_with(e, source, Capabilities::ALL, ScriptLimits::default())
}

/// Runs a script that must succeed, giving what it returned.
pub fn ok(e: &mut Editor, source: &str) -> String {
    match run(e, source).result {
        Ok(text) => text,
        Err(f) => panic!("{f}\n{source}"),
    }
}

pub fn layer_ids(e: &Editor) -> Vec<i64> {
    e.doc()
        .layers()
        .iter()
        .map(|l| i64::from(l.persisted_id().get()))
        .collect()
}

pub fn frames(e: &Editor) -> Vec<i64> {
    e.doc()
        .frames
        .iter()
        .map(|f| i64::try_from(f.key().get()).expect("a handle"))
        .collect()
}
