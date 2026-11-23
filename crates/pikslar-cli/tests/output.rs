//! What the program writes: the same bytes as the app, every time, and nothing it should not.

mod common;
use common::{cli, corpus, folder, listing};
use pikslar_format::piks;
use std::fs;

fn load(name: &str) -> pikslar_core::Document {
    piks::load(&fs::read(corpus(name)).expect("read")).expect("loads")
}

#[test]
fn every_frame_is_the_bytes_the_app_exports() {
    let d = folder("frames");
    for name in [
        "v3-animated.piks",
        "v3-offsets.piks",
        "v1-palette.piks",
        "v3-noise.piks",
    ] {
        let doc = load(name);
        for frame in 0..doc.frames.len() {
            let out = d.join(format!("{name}-{frame}.png"));
            let n = (frame + 1).to_string();
            let run = cli(&[&"export", &corpus(name), &"--frame", &n, &"-o", &out]);
            assert_eq!(run.code, 0, "{}", run.err);
            assert_eq!(run.out.trim(), out.display().to_string());
            let app = d.join("app.png");
            pikslar_export::export_frame(&app, &doc, frame).expect("app export");
            assert_eq!(
                fs::read(&out).expect("cli"),
                fs::read(&app).expect("app"),
                "{name} {frame}"
            );
        }
    }
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn with_no_frame_chosen_a_png_is_the_first_frame() {
    let d = folder("first");
    let out = d.join("first.png");
    let run = cli(&[&"export", &corpus("v3-animated.piks"), &"-o", &out]);
    assert_eq!(run.code, 0, "{}", run.err);
    let app = d.join("app.png");
    pikslar_export::export_frame(&app, &load("v3-animated.piks"), 0).expect("app export");
    assert_eq!(fs::read(&out).expect("cli"), fs::read(&app).expect("app"));
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn a_sheet_is_the_bytes_the_app_exports() {
    let (cli_dir, app_dir) = (folder("sheet-cli"), folder("sheet-app"));
    let doc = load("v3-animated.piks");
    let out = cli_dir.join("walk.png");
    let run = cli(&[
        &"export",
        &corpus("v3-animated.piks"),
        &"--format",
        &"sheet",
        &"-o",
        &out,
    ]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.out.lines().count(), 2);
    assert!(pikslar_export::export_sheet(&app_dir.join("walk.png"), &doc).expect("app"));
    for file in ["walk.png", "walk.json"] {
        assert_eq!(
            fs::read(cli_dir.join(file)).expect("cli"),
            fs::read(app_dir.join(file)).expect("app"),
            "{file}"
        );
    }
    assert_eq!(listing(&cli_dir), ["walk.json", "walk.png"]);
    fs::remove_dir_all(cli_dir).expect("cleanup");
    fs::remove_dir_all(app_dir).expect("cleanup");
}

#[test]
fn a_tag_makes_a_sheet_of_its_frames() {
    let d = folder("tag");
    let out = d.join("idle.png");
    let run = cli(&[
        &"export",
        &corpus("v3-animated.piks"),
        &"--format",
        &"sheet",
        &"--tag",
        &"idle",
        &"-o",
        &out,
    ]);
    assert_eq!(run.code, 0, "{}", run.err);
    let text = fs::read_to_string(d.join("idle.json")).expect("json");
    assert!(
        text.contains("\"idle 3\"") && text.contains("\"idle 4\"") && !text.contains("\"idle 2\""),
        "{text}"
    );
    assert!(
        text.contains("\"name\":\"idle\",\"from\":0,\"to\":1"),
        "{text}"
    );
    let image = pikslar_format::png::decode(&fs::read(&out).expect("png")).expect("decodes");
    assert_eq!((image.width, image.height), (96, 48));
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn many_runs_in_a_row_give_the_same_bytes_and_leave_nothing_else() {
    let d = folder("batch");
    let p = corpus("v3-animated.piks");
    let first = d.join("first.png");
    assert_eq!(
        cli(&[&"export", &p, &"--format", &"sheet", &"-o", &first]).code,
        0
    );
    let expected = fs::read(&first).expect("first");
    let again = d.join("again.png");
    for round in 0..25 {
        let run = cli(&[
            &"export",
            &p,
            &"--format",
            &"sheet",
            &"-o",
            &again,
            &"--overwrite",
        ]);
        assert_eq!(run.code, 0, "round {round}: {}", run.err);
        assert_eq!(fs::read(&again).expect("again"), expected, "round {round}");
    }
    assert_eq!(
        listing(&d),
        ["again.json", "again.png", "first.json", "first.png"]
    );
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn info_describes_the_project_the_same_way_every_time() {
    let p = corpus("v3-animated.piks");
    let text = cli(&[&"info", &p]);
    assert_eq!(text.code, 0);
    assert!(
        text.out
            .starts_with("48 x 48, 3 layers, 5 frames, 0.640 s\n"),
        "{}",
        text.out
    );
    assert!(
        text.out.contains("idle: frames 4 to 5, pingpong"),
        "{}",
        text.out
    );
    let top = text.out.find("Layer 3:").expect("top layer");
    let bottom = text.out.find("Layer 1:").expect("bottom layer");
    assert!(top < bottom, "top layer first: {}", text.out);
    let json = cli(&[&"info", &p, &"--json"]);
    assert!(
        json.out.contains("\"durations\":[100,90,120,150,180]"),
        "{}",
        json.out
    );
    assert!(json.out.contains(
        "{\"name\":\"walk\",\"first\":1,\"last\":3,\"direction\":\"forward\",\"repeat\":0}"
    ));
    assert!(json.out.contains("\"fps\":null"), "{}", json.out);
    assert!(!text.out.contains("fps"), "{}", text.out);
    assert_eq!(json.out.lines().count(), 1);
    assert_eq!(cli(&[&"info", &p, &"--json"]).out, json.out);
}

#[test]
fn info_says_the_frame_rate_of_a_project_timed_by_rate() {
    let d = folder("info-rate");
    let mut doc = load("v3-animated.piks");
    doc.frame_rate = Some(24);
    let project = d.join("rate.piks");
    fs::write(&project, piks::save(&doc)).expect("write project");
    let text = cli(&[&"info", &project]);
    let first = text.out.lines().next().expect("a line");
    assert!(first.ends_with(" s, 24 fps"), "{first}");
    let json = cli(&[&"info", &project, &"--json"]).out;
    assert!(json.contains("\"fps\":24"), "{json}");
    assert_eq!(json.lines().count(), 1);
    fs::remove_dir_all(d).expect("cleanup");
}
