//! Scripts reach the document only through the host functions their capabilities grant, and
//! each does what its API operation does.

mod common;

use common::*;
use pikslar_api::{run_ops, ApiError, Op, API_VERSION};
use pikslar_bind::{functions, Capabilities, Capability, Failure, ScriptLimits};
use pikslar_core::{ColorMode, Document, Editor, StepCmd};
use pikslar_script::ErrorKind;

#[test]
fn an_ordinary_script_is_one_undo_step() {
    let mut e = project();
    let before = state(&e);
    let plain = but_ids(&e);
    let src = r#"
        let ls = layers();
        for i in 0..len(ls) { rename_layer(ls[i], "Part " + str(i + 1)); }
        let f = add_frame(len(frames()));
        set_duration(f, 250);
        let all = frames();
        add_tag("walk", all[0], all[len(all) - 1]);
        let ink = add_layer("Ink");
        print(layer_name(ink));
        return len(frames());
    "#;
    let out = run(&mut e, src);
    assert_eq!(out.result, Ok("3".into()));
    assert_eq!(out.printed, "Ink\n");
    let names: Vec<_> = e.doc().layers().iter().map(|l| l.name.clone()).collect();
    assert_eq!(names, ["Part 1", "Part 2", "Ink"]);
    assert_eq!(e.doc().frames.len(), 3);
    assert_eq!(e.doc().frames[2].duration_ms, 250);
    assert_eq!(e.doc().tags.len(), 1);
    assert_eq!(e.history().undo_depth(), before.undo + 1, "one step");
    let after = save(&e);
    e.apply(StepCmd::Undo).expect("undoes");
    // Everything but the ids the run used up, which stay used up.
    assert_eq!(
        but_ids(&e).bytes,
        plain.bytes,
        "undo brings back the document"
    );
    e.apply(StepCmd::Redo).expect("redoes");
    assert_eq!(save(&e), after);
}

#[test]
fn set_pixel_saves_as_the_same_api_operations_one_by_one() {
    let (mut by_script, mut by_ops) = (project(), project());
    let src = r#"
        let ls = layers();
        let fs = frames();
        for y in 0..24 { for x in 0..32 {
            if (x * 7 + y * 3) % 5 != 0 {
                set_pixel(ls[(x + y) % 2], fs[y % 2], x, y, 0xff000000 + x * 4 + y * 1024);
            }
            if x == 31 && y % 6 == 0 { let seen = get_pixel(ls[0], fs[0], 2, 3); }
        } }
        set_pixel(ls[0], fs[0], 5, 5, 0);
    "#;
    ok(&mut by_script, src);
    let (ls, fs) = (layer_ids(&by_ops), frames(&by_ops));
    let mut list = Vec::new();
    for y in 0..24i64 {
        for x in 0..32i64 {
            if (x * 7 + y * 3) % 5 != 0 {
                list.push(Op::SetPixel {
                    layer: ls[((x + y) % 2) as usize],
                    frame: fs[(y % 2) as usize],
                    x,
                    y,
                    color: 0xff00_0000 + x * 4 + y * 1024,
                });
            }
        }
    }
    list.push(Op::ErasePixel {
        layer: ls[0],
        frame: fs[0],
        x: 5,
        y: 5,
    });
    run_ops(&mut by_ops, API_VERSION, &list).expect("runs");
    assert_eq!(save(&by_script), save(&by_ops));
}

#[test]
fn bulk_functions_do_what_their_operations_do() {
    let mut e = project();
    let src = r#"
        let l = layers()[0];
        let f = frames()[0];
        fill_rect(l, f, 0, 0, 32, 24, 0xff102030);
        draw_line(l, f, 0, 0, 31, 23, 0xff0000ff);
        fill_rect(l, f, 10, 10, 4, 4, 0);
        let block = read_rect(l, f, 9, 10, 2, 1);
        write_rect(l, f, 0, 23, 3, [0xffffffff, 0, 0xffffffff]);
        return [block[0], block[1], get_pixel(l, f, 0, 23), get_pixel(l, f, 1, 23)];
    "#;
    let want = format!(
        "[{}, 0, {}, {}]",
        0xff10_2030_u32,
        u32::MAX,
        0xff10_2030_u32
    );
    assert_eq!(ok(&mut e, src), want);
}

