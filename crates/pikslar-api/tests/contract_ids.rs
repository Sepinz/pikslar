//! Contract: a reference either names the object it was given for or is refused. No layer id
//! or frame handle ever comes to name another object. See `docs/stable-ids.md`.

mod common;

use common::*;
use pikslar_api::{run, ApiError, Mode, Op, Reply, API_VERSION};
use pikslar_core::{Editor, FrameCmd, LayerCmd};

fn add_layer() -> Op {
    Op::AddLayer {
        above: None,
        mode: Mode::Rgba,
        name: None,
    }
}

fn pixel(layer: i64, frame: i64) -> Op {
    Op::Pixel {
        layer,
        frame,
        x: 0,
        y: 0,
    }
}

fn dot(layer: i64, frame: i64, color: u32) -> Op {
    Op::SetPixel {
        layer,
        frame,
        x: 0,
        y: 0,
        color: i64::from(color),
    }
}

#[test]
fn a_removed_layer_s_id_is_never_given_again_in_one_run() {
    let mut e = project();
    let replies = run(&mut e, API_VERSION, |s| {
        let made = created(&s.call(add_layer())?);
        s.call(Op::RemoveLayer { layer: made })?;
        let next = created(&s.call(add_layer())?);
        Ok((made, next, s.call(Op::Layer { layer: made })))
    })
    .expect("runs");
    let (made, next, stale) = replies;
    assert_ne!(made, next);
    assert_eq!(stale, Err(ApiError::NoSuchLayer(made)));
}

#[test]
fn a_removed_layer_s_id_is_never_given_again_across_runs_undo_and_redo() {
    let mut e = project();
    let mut given = layer_ids(&e);
    let a = created(&op(&mut e, add_layer()));
    op(&mut e, Op::RemoveLayer { layer: a });
    let b = created(&op(&mut e, add_layer()));
    assert!(!given.contains(&b) && b != a);
    given.extend([a, b]);
    undo(&mut e);
    undo(&mut e);
    assert_eq!(
        layer_ids(&e).last(),
        Some(&a),
        "undo brings the layer back as itself"
    );
    redo(&mut e);
    let c = created(&op(&mut e, add_layer()));
    assert!(!given.contains(&c), "{c} is new, not one of {given:?}");
    undo(&mut e);
    let d = created(&op(&mut e, add_layer()));
    assert!(
        !given.contains(&d) && d != c,
        "an undone add's id is not given again"
    );
}

#[test]
fn a_stale_layer_reference_is_refused_and_never_names_a_newer_layer() {
    let mut e = project();
    let top = *layer_ids(&e).last().expect("a layer");
    op(&mut e, Op::RemoveLayer { layer: top });
    op(&mut e, add_layer());
    for stale in [
        Op::Layer { layer: top },
        pixel(top, frames(&e)[0]),
        Op::RemoveLayer { layer: top },
    ] {
        let result = run(&mut e, API_VERSION, |s| s.call(stale.clone()));
        assert_eq!(result, Err(ApiError::NoSuchLayer(top)), "{stale:?}");
    }
}

#[test]
fn an_id_from_a_failed_run_is_never_given_again() {
    let mut e = project();
    let mut given = 0;
    let failed: Result<(), ApiError> = run(&mut e, API_VERSION, |s| {
        given = created(&s.call(add_layer())?);
        Err(ApiError::Cancelled)
    });
    assert!(failed.is_err());
    assert_ne!(created(&op(&mut e, add_layer())), given);
}

#[test]
fn a_frame_handle_follows_its_frame_through_inserts_and_moves() {
    let mut e = project();
    let bottom = layer_ids(&e)[0];
    let second = frames(&e)[1];
    op(&mut e, dot(bottom, second, BLUE));
    op(&mut e, Op::AddFrame { at: 0, copy: false });
    op(&mut e, Op::AddFrame { at: 1, copy: false });
    assert_eq!(frames(&e)[3], second, "two frames went in before it");
    // A move from the interface, between runs.
    e.apply(FrameCmd::Move { from: 3, to: 0 }).expect("moves");
    assert_eq!(frames(&e)[0], second);
    assert_eq!(op(&mut e, pixel(bottom, second)), Reply::Color(BLUE));
    let Reply::Document(info) = op(&mut e, Op::Document) else {
        panic!("a document")
    };
    assert_eq!(info.frames[0] as i64, second);
}

