//! --format apng: frames, tags, loops, scale and layers against the PNG of each frame, cell for
//! cell; the options an APNG takes and refuses; and the file safety every export has.

#[path = "../../pikslar-format/src/apng_read.rs"]
mod apng_read;
mod common;
use apng_read::{played, png_cells, read};
use common::{cli, folder, listing};
use pikslar_core::{BlendMode, ColorMode, Direction, Document, Grid, Tag};
use std::fs;
use std::path::{Path, PathBuf};

/// Four frames, 12 by 9: a ground with a hole in frame 3, a multiply layer at partial opacity,
/// a hidden half-clear layer, and a dot that moves. Frames 2 to 4 are a ping-pong tag "walk".
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
        layer.opacity = [255, 160, 200][l];
    }
    for f in 0..4 {
        let ground: Vec<u32> = (0..w * h)
            .map(|i| match (f, i % w < 4 && i / w < 3) {
                (2, true) => 0,
                _ => 0xc020_4060 + (f as u32) * 0x10,
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

/// Frame `frame` (from 1) exactly as the PNG export with `extra` writes it.
fn png_frame(project: &Path, d: &Path, frame: usize, extra: &[&str]) -> Vec<u32> {
    let out = d.join("frame.png");
    let n = frame.to_string();
    let mut args = vec!["--overwrite", "--frame", n.as_str()];
    args.extend_from_slice(extra);
    assert_eq!(run(project, &out, &args).0, 0);
    png_cells(&fs::read(&out).expect("png")).1
}

/// Options, the frames (from 1) the APNG should show, its plays and its delays in ms.
type Case<'a> = (&'a [&'a str], &'a [usize], u32, &'a [u16]);

#[test]
fn an_apng_shows_each_frame_as_its_png_in_play_order_with_its_loop() {
    let d = folder("apng-frames");
    let project = write_project(&d, &animation());
    let cases: [Case; 5] = [
        (&[], &[1, 2, 3, 4], 0, &[100, 40, 250, 75]),
        (
            &["--tag", "walk", "--loop", "3"],
            &[2, 3, 4, 3],
            3,
            &[40, 250, 75, 250],
        ),
        (&["--frames", "3-4", "--loop", "1"], &[3, 4], 1, &[250, 75]),
        (&["--frame", "2", "--scale", "3"], &[2], 1, &[40]),
        (
            &["--layer", "ground", "--layer", "secret", "--scale", "2"],
            &[1, 2, 3, 4],
            0,
            &[100, 40, 250, 75],
        ),
    ];
    for (args, frames, plays, delays) in cases {
        let out = d.join("a.png");
        let mut all = vec!["--format", "apng", "--overwrite"];
        all.extend_from_slice(args);
        let (code, err) = run(&project, &out, &all);
        assert_eq!(code, 0, "{args:?}: {err}");
        let looks = &args[args
            .iter()
            .position(|a| *a == "--scale" || *a == "--layer")
            .unwrap_or(args.len())..];
        let expected: Vec<Vec<u32>> = frames
            .iter()
            .map(|&f| png_frame(&project, &d, f, looks))
            .collect();
        let r = read(&fs::read(&out).expect("apng"));
        assert_eq!(played(&r, 1), expected, "{args:?}");
        assert_eq!(r.plays, plays, "{args:?}");
        let got: Vec<u16> = r.frames.iter().map(|f| f.delay.0).collect();
        assert_eq!(got, delays, "{args:?}");
    }
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn options_an_apng_does_not_take_are_refused_before_anything_is_read() {
    let d = folder("apng-arguments");
    let project = write_project(&d, &animation());
    let bad: [&[&str]; 4] = [
        &["--format", "apng", "--indexed"],
        &["--format", "apng", "--loop", "65537"],
        &["--format", "apng", "--loop", "-1"],
        &["--format", "apng", "--loop", "1", "--loop", "2"],
    ];
    for args in bad {
        let (code, err) = run(&project, &d.join("a.png"), args);
        assert_eq!(code, 1, "{args:?}: {err}");
        assert!(
            err.contains("--loop") || err.contains("--indexed"),
            "{args:?}: {err}"
        );
    }
    let (code, err) = run(&project, &d.join("a.apng"), &["--format", "apng"]);
    assert_eq!(code, 1);
    assert!(err.contains("does not end in .png"), "{err}");
    assert_eq!(listing(&d), ["p.piks"], "nothing written");
    let r = cli(&[
        &"export",
        &project,
        &"--format",
        &"apng",
        &"--loop",
        &"65536",
    ]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(listing(&d), ["p.piks", "p.png"], "named after the project");
    assert_eq!(
        read(&fs::read(d.join("p.png")).expect("apng")).plays,
        65_536
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
fn an_apng_is_never_replaced_without_asking_and_a_refusal_changes_nothing() {
    let d = folder("apng-safety");
    let project = write_project(&d, &animation());
    let out = d.join("out.png");
    assert_eq!(run(&project, &out, &["--format", "apng"]).0, 0);
    let first = fs::read(&out).expect("apng");
    let before = snapshot(&d);
    let lock = pikslar_format::set_lock::lock_path(&out);
    let (code, err) = run(&project, &out, &["--format", "apng", "--frame", "2"]);
    assert_eq!(code, 4, "an existing file needs --overwrite: {err}");
    for args in [
        &["--frames", "2-9"][..],
        &["--tag", "run"],
        &["--layer", "none"],
    ] {
        let mut all = vec!["--format", "apng", "--overwrite"];
        all.extend_from_slice(args);
        let (code, err) = run(&project, &out, &all);
        assert_ne!(code, 0, "{args:?}: {err}");
        assert!(snapshot(&d) == before, "{args:?} changed the folder");
        assert!(!lock.exists(), "{args:?} left a lock");
    }
    assert_eq!(
        run(&project, &out, &["--format", "apng", "--overwrite"]).0,
        0
    );
    assert_eq!(fs::read(&out).expect("apng"), first, "the same bytes again");
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn frames_of_ten_milliseconds_are_kept_and_said_once() {
    let d = folder("apng-short");
    let mut doc = animation();
    for (f, ms) in [10, 11, 30, 10].into_iter().enumerate() {
        doc.frames[f].duration_ms = ms;
    }
    let project = write_project(&d, &doc);
    let out = d.join("a.png");
    let (code, err) = run(&project, &out, &["--format", "apng"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        err.trim(),
        "pikslar-cli: note: 2 frame(s) last 10 ms. The APNG keeps 10 ms, but some browsers \
         show frames that short as 100 ms"
    );
    let r = read(&fs::read(&out).expect("apng"));
    let delays: Vec<(u16, u16)> = r.frames.iter().map(|f| f.delay).collect();
    assert_eq!(delays, [(10, 1000), (11, 1000), (30, 1000), (10, 1000)]);
    let still = ["--format", "apng", "--frame", "1", "--overwrite"];
    assert_eq!(
        run(&project, &out, &still),
        (0, String::new()),
        "a still image plays once"
    );
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn a_tag_plays_as_many_times_as_it_is_set_to_unless_a_loop_is_asked() {
    let d = folder("apng-repeat");
    let mut doc = animation();
    doc.tags[0].repeat = 2;
    let project = write_project(&d, &doc);
    let info = cli(&[&"info", &project]).out;
    assert!(
        info.contains("walk: frames 2 to 4, pingpong, plays 2 times"),
        "{info}"
    );
    let json = cli(&[&"info", &project, &"--json"]).out;
    assert!(
        json.contains("\"direction\":\"pingpong\",\"repeat\":2}"),
        "{json}"
    );
    let out = d.join("a.png");
    let walk = ["--format", "apng", "--overwrite", "--tag", "walk"];
    assert_eq!(run(&project, &out, &walk).0, 0);
    let r = read(&fs::read(&out).expect("apng"));
    let delays: Vec<u16> = r.frames.iter().map(|f| f.delay.0).collect();
    assert_eq!(
        (r.plays, delays),
        (1, vec![40, 250, 75, 250, 40]),
        "there and back"
    );
    let forever = [&walk[..], &["--loop", "0"]].concat();
    assert_eq!(run(&project, &out, &forever).0, 0);
    let r = read(&fs::read(&out).expect("apng"));
    assert_eq!((r.plays, r.frames.len()), (0, 4));
    fs::remove_dir_all(d).expect("cleanup");
}