#[test]
fn a_run_without_a_capability_cannot_name_its_functions() {
    let mut e = project();
    let before = state(&e);
    let edit = "set_pixel(layers()[0], frames()[0], 0, 0, 0xffffffff);";
    let read_only = run_with(&mut e, edit, Capabilities::READ, ScriptLimits::default());
    let Err(Failure::Script(err)) = read_only.result else {
        panic!("compiled without the capability: {:?}", read_only.result);
    };
    assert_eq!(err.kind, ErrorKind::Syntax);
    assert!(err.message.contains("set_pixel"), "{err}");
    let blind = Capabilities::NONE.with(Capability::EditDocument);
    let out = run_with(&mut e, "return width();", blind, ScriptLimits::default());
    assert_eq!(
        out.result.map_err(|f| f.kind()),
        Err(Some(ErrorKind::Syntax))
    );
    let none = run_with(
        &mut e,
        "print(1); return 2;",
        Capabilities::NONE,
        ScriptLimits::default(),
    );
    assert_eq!(none.result, Ok("2".into()));
    assert_eq!(state(&e), before);
}

#[test]
fn nothing_outside_the_host_table_exists() {
    let mut e = project();
    for name in [
        "open",
        "read_file",
        "exec",
        "env",
        "time",
        "clock",
        "socket",
        "system",
    ] {
        let out = run(&mut e, &format!("{name}(\"x\");"));
        assert_eq!(
            out.result.map_err(|f| f.kind()),
            Err(Some(ErrorKind::Syntax)),
            "{name}"
        );
    }
    // Files only through a picker the user answers, for a run granted that, and no paths.
    for f in functions() {
        assert!(!f.name.contains("path"), "{}", f.name);
        if f.name.contains("file") {
            let pickers = [Capability::PickFileToRead, Capability::PickFileToWrite];
            assert!(f.needs.is_some_and(|c| pickers.contains(&c)), "{}", f.name);
        }
        if f.name.contains("write_file") || f.name.contains("to_write") {
            assert_eq!(f.needs, Some(Capability::PickFileToWrite), "{}", f.name);
        }
    }
}

#[test]
fn stale_references_fail_loudly_where_they_are_used() {
    let mut e = project();
    let plain = but_ids(&e);
    let src = "let l = add_layer(\"Temp\");\nremove_layer(l);\nset_pixel(l, frames()[0], 0, 0, 0xffffffff);";
    let out = run(&mut e, src);
    let Err(Failure::Refused {
        error,
        at: Some(at),
        ..
    }) = out.result
    else {
        panic!("not refused: {:?}", out.result);
    };
    assert!(matches!(error, ApiError::NoSuchLayer(_)), "{error:?}");
    assert_eq!(at.line, 3);
    let src = "let f = add_frame(0);\nremove_frame(f);\nset_duration(f, 100);";
    let out = run(&mut e, src);
    assert!(matches!(
        out.result,
        Err(Failure::Refused { error: ApiError::NoSuchFrame(_), at: Some(p), .. }) if p.line == 3
    ));
    let src = "set_pixel(layers()[0], frames()[0], 32, 0, 0xffffffff);";
    let out = run(&mut e, src);
    assert!(matches!(
        out.result,
        Err(Failure::Refused {
            error: ApiError::Outside { x: 32, y: 0 },
            ..
        })
    ));
    // The frame the second script added and removed has used up its id; nothing else changed.
    assert_eq!(but_ids(&e), plain);
}
#[test]
fn set_pixel_on_a_layer_that_cannot_be_drawn_on_is_refused_where_it_is_called() {
    let mut e = project();
    let id = e.layer_id(0).expect("a layer");
    e.apply(pikslar_core::LayerCmd::SetLocked {
        layer: id,
        locked: true,
    })
    .expect("locks");
    let before = state(&e);
    let src = "let n = 1;\nset_pixel(layers()[0], frames()[0], 1, 1, 0xffffffff);\nlet m = len(swatches());";
    let out = run(&mut e, src);
    assert!(
        matches!(
            out.result,
            Err(Failure::Refused { error: ApiError::Refused(pikslar_core::LimitError::Locked), at: Some(p), waiting: false }) if p.line == 2
        ),
        "{:?}",
        out.result
    );
    assert_eq!(state(&e), before);
}

/// A palette document whose color table is full, so new colors are refused when written.
fn full_table() -> Editor {
    let mut doc = Document::new(16, 16, ColorMode::Palette).expect("a canvas");
    let mut c = 0xff00_0000u32;
    while std::sync::Arc::make_mut(&mut doc.colors).id_for(c).is_ok() {
        c += 1;
    }
    Editor::new(doc)
}