#[test]
fn a_removed_frame_s_handle_is_refused_even_once_frames_are_added() {
    let mut e = project();
    let f = frames(&e);
    op(&mut e, Op::RemoveFrame { frame: f[1] });
    op(&mut e, Op::AddFrame { at: 1, copy: true });
    let bottom = layer_ids(&e)[0];
    for stale in [
        pixel(bottom, f[1]),
        Op::SetDuration {
            frame: f[1],
            ms: 50,
        },
        Op::RemoveFrame { frame: f[1] },
    ] {
        let result = run(&mut e, API_VERSION, |s| s.call(stale.clone()));
        assert_eq!(result, Err(ApiError::NoSuchFrame(f[1])), "{stale:?}");
    }
}

#[test]
fn undo_of_a_removal_makes_the_handle_name_the_same_frame_again() {
    let mut e = project();
    let bottom = layer_ids(&e)[0];
    let first = frames(&e)[0];
    op(&mut e, Op::RemoveFrame { frame: first });
    undo(&mut e);
    assert_eq!(frames(&e)[0], first);
    assert_eq!(op(&mut e, pixel(bottom, first)), Reply::Color(0));
    let dot_at = Op::Pixel {
        layer: bottom,
        frame: first,
        x: 2,
        y: 3,
    };
    assert_eq!(
        op(&mut e, dot_at),
        Reply::Color(RED),
        "the frame's own drawing"
    );
}

#[test]
fn a_frame_made_in_a_failed_run_never_has_its_handle_given_again() {
    let mut e = project();
    let mut made = 0;
    let failed: Result<(), ApiError> = run(&mut e, API_VERSION, |s| {
        made = made_frame(&s.call(Op::AddFrame { at: 2, copy: false })?);
        Err(ApiError::Cancelled)
    });
    assert!(failed.is_err());
    let again = made_frame(&op(&mut e, Op::AddFrame { at: 2, copy: false }));
    assert_ne!(again, made);
    let bottom = layer_ids(&e)[0];
    let stale = run(&mut e, API_VERSION, |s| s.call(pixel(bottom, made)));
    assert_eq!(stale, Err(ApiError::NoSuchFrame(made)));
}

#[test]
fn a_removed_layer_s_id_is_never_given_again_after_reopening() {
    let mut e = project();
    let top = *layer_ids(&e).last().expect("a layer");
    op(&mut e, Op::RemoveLayer { layer: top });
    let mut opened = reopen(&save(&e));
    let next = created(&op(&mut opened, add_layer()));
    assert!(next > top, "{next} after {top}");
    let stale = run(&mut opened, API_VERSION, |s| {
        s.call(Op::Layer { layer: top })
    });
    assert_eq!(stale, Err(ApiError::NoSuchLayer(top)));
}

#[test]
fn save_and_reopen_keep_every_layer_id_and_refuse_old_frame_handles() {
    let mut e = project();
    let a = created(&op(&mut e, add_layer()));
    created(&op(&mut e, add_layer()));
    op(&mut e, Op::RemoveLayer { layer: a });
    let (ids, old_frames) = (layer_ids(&e), frames(&e));
    let mut opened = reopen(&save(&e));
    assert_eq!(layer_ids(&opened), ids, "every persisted id is kept");
    let new_frames = frames(&opened);
    assert!(new_frames.iter().all(|f| !old_frames.contains(f)));
    let bottom = ids[0];
    let stale = run(&mut opened, API_VERSION, |s| {
        s.call(pixel(bottom, old_frames[0]))
    });
    assert_eq!(stale, Err(ApiError::NoSuchFrame(old_frames[0])));
}

