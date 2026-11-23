//! --scale and --layer: what each format draws with them, and what is refused before any file
//! is made. Expected pixels come from the compositor, apart from the export's own code.

mod common;
use common::{cli, folder, listing};
use pikslar_core::{BlendMode, ColorMode, Document, Grid};
use pikslar_format::png;
use pikslar_render::{flatten_document, Compositor};
use std::fs;
use std::path::{Path, PathBuf};

/// Names of the layers from the bottom: an opaque base, a translucent multiply layer at half
/// opacity, a hidden sketch, translucent ink, and two layers sharing a name.
const NAMES: [&str; 6] = ["base", "shade", "sketch", "ink", "dup", "dup"];

fn layered(side: usize, frames: usize) -> Document {
    let mut doc = Document::new(side, side, ColorMode::Rgba).expect("size");
    for at in 1..NAMES.len() {
        doc.add_layer(at, ColorMode::Rgba).expect("room");
    }
    for f in 1..frames {
        doc.add_frame(f, false).expect("room");
    }
    for (l, layer) in doc.layers_mut().iter_mut().enumerate() {
        layer.name = NAMES[l].to_string();
        layer.visible = l != 2;
        layer.opacity = [255, 128, 255, 200, 255, 90][l];
        layer.blend = if l == 1 {
            BlendMode::Multiply
        } else {
            BlendMode::Normal
        };
    }
    for f in 0..frames {
        for l in 0..NAMES.len() {
            let px: Vec<u32> = (0..side * side)
                .map(|i| {
                    let v = (i as u32 * (l as u32 + 3) + f as u32 * 17) & 0xff;
                    let alpha = if l == 0 { 255 } else { 40 + ((v * 3) & 0xbf) };
                    (alpha << 24) | (v << 16) | ((v ^ 0x5a) << 8) | (l as u32 * 40)
                })
                .collect();
            doc.set_cel(l, f, Grid::from_cells(side, side, &px))
                .expect("cel");
        }
    }
    doc
}

fn write_project(d: &Path, doc: &Document) -> PathBuf {
    let at = d.join("layers.piks");
    fs::write(&at, pikslar_format::piks::save(doc)).expect("write project");
    at
}

/// The frame with exactly `layers` shown (or as saved, if none), each pixel made a square.
fn expected(doc: &Document, layers: &[usize], frame: usize, scale: usize) -> Vec<u32> {
    let mut shown = doc.clone();
    if !layers.is_empty() {
        for (i, layer) in shown.layers_mut().iter_mut().enumerate() {
            layer.visible = layers.contains(&i);
        }
    }
    let px = flatten_document(&shown, frame, &mut Compositor::new(1, 1));
    let mut out = Vec::new();
    for row in px.chunks_exact(doc.width) {
        for _ in 0..scale {
            row.iter()
                .for_each(|&p| out.extend(std::iter::repeat_n(p, scale)));
        }
    }
    out
}

fn decode(path: &Path) -> png::Image {
    png::decode(&fs::read(path).expect("written")).expect("decodes")
}

/// Every option given after the project, the export's code, and the folder's files after.
fn export(d: &Path, project: &Path, extra: &[&str]) -> (i32, String, Vec<String>) {
    let out = d.join("out.png");
    let mut args: Vec<&dyn AsRef<std::ffi::OsStr>> = vec![&"export", &project, &"--out", &out];
    args.extend(extra.iter().map(|a| a as &dyn AsRef<std::ffi::OsStr>));
    let run = cli(&args);
    (run.code, run.err, listing(d))
}