#[test]
fn waiting_pixels_refused_inside_another_call_say_so() {
    let mut e = full_table();
    let before = state(&e);
    let src = "let l = layers()[0];\nset_pixel(l, frames()[0], 1, 1, 0xfffedcba);\nadd_frame(0);";
    let out = run(&mut e, src);
    let failure = out.result.expect_err("the table is full");
    assert!(
        matches!(&failure, Failure::Refused { error: ApiError::Refused(pikslar_core::LimitError::TableFull), at: Some(p), waiting: true } if p.line == 3),
        "{failure:?}"
    );
    assert!(
        failure.to_string().contains("set_pixel (waiting pixels)"),
        "{failure}"
    );
    // At the end of the run they are the waiting pixels too.
    let out = run(
        &mut e,
        "set_pixel(layers()[0], frames()[0], 1, 1, 0xfffedcba);",
    );
    assert!(
        matches!(
            out.result,
            Err(Failure::Refused {
                at: None,
                waiting: true,
                ..
            })
        ),
        "{:?}",
        out.result
    );
    assert_eq!(state(&e), before);
}

#[test]
fn a_cancel_the_api_reports_is_a_cancel() {
    let f = Failure::Refused {
        error: ApiError::Cancelled,
        at: None,
        waiting: false,
    };
    assert!(f.is_cancel());
    assert!(f.is_stop());
}

#[test]
fn wrong_argument_types_are_script_errors() {
    let mut e = project();
    let before = state(&e);
    for src in [
        "rename_layer(\"a\", \"b\");",
        "set_visible(layers()[0], 1);",
        "write_rect(layers()[0], frames()[0], 0, 0, 1, [\"red\"]);",
        "write_rect(layers()[0], frames()[0], 0, 0, 1, 5);",
    ] {
        let out = run(&mut e, src);
        assert_eq!(
            out.result.map_err(|f| f.kind()),
            Err(Some(ErrorKind::Host)),
            "{src}"
        );
    }
    assert_eq!(state(&e), before);
}

#[test]
fn a_busy_editor_refuses_a_run() {
    let mut e = project();
    e.apply(pikslar_core::DrawCmd::Begin {
        tool: pikslar_core::Tool::Pencil,
        x: 1,
        y: 1,
    })
    .expect("a stroke starts");
    let out = run(&mut e, "add_layer(\"x\");");
    assert_eq!(
        out.result,
        Err(Failure::Refused {
            error: ApiError::Refused(pikslar_core::LimitError::Busy),
            at: None,
            waiting: false,
        })
    );
}

#[test]
fn a_huge_read_is_refused_before_any_work() {
    let mut e = project();
    let limits = ScriptLimits {
        memory: 1 << 20,
        ..ScriptLimits::default()
    };
    let src = "let l = layers()[0]; let f = frames()[0]; for i in 0..1000 { let b = read_rect(l, f, 0, 0, 32, 24); }";
    assert!(run_with(&mut e, src, Capabilities::ALL, limits)
        .result
        .is_ok());
    let src = "read_rect(layers()[0], frames()[0], 0, 0, 100000, 100000);";
    let out = run_with(&mut e, src, Capabilities::ALL, limits);
    assert!(
        out.fuel_used < 1000,
        "refused before burning: {}",
        out.fuel_used
    );
    assert_eq!(out.result.map_err(|f| f.kind()), Err(Some(ErrorKind::Host)));
}

/// Work that grows with the pixels it touches is charged for them before it starts, so a run
/// out of fuel never starts a fill it cannot pay for.
#[test]
fn pixel_work_is_paid_for_by_the_pixel_before_it_runs() {
    let mut e = project();
    let fuel = |e: &mut Editor, src: &str| run(e, src).fuel_used;
    let one = "let l = layers()[0]; let f = frames()[0];";
    let rect = fuel(
        &mut e,
        &format!("{one} fill_rect(l, f, 0, 0, 32, 24, 0xff0000ff);"),
    );
    let fill = fuel(&mut e, &format!("{one} fill(l, f, 0, 0, 0xff00ff00);"));
    let line = fuel(
        &mut e,
        &format!("{one} draw_line(l, f, 0, 0, 31, 0, 0xff00ff00);"),
    );
    assert!(rect >= 32 * 24 && fill >= 32 * 24, "{rect} {fill}");
    assert!(line >= 4 * 31, "{line}");
    let poor = ScriptLimits {
        fuel: 10_000,
        ..ScriptLimits::default()
    };
    let mut big = Editor::new(Document::new(200, 200, ColorMode::Rgba).expect("a canvas"));
    let before = save(&big);
    let out = run_with(
        &mut big,
        &format!("{one} fill_rect(l, f, 0, 0, 200, 200, 0xff0000ff);"),
        Capabilities::ALL,
        poor,
    );
    assert!(out.result.is_err_and(|f| f.is_stop()));
    assert!(out.fuel_used <= 10_000);
    assert_eq!(save(&big), before);
}

