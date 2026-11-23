//! --indexed: each format, with layers and scales, against the same export without it; and
//! what is refused, before any file is made or replaced.

mod common;
use common::{cli, folder, listing};
use pikslar_core::{BlendMode, ColorMode, Direction, Document, Grid, Tag};
use pikslar_format::png;
use std::fs;
use std::path::{Path, PathBuf};

/// Three frames of an opaque base in six colours, a multiply layer at half opacity, a hidden
/// layer and translucent ink: blending makes colours no cell holds, under 256 in all. Frames
/// 2 and 3 are tagged, so a sheet has a row of one under a row of two and space beside it.
fn few_colours(side: usize) -> Document {
    let mut doc = Document::new(side, side, ColorMode::Rgba).expect("size");
    for at in 1..4 {
        doc.add_layer(at, ColorMode::Rgba).expect("room");
    }
    for f in 1..3 {
        doc.add_frame(f, false).expect("room");
    }
    let base = [
        0xff20_4060,
        0xff00_00ff,
        0xff00_ff00,
        0xffff_0000,
        0xff80_8080,
        0xffff_ffff,
    ];
    for (l, layer) in doc.layers_mut().iter_mut().enumerate() {
        layer.name = ["base", "shade", "sketch", "ink"][l].into();
        layer.visible = l != 2;
        layer.opacity = [255, 128, 255, 200][l];
        layer.blend = [
            BlendMode::Normal,
            BlendMode::Multiply,
            BlendMode::Normal,
            BlendMode::Normal,
        ][l];
    }
    for f in 0..3 {
        let cells = |l: usize, i: usize| match l {
            0 => base[(i + f) % 6],
            1 if i % 5 == 0 => 0xff40_c0c0,
            2 => 0xff12_3456,
            3 if i % 7 == f => 0x80ff_ffff,
            _ => 0,
        };
        for l in 0..4 {
            let px: Vec<u32> = (0..side * side).map(|i| cells(l, i)).collect();
            doc.set_cel(l, f, Grid::from_cells(side, side, &px))
                .expect("cel");
        }
    }
    doc.tags.push(Tag {
        name: "walk".into(),
        from: 1,
        to: 2,
        direction: Direction::Forward,
        repeat: 0,
        id: pikslar_core::TagId::fresh(),
    });
    doc
}

fn write_project(d: &Path, doc: &Document) -> PathBuf {
    let at = d.join("p.piks");
    fs::write(&at, pikslar_format::piks::save(doc)).expect("write project");
    at
}

fn run(project: &Path, out: &Path, extra: &[&str]) -> (i32, String) {
    let mut args: Vec<&dyn AsRef<std::ffi::OsStr>> = vec![&"export", &project, &"--out", &out];
    args.extend(extra.iter().map(|a| a as &dyn AsRef<std::ffi::OsStr>));
    let r = cli(&args);
    (r.code, r.err)
}

/// Colour type and bits a pixel from a PNG's header.
fn kind(bytes: &[u8]) -> (u8, u8) {
    (bytes[25], bytes[24])
}

/// The bits a pixel an indexed description gives, in order.
fn declared_depths(json: &str) -> Vec<u8> {
    json.split("\"bitDepth\":")
        .skip(1)
        .map(|rest| rest[..1].parse().expect("a digit"))
        .collect()
}

/// An indexed description with what it adds taken out, which leaves the RGBA one.
fn as_rgba(json: &str) -> String {
    let mut out = String::new();
    let mut rest = json;
    while let Some(at) = rest.find(",\"bitDepth\":") {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        let size = after.find("\"paletteSize\":").expect("a palette size");
        let digits = after[size + 14..]
            .find(|c: char| !c.is_ascii_digit())
            .expect("ends");
        rest = &after[size + 14 + digits..];
    }
    out.push_str(rest);
    out.replace("\"format\":\"INDEXED\"", "\"format\":\"RGBA8888\"")
}

