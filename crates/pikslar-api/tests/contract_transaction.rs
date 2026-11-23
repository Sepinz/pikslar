//! Contract: a run is one undo step or nothing, ids last across a file, and an API-driven edit
//! saves as the same commands do.

mod common;

use common::*;
use pikslar_api::{run, run_cancellable, run_ops, ApiError, Mode, Op, Reply, API_VERSION};
use pikslar_core::{
    BlendMode, ColorMode, DrawCmd, Editor, FrameCmd, LayerCmd, LimitError, PaletteCmd, StepCmd,
    TagCmd, Tool,
};
use std::sync::atomic::{AtomicBool, Ordering};

/// A script touching every kind of operation, on the project's bottom layer.
fn script(e: &Editor) -> Vec<Op> {
    let (bottom, swatch, f) = (layer_ids(e)[0], swatch(e), frames(e));
    vec![
        Op::AddLayer {
            above: None,
            mode: Mode::Rgba,
            name: Some("Ink".into()),
        },
        Op::SetPixel {
            layer: bottom,
            frame: f[1],
            x: 7,
            y: 2,
            color: i64::from(BLUE),
        },
        Op::DrawLine {
            layer: bottom,
            frame: f[0],
            from: (0, 11),
            to: (15, 0),
            color: i64::from(RED),
        },
        Op::Fill {
            layer: bottom,
            frame: f[0],
            x: 0,
            y: 0,
            color: i64::from(BLUE),
        },
        Op::SetOpacity {
            layer: bottom,
            opacity: 77,
        },
        Op::SetBlend {
            layer: bottom,
            blend: "Screen".into(),
        },
        Op::SetDuration {
            frame: f[1],
            ms: 40,
        },
        Op::AddFrame { at: 2, copy: true },
        Op::AddTag {
            name: "loop".into(),
            from: f[0],
            to: f[1],
        },
        Op::Recolor {
            swatch,
            color: i64::from(RED),
        },
        Op::AddSwatch { color: 0x7f10_2030 },
        Op::MoveLayer {
            layer: bottom,
            to: 1,
        },
    ]
}

fn swatch(e: &Editor) -> i64 {
    i64::from(e.doc().colors.swatches()[0])
}

/// The same edit as [`script`], by commands, as the interface would give them.
fn by_commands(e: &mut Editor) -> Result<(), LimitError> {
    let bottom = e.layer_id(0).ok_or(LimitError::NoSuchItem)?;
    let swatch = e.doc().colors.swatches()[0];
    e.apply(LayerCmd::Add {
        at: 2,
        mode: ColorMode::Rgba,
    })?;
    let ink = e.layer_id(2).ok_or(LimitError::NoSuchItem)?;
    e.apply(LayerCmd::Rename {
        layer: ink,
        name: "Ink".into(),
    })?;
    (e.brush_size, e.pixel_perfect, e.rhythm) = (1, false, false);
    (e.layer, e.frame, e.color) = (0, 1, BLUE);
    e.apply(DrawCmd::Stroke {
        tool: Tool::Pencil,
        path: vec![(7, 2)],
    })?;
    (e.frame, e.color) = (0, RED);
    e.apply(DrawCmd::Stroke {
        tool: Tool::Line,
        path: vec![(0, 11), (15, 0)],
    })?;
    e.color = BLUE;
    e.apply(DrawCmd::Begin {
        tool: Tool::Fill,
        x: 0,
        y: 0,
    })?;
    e.apply(LayerCmd::SetOpacity {
        layer: bottom,
        opacity: 77,
    })?;
    e.apply(StepCmd::EndDrag)?;
    e.apply(LayerCmd::SetBlend {
        layer: bottom,
        blend: BlendMode::Screen,
    })?;
    e.apply(FrameCmd::Retime {
        from: 1,
        durations: vec![40],
    })?;
    e.apply(StepCmd::EndDrag)?;
    e.apply(FrameCmd::Add { at: 2, copy: true })?;
    e.apply(TagCmd::Add {
        name: "loop".into(),
        from: 0,
        to: 1,
    })?;
    e.apply(StepCmd::EndDrag)?;
    e.apply(PaletteCmd::Recolor {
        id: swatch,
        color: RED,
    })?;
    e.apply(StepCmd::EndDrag)?;
    e.apply(PaletteCmd::AddSwatch { color: 0x7f10_2030 })?;
    e.apply(LayerCmd::Move {
        layer: bottom,
        to: 1,
    })?;
    Ok(())
}

#[test]
fn a_saved_api_edit_matches_the_same_commands_byte_for_byte() {
    let mut scripted = project();
    let list = script(&scripted);
    ops(&mut scripted, &list);
    let mut commanded = project();
    by_commands(&mut commanded).expect("the commands run");
    assert_eq!(save(&scripted), save(&commanded));
    assert_ne!(
        save(&scripted),
        save(&project()),
        "the script changed something"
    );
}

#[test]
fn a_run_is_one_undo_step() {
    let mut e = project();
    let (before, depth) = (save(&e), e.history().undo_depth());
    let list = script(&e);
    ops(&mut e, &list);
    let after = save(&e);
    assert_eq!(e.history().undo_depth(), depth + 1);
    undo(&mut e);
    // The layer the run added keeps its id used up, and that is saved; nothing else changes.
    assert_eq!(save_but_ids(&e), before);
    redo(&mut e);
    assert_eq!(save(&e), after);
}