/// A small canvas with `frames` frames, `layers` layers and `swatches` swatches.
fn wide(frames: usize, layers: usize, swatches: u32) -> Editor {
    let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("a canvas");
    for at in 1..frames {
        doc.add_frame(at, false).expect("room");
    }
    for at in 1..layers {
        doc.add_layer(at, ColorMode::Rgba).expect("room");
    }
    let colors = std::sync::Arc::make_mut(&mut doc.colors);
    for c in 0..swatches {
        colors.add_swatch(0xff00_0000 + c).expect("room");
    }
    Editor::new(doc)
}

/// Reads and palette edits look through every frame, layer or swatch they name one by: each
/// is priced by how many there are, so a loop of them on the widest document still stops on
/// time (`pikslar-bench script`, `hostile_frame_reads` and `hostile_swatch_edits`).
#[test]
fn lookups_are_priced_by_what_they_look_through() {
    let src = "let l = layers()[len(layers()) - 1]; let f = frames()[len(frames()) - 1]; let s = swatches(); let fs = 0;\n";
    let per = |e: &mut Editor, body: &str| {
        let base = run(e, &format!("{src} for i in 0..100 {{ }}")).fuel_used;
        let out = run(e, &format!("{src} for i in 0..100 {{ {body} }}"));
        assert!(out.result.is_ok(), "{:?}", out.result);
        (out.fuel_used - base) / 100
    };
    let (mut small, mut many_frames) = (wide(1, 1, 1), wide(4096, 1, 1));
    let read = "get_pixel(l, f, 0, 0);";
    assert!(per(&mut many_frames, read) >= per(&mut small, read) + 4096 / 64);
    let mut many_layers = wide(1, 256, 1);
    let look = "layer_visible(l);";
    assert!(per(&mut many_layers, look) >= per(&mut small, look) + 256 / 64);
    let mut many_swatches = wide(1, 1, 65_000);
    let edit = "recolor(s[0], 0xff123456);";
    assert!(per(&mut many_swatches, edit) >= per(&mut small, edit) + 65_000 / 4);
    for read in ["swatches();", "swatch_color(s[0]);"] {
        let (m, s) = (per(&mut many_swatches, read), per(&mut small, read));
        assert!(m >= s + 64_999, "{read} {m} {s}");
    }
}

#[test]
fn long_text_arguments_are_refused_before_they_are_copied() {
    let mut e = project();
    let before = state(&e);
    let out = run(&mut e, "add_layer(repeat(\"x\", 15000000));");
    let failure = out.result.expect_err("refused");
    let said = failure.to_string();
    assert!(said.len() < 300, "{} bytes", said.len());
    assert!(said.contains("longer than 1024 bytes"), "{said}");
    assert_eq!(
        failure.kind(),
        Some(ErrorKind::Host),
        "not passed on to the API"
    );
    // A name a little too long is still the API's refusal, quoted short.
    let out = run(&mut e, "add_layer(repeat(\"y\", 500));");
    let failure = out.result.expect_err("refused");
    assert!(
        matches!(&failure, Failure::Refused { error: ApiError::BadName { name, .. }, .. } if name.chars().count() == 65),
        "{failure:?}"
    );
    assert_eq!(state(&e), before);
}

#[test]
fn write_rect_refuses_more_colors_than_the_canvas_before_copying_them() {
    let mut e = project();
    let out = run(
        &mut e,
        "write_rect(layers()[0], frames()[0], 0, 0, 32, list(32 * 24 + 1, 0));",
    );
    assert!(
        matches!(
            out.result,
            Err(Failure::Refused {
                error: ApiError::OutOfRange {
                    what: "the number of colors",
                    ..
                },
                ..
            })
        ),
        "{:?}",
        out.result
    );
}

/// Adding or removing a frame moves every layer's cel entries, so on a long timeline of many
/// layers it is paid for by the cel: a loop of them stops by fuel as fast as on a small one.
#[test]
fn frame_edits_are_paid_for_by_the_cel() {
    let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("a canvas");
    for at in 1..256 {
        doc.add_layer(at, ColorMode::Rgba).expect("room");
    }
    for at in 1..512 {
        doc.add_frame(at, false).expect("room");
    }
    let mut e = Editor::new(doc);
    let cels = 256 * 512;
    for src in [
        "add_frame(0);",
        "copy_frame(0);",
        "remove_frame(frames()[0]);",
    ] {
        let out = run(&mut e, src);
        assert!(out.result.is_ok(), "{src}: {:?}", out.result);
        assert!(out.fuel_used >= cels / 8, "{src}: {} units", out.fuel_used);
        e.apply(StepCmd::Undo).expect("undoes");
    }
}
