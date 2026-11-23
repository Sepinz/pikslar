//! --format gif: frames, tags, loops, scale and layers against the PNG of each frame; the
//! options only a GIF has; and what is refused, before any file is made or replaced.

mod common;
#[path = "../../pikslar-format/src/gif_read.rs"]
mod gif_read;
use common::{cli, folder, listing};
use gif_read::{read, shown};
use pikslar_core::{BlendMode, ColorMode, Direction, Document, Grid, Tag};
use pikslar_format::png;
use std::fs;
use std::path::{Path, PathBuf};

/// Four frames, 12 by 9: an opaque ground with a hole in frame 3, a multiply layer over it, a
/// hidden layer, and a dot that moves. Frames 2 to 4 are a ping-pong tag "walk".
fn animation() -> Document {
    let (w, h) = (12, 9);
    let mut doc = Document::new(w, h, ColorMode::Rgba).expect("size");
    for at in 1..3 {
        doc.add_layer(at, ColorMode::Rgba).expect("room");
    }
    for f in 1..4 {
        doc.add_frame(f, false).expect("room");
    }
    for (l, layer) in doc.layers_mut().iter_mut().enumerate() {
        layer.name = ["ground", "shade", "secret"][l].into();
        layer.visible = l != 2;
        layer.blend = [BlendMode::Normal, BlendMode::Multiply, BlendMode::Normal][l];
        layer.opacity = [255, 160, 255][l];
    }
    for f in 0..4 {
        let ground: Vec<u32> = (0..w * h)
            .map(|i| match (f, i % w < 4 && i / w < 3) {
                (2, true) => 0,
                _ => 0xff20_4060 + (f as u32) * 0x10,
            })
            .collect();
        let shade: Vec<u32> = (0..w * h)
            .map(|i| if i == f * 13 + 5 { 0xff80_ff40 } else { 0 })
            .collect();
        doc.set_cel(0, f, Grid::from_cells(w, h, &ground))
            .expect("cel");
        doc.set_cel(1, f, Grid::from_cells(w, h, &shade))
            .expect("cel");
        doc.set_cel(2, f, Grid::from_cells(w, h, &vec![0x8012_3456; w * h]))
            .expect("cel");
        doc.frames[f].duration_ms = [100, 40, 250, 75][f];
    }
    doc.tags.push(Tag {
        name: "walk".into(),
        from: 1,
        to: 3,
        direction: Direction::PingPong,
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

/// Frame `frame` (from 1) as a PNG export with `extra`, a clear pixel as (0, 0, 0, 0).
fn png_frame(project: &Path, d: &Path, frame: usize, extra: &[&str]) -> Vec<u32> {
    let out = d.join("frame.png");
    let n = frame.to_string();
    let mut args = vec!["--overwrite", "--frame", n.as_str()];
    args.extend_from_slice(extra);
    assert_eq!(run(project, &out, &args).0, 0);
    let px = png::decode(&fs::read(&out).expect("png"))
        .expect("decodes")
        .px;
    px.into_iter()
        .map(|c| if c >> 24 == 0 { 0 } else { c })
        .collect()
}

/// Options, the frames (from 1) the GIF should show, its loop's repeats and its delays.
type Case<'a> = (&'a [&'a str], &'a [usize], Option<u16>, &'a [u16]);

#[test]
fn a_gif_shows_each_frame_as_its_png_in_play_order_with_its_loop() {
    let d = folder("gif-frames");
    let project = write_project(&d, &animation());
    let cases: [Case; 5] = [
        (&[], &[1, 2, 3, 4], Some(0), &[10, 4, 25, 8]),
        (
            &["--tag", "walk", "--loop", "3"],
            &[2, 3, 4, 3],
            Some(2),
            &[4, 25, 8, 25],
        ),
        (&["--frames", "3-4", "--loop", "1"], &[3, 4], None, &[25, 8]),
        (&["--frame", "2", "--scale", "3"], &[2], None, &[0]),
        (
            &["--layer", "ground", "--layer", "secret", "--scale", "2"],
            &[1, 2, 3, 4],
            Some(0),
            &[10, 4, 25, 8],
        ),
    ];
    for (args, frames, repeats, delays) in cases {
        let out = d.join("a.gif");
        let mut all = vec!["--format", "gif", "--overwrite"];
        all.extend_from_slice(args);
        let (code, err) = run(&project, &out, &all);
        let looks = &args[args
            .iter()
            .position(|a| *a == "--scale" || *a == "--layer")
            .unwrap_or(args.len())..];
        let expected: Vec<Vec<u32>> = frames
            .iter()
            .map(|&f| png_frame(&project, &d, f, looks))
            .collect();
        if looks.contains(&"secret") {
            assert_eq!(code, 3, "{args:?}: the hidden layer is translucent: {err}");
            continue;
        }
        assert_eq!(code, 0, "{args:?}: {err}");
        let r = read(&fs::read(&out).expect("gif"));
        assert_eq!(shown(&r), expected, "{args:?}");
        assert_eq!(r.repeats, repeats, "{args:?}");
        let got: Vec<u16> = r.frames.iter().map(|f| f.delay).collect();
        assert_eq!(got, delays, "{args:?}");
    }
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn options_only_a_gif_has_are_checked_before_anything_is_read() {
    let d = folder("gif-arguments");
    let project = write_project(&d, &animation());
    let bad: [&[&str]; 7] = [
        &["--format", "png", "--loop", "2"],
        &["--format", "sheet", "--loop", "0"],
        &["--format", "gif", "--indexed"],
        &["--format", "gif", "--loop", "65537"],
        &["--format", "gif", "--loop", "-1"],
        &["--format", "gif", "--loop", "1", "--loop", "2"],
        &["--format", "gif", "--loop"],
    ];
    for args in bad {
        let name = if args[1] == "gif" { "a.gif" } else { "a.png" };
        let (code, err) = run(&project, &d.join(name), args);
        assert_eq!(code, 1, "{args:?}: {err}");
        assert!(
            err.contains("--loop") || err.contains("--indexed"),
            "{args:?}: {err}"
        );
    }
    assert_eq!(listing(&d), ["p.piks"], "nothing written");
    let (code, err) = run(&project, &d.join("a.png"), &["--format", "gif"]);
    assert_eq!(code, 1);
    assert!(err.contains("does not end in .gif"), "{err}");
    let r = cli(&[
        &"export",
        &project,
        &"--format",
        &"gif",
        &"--loop",
        &"65536",
    ]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(listing(&d), ["p.gif", "p.piks"], "named after the project");
    let r = read(&fs::read(d.join("p.gif")).expect("gif"));
    assert_eq!(r.repeats, Some(u16::MAX));
    fs::remove_dir_all(d).expect("cleanup");
}

fn snapshot(d: &Path) -> Vec<(String, Vec<u8>)> {
    listing(d)
        .into_iter()
        .map(|n| (n.clone(), fs::read(d.join(&n)).expect("read")))
        .collect()
}

#[test]
fn a_gif_that_cannot_be_exact_is_refused_and_changes_nothing() {
    let d = folder("gif-refused");
    let mut doc = animation();
    let out = d.join("out.gif");
    let project = write_project(&d, &doc);
    assert_eq!(run(&project, &out, &["--format", "gif"]).0, 0);
    let many: Vec<u32> = (0..108).map(|i| 0xff00_0000 | (i * 97)).collect();
    doc.set_cel(0, 3, Grid::from_cells(12, 9, &many))
        .expect("cel");
    let other: Vec<u32> = (0..108).map(|i| 0xff10_0000 | (i * 89)).collect();
    doc.set_cel(0, 1, Grid::from_cells(12, 9, &other))
        .expect("cel");
    let third: Vec<u32> = (0..108).map(|i| 0xff20_0000 | (i * 83)).collect();
    doc.set_cel(0, 2, Grid::from_cells(12, 9, &third))
        .expect("cel");
    let mut tinted = vec![0xff00_ff00u32; 108];
    tinted[40] = 0x40ff_ffff;
    doc.set_cel(0, 0, Grid::from_cells(12, 9, &tinted))
        .expect("cel");
    doc.layers_mut()[1].visible = false;
    let project = write_project(&d, &doc);
    let before = snapshot(&d);
    let lock = pikslar_format::set_lock::lock_path(&out);
    let refusals: [(&[&str], &str); 3] = [
        (
            &["--frame", "1"],
            "Frame 1 has a partly clear pixel at x 4, y 3",
        ),
        (&["--frames", "2-4"], "more than 256 colours together"),
        (
            &["--frames", "2-4", "--scale", "2"],
            "A GIF has one palette for all its frames",
        ),
    ];
    for (args, says) in refusals {
        let mut all = vec!["--format", "gif", "--overwrite"];
        all.extend_from_slice(args);
        let (code, err) = run(&project, &out, &all);
        assert_eq!(code, 3, "{args:?}: {err}");
        assert!(err.contains(says), "{err}");
        assert!(snapshot(&d) == before, "{args:?} changed the folder");
        assert!(!lock.exists(), "{args:?} left a lock");
    }
    let (code, err) = run(&project, &out, &["--format", "gif", "--frame", "2"]);
    assert_eq!(code, 4, "an existing GIF needs --overwrite: {err}");
    assert!(snapshot(&d) == before);
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn frames_too_short_for_a_gif_are_lengthened_and_said_once() {
    let d = folder("gif-short");
    let mut doc = animation();
    for (f, ms) in [10, 12, 30, 10].into_iter().enumerate() {
        doc.frames[f].duration_ms = ms;
    }
    let project = write_project(&d, &doc);
    let out = d.join("a.gif");
    let (code, err) = run(&project, &out, &["--format", "gif"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        err.trim(),
        "pikslar-cli: note: 3 frame(s) would round to less than 20 ms, which most players \
         show as 100 ms, so in the GIF they play for 20 ms"
    );
    let r = read(&fs::read(&out).expect("gif"));
    let delays: Vec<u16> = r.frames.iter().map(|f| f.delay).collect();
    assert_eq!(delays, [2, 2, 3, 2]);
    let (code, err) = run(
        &project,
        &out,
        &["--format", "gif", "--frame", "1", "--overwrite"],
    );
    assert_eq!(
        (code, err.as_str()),
        (0, ""),
        "a still image has no delay to lengthen"
    );
    fs::remove_dir_all(d).expect("cleanup");
}