#[test]
fn ids_used_up_but_not_kept_are_saved_and_nothing_else_changes() {
    let mut e = project();
    let before = save(&e);
    let layer = created(&op(&mut e, add_layer()));
    op(&mut e, Op::AddFrame { at: 0, copy: true });
    undo(&mut e);
    undo(&mut e);
    assert_eq!(save_but_ids(&e), before);
    assert_ne!(save(&e), before, "the ids used up are saved");
    let mut opened = reopen(&save(&e));
    let next = created(&op(&mut opened, add_layer()));
    assert!(next > layer, "{next} is not the undone layer's {layer}");
    assert_eq!(
        save(&reopen(&before)),
        before,
        "a reopened file saves as it was"
    );
}

#[test]
fn layer_commands_from_the_interface_use_the_same_ids() {
    let mut e = project();
    let top = *layer_ids(&e).last().expect("a layer");
    let layer = e.layer_id(1).expect("the top layer");
    e.apply(LayerCmd::Remove { layer }).expect("removes");
    e.apply(LayerCmd::Add {
        at: 1,
        mode: pikslar_core::ColorMode::Rgba,
    })
    .expect("room");
    assert_ne!(*layer_ids(&e).last().expect("a layer"), top);
    let result = run(&mut e, API_VERSION, |s| s.call(Op::Layer { layer: top }));
    assert_eq!(result, Err(ApiError::NoSuchLayer(top)));
}

#[test]
fn older_files_open_with_their_layers_numbered_and_frames_of_their_own() {
    let files: [&[u8]; 3] = [
        include_bytes!("../../pikslar-safety/corpus/v1-layers.piks"),
        include_bytes!("../../pikslar-safety/corpus/v2-animated.piks"),
        include_bytes!("../../pikslar-safety/corpus/v2-palette.piks"),
    ];
    for bytes in files {
        let mut e = reopen(bytes);
        let ids = layer_ids(&e);
        let numbered: Vec<i64> = (1..=ids.len() as i64).collect();
        assert_eq!(
            ids, numbered,
            "a file before version 3 numbers layers from 1"
        );
        let mut handles = frames(&e);
        handles.sort_unstable();
        handles.dedup();
        assert_eq!(handles.len(), e.doc().frames.len());
        let made = created(&op(&mut e, add_layer()));
        assert_eq!(made, ids.len() as i64 + 1);
        assert_eq!(layer_ids(&reopen(&save(&e)))[ids.len()], made);
    }
}

const GREEN: u32 = pikslar_core::rgba(20, 220, 20, 255);
const GOLD: u32 = pikslar_core::rgba(220, 200, 20, 255);

fn add_swatch(color: u32) -> Op {
    Op::AddSwatch {
        color: i64::from(color),
    }
}

fn swatches(e: &mut Editor) -> Vec<(u32, u32)> {
    match op(e, Op::Swatches) {
        Reply::Swatches(list) => list,
        other => panic!("expected swatches, got {other:?}"),
    }
}

#[test]
fn an_undone_new_swatch_s_id_is_never_given_again() {
    let mut e = project();
    let a = created(&op(&mut e, add_swatch(GREEN)));
    undo(&mut e);
    let b = created(&op(&mut e, add_swatch(GOLD)));
    assert_ne!(a, b, "a new color after the undo takes a new id");
    redo(&mut e);
    undo(&mut e);
    let c = created(&op(&mut e, add_swatch(GREEN)));
    assert!(c != a && c != b, "{c} is new, not {a} or {b}");
}

#[test]
fn a_failed_run_s_swatch_id_is_never_given_again() {
    let mut e = project();
    let mut given = None;
    let failed = run(&mut e, API_VERSION, |s| {
        given = Some(created(&s.call(add_swatch(GREEN))?));
        s.call(Op::SetOpacity {
            layer: 1,
            opacity: 300,
        })
    });
    assert!(failed.is_err());
    let next = created(&op(&mut e, add_swatch(GOLD)));
    assert_ne!(Some(next), given);
}