#[test]
fn each_format_draws_the_chosen_layers_at_the_chosen_scale() {
    let d = folder("controls-draw");
    let doc = layered(9, 3);
    let project = write_project(&d, &doc);
    let cases: [(&[&str], &[usize]); 5] = [
        (&[], &[]),
        (&["--layer", "sketch"], &[2]),
        (&["--layer", "ink", "--layer", "base"], &[0, 3]),
        (
            &["--layer", "shade", "--layer", "base", "--layer", "shade"],
            &[0, 1],
        ),
        (
            &["--layer", "base", "--layer", "shade", "--layer", "sketch"],
            &[0, 1, 2],
        ),
    ];
    for (layer_args, layers) in cases {
        for scale in [1, 2, 3, 4] {
            let s = scale.to_string();
            let mut args = vec!["--scale", s.as_str(), "--overwrite", "--frame", "2"];
            args.extend_from_slice(layer_args);
            let (code, err, _) = export(&d, &project, &args);
            assert_eq!(code, 0, "{err}");
            let image = decode(&d.join("out.png"));
            assert_eq!((image.width, image.height), (9 * scale, 9 * scale));
            assert!(
                image.px == expected(&doc, layers, 1, scale),
                "{layers:?} at {scale}"
            );
            args.splice(3..5, ["--format", "sequence"]);
            assert_eq!(export(&d, &project, &args).0, 0);
            for frame in 0..3 {
                let member = decode(&d.join(format!("out_{:04}.png", frame + 1)));
                assert!(
                    member.px == expected(&doc, layers, frame, scale),
                    "member {frame}"
                );
            }
            let text = fs::read_to_string(d.join("out.json")).expect("description");
            let side = 9 * scale;
            assert!(
                text.contains(&format!("\"size\":{{\"w\":{side},\"h\":{side}}}")),
                "{text}"
            );
        }
    }
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn a_scaled_sheet_places_each_frame_at_its_scaled_place() {
    let d = folder("controls-sheet");
    let doc = layered(7, 4);
    let project = write_project(&d, &doc);
    let args = [
        "--format", "sheet", "--scale", "3", "--layer", "shade", "--layer", "ink",
    ];
    let (code, err, _) = export(&d, &project, &args);
    assert_eq!(code, 0, "{err}");
    let image = decode(&d.join("out.png"));
    assert_eq!((image.width, image.height), (4 * 21, 21));
    for frame in 0..4 {
        let want = expected(&doc, &[1, 3], frame, 3);
        for (r, row) in want.chunks_exact(21).enumerate() {
            let at = r * image.width + frame * 21;
            assert_eq!(&image.px[at..at + 21], row, "frame {frame} row {r}");
        }
    }
    let text = fs::read_to_string(d.join("out.json")).expect("description");
    assert!(
        text.contains("\"frame\":{\"x\":63,\"y\":0,\"w\":21,\"h\":21}"),
        "{text}"
    );
    assert!(text.contains("\"scale\":\"3\""), "{text}");
    fs::remove_dir_all(d).expect("cleanup");
}

fn lock_free_and_same(d: &Path, before: &[(String, Vec<u8>)], lock: &Path) -> bool {
    let after: Vec<(String, Vec<u8>)> = listing(d)
        .into_iter()
        .map(|n| (n.clone(), fs::read(d.join(&n)).expect("read")))
        .collect();
    after == before && !lock.exists()
}

#[test]
fn what_cannot_be_drawn_is_refused_before_any_file_is_made_or_replaced() {
    let d = folder("controls-refused");
    let doc = layered(64, 2);
    let project = write_project(&d, &doc);
    for format in ["png", "sheet", "sequence"] {
        assert_eq!(
            export(&d, &project, &["--format", format, "--overwrite"]).0,
            0
        );
    }
    let before: Vec<(String, Vec<u8>)> = listing(&d)
        .into_iter()
        .map(|n| (n.clone(), fs::read(d.join(&n)).expect("read")))
        .collect();
    let lock = pikslar_format::set_lock::lock_path(&d.join("out.json"));
    let refusals: [(&[&str], &str); 6] = [
        (&["--scale", "129"], "8256 by 8256"),
        (&["--scale", "18446744073709551615"], "at most 8192 a side"),
        (&["--layer", "nobody"], "no layer named \"nobody\""),
        (
            &["--layer", "base", "--layer", "Ink"],
            "no layer named \"Ink\"",
        ),
        (&["--layer", "dup"], "2 layers are named \"dup\""),
        (&["--scale", "65", "--layer", "dup"], "2 layers are named"),
    ];
    for (args, says) in refusals {
        for format in ["png", "sheet", "sequence"] {
            let mut all = vec!["--format", format, "--overwrite"];
            all.extend_from_slice(args);
            let (code, err, _) = export(&d, &project, &all);
            assert_eq!(code, 3, "{format} {args:?}: {err}");
            assert!(err.contains(says), "{err}");
            let after: Vec<(String, Vec<u8>)> = listing(&d)
                .into_iter()
                .map(|n| (n.clone(), fs::read(d.join(&n)).expect("read")))
                .collect();
            assert!(after == before, "{format} {args:?} changed the folder");
            assert!(!lock.exists(), "{format} {args:?} left a lock");
        }
    }
    let (code, err, _) = export(&d, &project, &["--format", "sheet", "--scale", "100"]);
    assert_eq!(code, 3, "6400 a side fits once, not twice: {err}");
    assert!(err.contains("do not fit in one sheet"), "{err}");
    assert!(lock_free_and_same(&d, &before, &lock));
    let sheet = ["--format", "sheet", "--overwrite", "--scale", "2"];
    let (code, err, _) = export(&d, &project, &sheet);
    assert_eq!(code, 0, "128 by 128 frames fit a sheet: {err}");
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn a_scale_or_layer_that_is_not_one_exits_1_and_writes_nothing() {
    let d = folder("controls-args");
    let project = write_project(&d, &layered(4, 1));
    let cases: [&[&str]; 8] = [
        &["--scale", "0"],
        &["--scale", "-2"],
        &["--scale", "2x"],
        &["--scale", "1.5"],
        &["--scale", "99999999999999999999999"],
        &["--scale", "2", "--scale", "2"],
        &["--scale"],
        &["--layer"],
    ];
    for args in cases {
        let (code, err, files) = export(&d, &project, args);
        assert_eq!(code, 1, "{args:?}: {err}");
        assert_eq!(files, ["layers.piks"], "{args:?}");
    }
    let (_, err, _) = export(&d, &project, &["--scale", "0"]);
    assert!(err.contains("whole number from 1"), "{err}");
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn the_largest_scale_of_a_small_project_is_streamed_whole() {
    let d = folder("controls-largest");
    let doc = layered(2, 1);
    let project = write_project(&d, &doc);
    let (code, err, _) = export(&d, &project, &["--scale", "4096", "--layer", "ink"]);
    assert_eq!(code, 0, "{err}");
    let image = decode(&d.join("out.png"));
    assert_eq!((image.width, image.height), (8192, 8192));
    assert!(image.px == expected(&doc, &[3], 0, 4096));
    assert_eq!(
        export(&d, &project, &["--scale", "4097", "--overwrite"]).0,
        3
    );
    fs::remove_dir_all(d).expect("cleanup");
}