#[test]
fn a_failure_part_way_leaves_the_saved_file_byte_identical() {
    let mut e = project();
    let (before, depth, revision) = (save(&e), e.history().undo_depth(), e.revision());
    let mut list = script(&e);
    list.push(Op::RemoveFrame { frame: 99 });
    list.extend(script(&e));
    let result = run_ops(&mut e, API_VERSION, &list);
    assert_eq!(result, Err(ApiError::NoSuchFrame(99)));
    assert_eq!(
        save_but_ids(&e),
        before,
        "only the ids the run used up are saved"
    );
    let used = reopen(&save(&e)).doc().layer_floor();
    assert!(used > reopen(&before).doc().layer_floor());
    assert_eq!(e.history().undo_depth(), depth);
    assert_eq!(e.revision(), revision);
}

#[test]
fn an_error_from_the_script_itself_rolls_back() {
    let mut e = project();
    let before = save(&e);
    let bottom = layer_ids(&e)[0];
    let result: Result<(), ApiError> = run(&mut e, API_VERSION, |s| {
        s.call(Op::RemoveLayer { layer: bottom })?;
        Err(ApiError::Refused(LimitError::NoSuchItem))
    });
    assert!(result.is_err());
    assert_eq!(save(&e), before);
}

#[test]
fn a_refused_command_rolls_back_with_the_editor_s_reason() {
    let mut e = project();
    let before = save(&e);
    let [bottom, top] = [layer_ids(&e)[0], layer_ids(&e)[1]];
    let result = run_ops(
        &mut e,
        API_VERSION,
        &[
            Op::RemoveLayer { layer: bottom },
            Op::RemoveLayer { layer: top },
        ],
    );
    assert_eq!(result, Err(ApiError::Refused(LimitError::LastLayer)));
    assert_eq!(save(&e), before);
}

#[test]
fn a_cancelled_run_leaves_nothing() {
    let mut e = project();
    let before = save(&e);
    let cancel = AtomicBool::new(false);
    let (bottom, first) = (layer_ids(&e)[0], frames(&e)[0]);
    let result = run_cancellable(&mut e, API_VERSION, Some(&cancel), |s| {
        for x in 0..16 {
            if x == 8 {
                cancel.store(true, Ordering::Relaxed);
            }
            s.call(Op::SetPixel {
                layer: bottom,
                frame: first,
                x,
                y: 0,
                color: i64::from(BLUE),
            })?;
        }
        Ok(())
    });
    assert_eq!(result, Err(ApiError::Cancelled));
    assert_eq!(save(&e), before);
}

#[test]
fn a_run_that_changes_nothing_adds_no_step() {
    let mut e = project();
    let (depth, revision) = (e.history().undo_depth(), e.revision());
    ops(&mut e, &[Op::Version, Op::Document, Op::Swatches]);
    assert_eq!(e.history().undo_depth(), depth);
    assert_eq!(e.revision(), revision);
}

#[test]
fn ids_last_across_save_and_reopen() {
    let mut e = project();
    let [bottom, top] = [layer_ids(&e)[0], layer_ids(&e)[1]];
    let replies = ops(
        &mut e,
        &[
            Op::AddLayer {
                above: Some(bottom),
                mode: Mode::Palette,
                name: Some("Mid".into()),
            },
            Op::AddSwatch {
                color: i64::from(RED),
            },
            Op::MoveLayer { layer: top, to: 0 },
        ],
    );
    let (mid, red) = (created(&replies[0]), created(&replies[1]));
    let ids = layer_ids(&e);
    assert_eq!(ids, vec![top, bottom, mid]);
    let mut reopened = reopen(&save(&e));
    assert_eq!(layer_ids(&reopened), ids);
    let Reply::Swatches(swatches) = op(&mut reopened, Op::Swatches) else {
        panic!("swatches")
    };
    assert!(swatches.contains(&(u32::try_from(red).expect("an id"), RED)));
    // The old ids still find the same things.
    ops(
        &mut reopened,
        &[
            Op::RenameLayer {
                layer: mid,
                name: "Again".into(),
            },
            Op::Recolor {
                swatch: red,
                color: i64::from(BLUE),
            },
        ],
    );
    assert_eq!(reopened.doc().layers()[2].name, "Again");
    let red = u32::try_from(red).expect("an id");
    assert_eq!(reopened.doc().colors.color(red), BLUE);
}

#[test]
fn ids_of_other_layers_are_kept_when_one_is_removed() {
    let mut e = project();
    let [bottom, top] = [layer_ids(&e)[0], layer_ids(&e)[1]];
    let added = created(&op(
        &mut e,
        Op::AddLayer {
            above: None,
            mode: Mode::Rgba,
            name: None,
        },
    ));
    op(&mut e, Op::RemoveLayer { layer: top });
    assert_eq!(layer_ids(&e), vec![bottom, added]);
    assert_eq!(
        run_ops(&mut e, API_VERSION, &[Op::Layer { layer: top }]),
        Err(ApiError::NoSuchLayer(top))
    );
}