#[test]
fn every_format_with_layers_and_scale_matches_its_rgba_export() {
    let d = folder("indexed-formats");
    let project = write_project(&d, &few_colours(12));
    let layer_choices: [&[&str]; 3] = [
        &[],
        &["--layer", "base"],
        &["--layer", "ink", "--layer", "sketch"],
    ];
    let mut compared = 0;
    for format in ["png", "sheet", "sequence"] {
        for scale in ["1", "3"] {
            for layers in layer_choices {
                let (rgba, coded) = (
                    d.join(format!("{format}-rgba")),
                    d.join(format!("{format}-ix")),
                );
                for dir in [&rgba, &coded] {
                    let _ = fs::remove_dir_all(dir);
                    fs::create_dir_all(dir).expect("dir");
                }
                let mut args = vec!["--format", format, "--scale", scale];
                args.extend_from_slice(layers);
                assert_eq!(run(&project, &rgba.join("a.png"), &args).0, 0);
                args.push("--indexed");
                let (code, err) = run(&project, &coded.join("a.png"), &args);
                assert_eq!(code, 0, "{format} {scale} {layers:?}: {err}");
                assert_eq!(listing(&rgba), listing(&coded));
                for name in listing(&rgba) {
                    let (a, b) = (
                        fs::read(rgba.join(&name)).expect("a"),
                        fs::read(coded.join(&name)).expect("b"),
                    );
                    if name.ends_with(".json") {
                        let pngs: Vec<Vec<u8>> = listing(&coded)
                            .iter()
                            .filter(|n| n.ends_with(".png"))
                            .map(|n| fs::read(coded.join(n)).expect("png"))
                            .collect();
                        let b = String::from_utf8(b).expect("utf-8");
                        assert!(b.contains("\"format\":\"INDEXED\""), "{b}");
                        let depths: Vec<u8> = pngs.iter().map(|p| kind(p).1).collect();
                        assert_eq!(declared_depths(&b), depths, "each image's own bits");
                        assert_eq!(as_rgba(&b).into_bytes(), a, "otherwise the same");
                        continue;
                    }
                    assert_eq!(kind(&a), (6, 8));
                    assert_eq!(kind(&b).0, 3, "{name} is indexed");
                    let (a, b) = (
                        png::decode(&a).expect("rgba"),
                        png::decode(&b).expect("indexed"),
                    );
                    assert!(a == b, "{format} {scale} {layers:?} {name}");
                    compared += 1;
                }
            }
        }
    }
    assert_eq!(
        compared,
        (1 + 1 + 3) * 2 * 3,
        "a png, a sheet and three members, each six ways"
    );
    fs::remove_dir_all(d).expect("cleanup");
}

fn snapshot(d: &Path) -> Vec<(String, Vec<u8>)> {
    listing(d)
        .into_iter()
        .map(|n| (n.clone(), fs::read(d.join(&n)).expect("read")))
        .collect()
}

#[test]
fn too_many_colours_is_refused_before_any_file_is_made_or_replaced() {
    let d = folder("indexed-refused");
    let mut doc = few_colours(20);
    let many: Vec<u32> = (0..400).map(|i| 0xff00_0000 | (i * 97)).collect();
    doc.set_cel(0, 2, Grid::from_cells(20, 20, &many))
        .expect("cel");
    let project = write_project(&d, &doc);
    let out = d.join("out.png");
    for format in ["png", "sheet", "sequence"] {
        assert_eq!(
            run(&project, &out, &["--format", format, "--overwrite"]).0,
            0
        );
    }
    let before = snapshot(&d);
    let lock = pikslar_format::set_lock::lock_path(&d.join("out.json"));
    let refusals: [(&[&str], &str); 4] = [
        (
            &["--format", "png", "--frame", "3"],
            "Frame 3 has more than 256 colours",
        ),
        (
            &["--format", "sequence"],
            "Frame 3 has more than 256 colours",
        ),
        (
            &["--format", "sheet"],
            "These frames together have more than 256 colours",
        ),
        (
            &["--format", "sheet", "--scale", "2", "--layer", "base"],
            "more than 256 colours",
        ),
    ];
    for (args, says) in refusals {
        let mut all = vec!["--overwrite", "--indexed"];
        all.extend_from_slice(args);
        let (code, err) = run(&project, &out, &all);
        assert_eq!(code, 3, "{args:?}: {err}");
        assert!(err.contains(says), "{err}");
        assert!(snapshot(&d) == before, "{args:?} changed the folder");
        assert!(!lock.exists(), "{args:?} left a lock");
    }
    let (code, err) = run(
        &project,
        &d.join("two.png"),
        &[
            "--overwrite",
            "--indexed",
            "--format",
            "sequence",
            "--frames",
            "1-2",
        ],
    );
    assert_eq!(code, 0, "the first two frames fit: {err}");
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn indexed_takes_no_value_and_is_only_an_option_of_export() {
    let d = folder("indexed-args");
    let project = write_project(&d, &few_colours(4));
    let out = d.join("o.png");
    for bad in [&["--indexed=yes"][..], &["--Indexed"]] {
        let (code, err) = run(&project, &out, bad);
        assert_eq!(code, 1, "{bad:?}: {err}");
    }
    assert!(!out.exists());
    let r = cli(&[&"info", &project, &"--indexed"]);
    assert_eq!(r.code, 1, "{}", r.err);
    let (code, _) = run(&project, &out, &["--indexed", "--indexed"]);
    assert_eq!(code, 0, "given twice, it is still asked for once");
    assert_eq!(kind(&fs::read(&out).expect("written")).0, 3);
    fs::remove_dir_all(d).expect("cleanup");
}
