//! M12 red team: every host function given hostile arguments, on every kind of layer (locked,
//! hidden, tilemap, group), with waiting pixels in between, and host calls at the deepest
//! native stack the limits allow. A failure must change nothing; a success must never touch
//! a locked layer; nothing may panic or overflow the stack.

mod common;

use common::{project, run_with, state, State, RED};
use pikslar_bind::{functions, Capabilities, Failure, Outcome, ScriptLimits};
use pikslar_core::tileset::TilesetId;
use pikslar_core::{ColorMode, Document, Editor, LayerCmd, TileCmd};

/// The project, with a tilemap layer, a locked layer, a hidden layer and a group added.
fn hostile_project() -> Editor {
    let mut e = project();
    let made = e
        .apply(TileCmd::NewTileset {
            name: "Walls".into(),
            mode: ColorMode::Rgba,
            tile_w: 4,
            tile_h: 4,
        })
        .expect("a tileset");
    let tileset = TilesetId::new(made.created.expect("an id"));
    e.apply(TileCmd::NewLayer { at: 2, tileset })
        .expect("a tilemap layer");
    for at in [3, 4] {
        e.apply(LayerCmd::Add {
            at,
            mode: ColorMode::Rgba,
        })
        .expect("room");
    }
    let locked = e.layer_id(3).expect("a layer");
    e.apply(LayerCmd::SetLocked {
        layer: locked,
        locked: true,
    })
    .expect("locks");
    let hidden = e.layer_id(4).expect("a layer");
    e.apply(LayerCmd::SetVisible {
        layer: hidden,
        visible: false,
    })
    .expect("hides");
    let grouped = e.layer_id(1).expect("a layer");
    e.apply(LayerCmd::Group { layer: grouped })
        .expect("a group");
    e
}

/// Every layer's id, and the frame handles.
fn ids(e: &Editor) -> (Vec<i64>, Vec<i64>) {
    (common::layer_ids(e), common::frames(e))
}

/// The locked layer's id in [`hostile_project`].
fn locked_layer(e: &Editor) -> i64 {
    common::layer_ids(e)[3]
}

/// The locked layer's pixels in each frame, by frame handle, read by a script that may only
/// read; nothing if the layer is gone. Removing a locked layer, or a frame, is allowed, as
/// in the interface: a lock keeps pixels from being drawn on.
fn locked_pixels(e: &mut Editor, layer: i64) -> Vec<(String, String)> {
    let src = format!(
        "let out = \"\"; for f in frames() {{ out = out + str(f) + \"=\" +
         str(read_rect({layer}, f, 0, 0, width(), height())) + \";\"; }} return out;"
    );
    let out = run_with(e, &src, Capabilities::READ, ScriptLimits::default());
    let text = out.result.unwrap_or_default();
    text.split_terminator(';')
        .filter_map(|cel| cel.split_once('='))
        .map(|(f, px)| (f.to_string(), px.to_string()))
        .collect()
}

/// Runs `src` on a fresh [`hostile_project`] and checks the invariants, giving the outcome.
fn checked(src: &str) -> Outcome {
    let mut e = hostile_project();
    let layer = locked_layer(&e);
    let before: State = state(&e);
    let locked = locked_pixels(&mut e, layer);
    let limits = ScriptLimits {
        fuel: 2_000_000,
        ..ScriptLimits::default()
    };
    let out = run_with(&mut e, src, Capabilities::ALL, limits);
    match &out.result {
        Err(Failure::Thread(why)) => panic!("{src}: the thread stopped: {why}"),
        Err(f) => assert_eq!(
            state(&e),
            before,
            "{src}: failed with {f} but changed things"
        ),
        Ok(_) => {
            for (frame, px) in locked_pixels(&mut e, layer) {
                if let Some((_, was)) = locked.iter().find(|(f, _)| *f == frame) {
                    assert_eq!(&px, was, "{src}: the locked layer changed in frame {frame}");
                }
            }
        }
    }
    out
}