#[test]
fn a_stale_swatch_reference_is_refused_and_never_names_a_newer_color() {
    let mut e = project();
    let a = created(&op(&mut e, add_swatch(GREEN)));
    undo(&mut e);
    created(&op(&mut e, add_swatch(GOLD)));
    for stale in [
        Op::Recolor {
            swatch: a,
            color: i64::from(RED),
        },
        Op::RemoveSwatch { swatch: a },
    ] {
        let result = run(&mut e, API_VERSION, |s| s.call(stale));
        assert_eq!(result, Err(ApiError::NoSuchSwatch(a)));
    }
}

#[test]
fn save_and_reopen_keep_every_live_swatch_id() {
    let mut e = project();
    created(&op(&mut e, add_swatch(GREEN)));
    undo(&mut e);
    let b = created(&op(&mut e, add_swatch(GOLD)));
    let live = swatches(&mut e);
    let mut opened = reopen(&save(&e));
    assert_eq!(swatches(&mut opened), live);
    op(
        &mut opened,
        Op::Recolor {
            swatch: b,
            color: i64::from(RED),
        },
    );
    assert_eq!(save(&reopen(&save(&opened))), save(&opened));
}

#[test]
fn saves_of_documents_that_never_undo_a_new_color_are_unchanged() {
    let mut e = project();
    let before = save(&e);
    op(&mut e, add_swatch(GREEN));
    undo(&mut e);
    assert_eq!(save(&e), before, "an undone color leaves nothing behind");
    let mut plain = project();
    for one in [add_swatch(GREEN), add_swatch(GOLD), add_swatch(GREEN)] {
        op(&mut plain, one);
    }
    let mut fresh = reopen(&before);
    ops(&mut fresh, &[add_swatch(GREEN), add_swatch(GOLD)]);
    assert_eq!(
        save(&plain),
        save(&fresh),
        "the same colors, the same bytes"
    );
    assert_eq!(save(&reopen(&save(&plain))), save(&plain));
}

/// Paints `n` colors the table does not hold onto the palette layer: ids the table takes
/// but the run is never given, as the API names colors by value.
fn paint_new_colors(s: &mut pikslar_api::Script, layer: i64, frame: i64, n: u32) {
    for i in 0..n {
        let color = 0xff00_0000 | (0x0001_0000 + i * 7);
        let at = Op::SetPixel {
            layer,
            frame,
            x: i64::from(i % 16),
            y: i64::from(i / 16),
            color: i64::from(color),
        };
        s.call(at).expect("painted");
    }
}

#[test]
fn colors_a_run_only_painted_with_are_not_retired() {
    let mut e = project();
    let (layer, frame) = (layer_ids(&e)[1], frames(&e)[0]);
    let held = e.doc().colors.len();
    let failed = run(&mut e, API_VERSION, |s| {
        paint_new_colors(s, layer, frame, 100);
        Err::<(), _>(ApiError::NoSuchSwatch(0))
    });
    assert!(failed.is_err());
    let next = created(&op(&mut e, add_swatch(GOLD)));
    assert_eq!(
        next as usize, held,
        "no ids the run never gave out are padded"
    );
    // Kept, then undone: the same.
    let held = e.doc().colors.len();
    run(&mut e, API_VERSION, |s| {
        paint_new_colors(s, layer, frame, 100);
        Ok(())
    })
    .expect("kept");
    undo(&mut e);
    let next = created(&op(&mut e, add_swatch(GREEN)));
    assert_eq!(next as usize, held);
}

#[test]
fn a_swatch_id_a_run_read_stays_retired() {
    let mut e = project();
    let (layer, frame) = (layer_ids(&e)[1], frames(&e)[0]);
    let mut read = 0;
    let failed = run(&mut e, API_VERSION, |s| {
        paint_new_colors(s, layer, frame, 20);
        s.call(add_swatch(GOLD))?;
        if let Reply::Swatches(list) = s.call(Op::Swatches)? {
            read = list.iter().map(|w| w.0).max().unwrap_or(0);
        }
        Err::<(), _>(ApiError::NoSuchSwatch(0))
    });
    assert!(failed.is_err());
    let next = created(&op(&mut e, add_swatch(GREEN)));
    assert!(next > i64::from(read), "{next} goes above the read {read}");
}