fn hostile_values(layers: &[i64], frames: &[i64]) -> Vec<String> {
    let mut v: Vec<String> = [
        "0",
        "-1",
        "9223372036854775807",
        "(-9223372036854775807 - 1)",
        "4294967296",
        "8193",
        "1.5",
        "\"\"",
        "\"Multiply\"",
        "nil",
        "true",
        "[]",
        "[1, 2]",
        "list(64, 4278190335)",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    v.extend(layers.iter().map(|l| l.to_string()));
    v.extend(frames.iter().map(|f| f.to_string()));
    v.push((frames.iter().max().copied().unwrap_or(0) + 1).to_string());
    v
}

/// A plausible argument list: a layer, a frame, small numbers and an opaque color, so that
/// replacing one argument at a time reaches the checks behind the first ones.
fn plausible(arity: usize, layer: i64, frame: i64) -> Vec<String> {
    let mut a = vec![layer.to_string(), frame.to_string()];
    a.extend(["1", "1", "2", "2", "4278190335"].map(String::from));
    a.truncate(arity);
    a
}

#[test]
fn every_host_function_with_hostile_arguments_on_every_layer_kind() {
    let (layers, frames) = ids(&hostile_project());
    let values = hostile_values(&layers, &frames);
    let mut runs = 0;
    for f in functions().iter().filter(|f| !f.name.contains("file")) {
        for &layer in &layers {
            let base = plausible(f.arity, layer, frames[0]);
            checked(&format!("{}({});", f.name, base.join(", ")));
            for at in 0..f.arity {
                for v in &values {
                    let mut args = base.clone();
                    args[at] = v.clone();
                    checked(&format!("{}({});", f.name, args.join(", ")));
                    runs += 1;
                }
            }
        }
    }
    println!("{runs} hostile calls");
}

/// Pixels waiting for a cel, then an operation that writes them, refuses, or reads.
#[test]
fn waiting_pixels_meet_every_kind_of_layer_and_failing_operations() {
    let (layers, frames) = ids(&hostile_project());
    let (f0, f1) = (frames[0], frames[1]);
    let afterwards = [
        String::new(),
        format!("get_pixel({}, {f0}, 1, 1);", layers[0]),
        "width();".to_string(),
        "add_layer(\"x\");".to_string(),
        "set_opacity(999999, 3);".to_string(),
        format!("set_pixel({}, {f1}, 0, 0, {RED});", layers[0]),
        format!("set_pixel({}, {f0}, 99, 0, {RED});", layers[0]),
        format!("remove_frame({f0});"),
        "return 1 / 0;".to_string(),
        "while true { }".to_string(),
    ];
    for &layer in &layers {
        for next in &afterwards {
            let src = format!(
                "for i in 0..300 {{ set_pixel({layer}, {f0}, i % 32, i / 32, {RED}); }}\n{next}"
            );
            checked(&src);
        }
    }
}

/// Host calls at the deepest stack the limits allow, in whatever build this is: each script
/// recurses as far as it may with every frame nested deep in an expression, then calls a host
/// function at the bottom. The test process would abort on a stack overflow.
#[test]
fn host_calls_at_the_deepest_native_stack() {
    let mut e = Editor::new(Document::new(512, 512, ColorMode::Rgba).expect("a canvas"));
    let nest = format!("{}deep(n - 1){}", "(1 + ".repeat(28), ")".repeat(28));
    let calls = [
        "fill(l, f, 3, 3, 4278190335)",
        "draw_line(l, f, 0, 0, 511, 511, 4278190335)",
        "fill_rect(l, f, 0, 0, 512, 512, 4278190335)",
        "len(read_rect(l, f, 0, 0, 256, 256))",
        "write_rect(l, f, 0, 0, 16, list(256, 4278190335))",
        "duplicate_layer(l)",
        "add_frame(1)",
    ];
    let limits = ScriptLimits {
        call_depth: 256,
        ..ScriptLimits::default()
    };
    for call in calls {
        let src = format!(
            "fn deep(n) {{ if n == 0 {{ let l = layers()[0]; let f = frames()[0]; {call};\n\
             return 0; }} return {nest}; }}\nreturn deep(250);"
        );
        let out = run_with(&mut e, &src, Capabilities::ALL, limits);
        assert!(
            !matches!(out.result, Err(Failure::Thread(_))),
            "{call}: {:?}",
            out.result
        );
        println!("{call}: {:?}", out.result.as_ref().map(|_| ()));
        let reached = match &out.result {
            Ok(_) | Err(Failure::Refused { .. }) => true,
            Err(f) => f.kind() == Some(pikslar_script::ErrorKind::CallDepth),
        };
        assert!(reached, "{call}: {:?}", out.result);
    }
}

/// Every failure's text is bounded, as what a script prints and returns are (64 KiB each):
/// the panel shows it in one status line.
#[test]
fn a_refusal_quotes_at_most_a_bounded_part_of_script_data() {
    for call in [
        "set_blend(layers()[0], s)",
        "rename_layer(layers()[0], s)",
        "add_layer(s)",
        "add_tag(s, frames()[0], frames()[0])",
    ] {
        let src = format!("let s = repeat(\"\\n\", 4000000); {call};");
        let out = checked(&src);
        let text = out.result.expect_err("refused").to_string();
        assert!(
            text.len() <= 65 << 10,
            "{call}: {} bytes of error",
            text.len()
        );
    }
}
