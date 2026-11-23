//! `export --format tmj`: a Tiled map and its atlases, as the library plans them.
//!
//! `write_interop_fixtures` (ignored) writes the projects `spikes/check-tmj.py` exports and
//! checks with Pillow and Tiled's tmxrasterizer: run it with `PIKSLAR_TMJ_FIXTURES` set to a
//! folder.

mod common;
use common::{cli, folder, listing};
use pikslar_core::orient::Orientation;
use pikslar_core::tilemap::MapCell;
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::{rgba, BlendMode, Cel, ColorMode, Document, Grid, LayerKind};
use pikslar_export::tmj_json::map_text;
use pikslar_export::{tmj_map, tmj_outputs, write_outputs};
use pikslar_format::piks;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A tile of `side` whose pixels differ, opaque or clear, or with some partly clear when `soft`.
fn tile(side: (usize, usize), seed: u32, soft: bool) -> Arc<pikslar_core::Image> {
    let px: Vec<u32> = (0..(side.0 * side.1) as u32)
        .map(|i| match (i * 7 + seed * 3) % 13 {
            0 => 0,
            1 if soft => rgba((i * 9) as u8, 200, (seed * 50) as u8, 100),
            v => rgba(
                (seed * 60 + i * 3) as u8,
                (v * 19) as u8,
                (i * 11) as u8,
                255,
            ),
        })
        .collect();
    Arc::new(Grid::from_cells(side.0, side.1, &px).into_image())
}

fn tileset(doc: &mut Document, name: &str, side: (usize, usize), count: u32) -> TilesetId {
    let set = doc.add_tileset(name, ColorMode::Rgba, side).expect("room");
    for n in 0..count {
        doc.add_tile(set, tile(side, n + 1, false)).expect("fits");
    }
    set
}

fn cell(n: u32, o: Orientation) -> u32 {
    MapCell::place(TileId::new(n).expect("an id"), o).bits()
}

/// Adds a tilemap layer at the top whose frame `frame` places `cells`, `cw` a row, at `at`.
fn map_layer(
    doc: &mut Document,
    set: TilesetId,
    (frame, at): (usize, (i32, i32)),
    cw: usize,
    cells: &[u32],
) -> usize {
    let index = doc.layers().len();
    if doc.layers()[index - 1].tileset() != Some(set) || frame == 0 {
        doc.add_tilemap_layer(index, set).expect("room");
    }
    let index = doc.layers().len() - 1;
    let cel = Cel::new(Grid::from_cells(cw, cells.len() / cw, cells));
    doc.put_cel(index, frame, cel.moved_to(at.0, at.1).expect("in range"))
        .expect("fits");
    index
}

/// A map of every orientation on two tilesets, over two frames, with a paint layer.
fn two_frames() -> Document {
    let mut doc = Document::new(20, 12, ColorMode::Rgba).expect("size");
    doc.add_frame(1, false).expect("room");
    let a = tileset(&mut doc, "Ground", (4, 4), 3);
    let b = tileset(&mut doc, "Things \"B\"", (4, 4), 2);
    let all: Vec<u32> = Orientation::ALL.iter().map(|&o| cell(2, o)).collect();
    map_layer(&mut doc, a, (0, (0, 0)), 4, &all);
    map_layer(
        &mut doc,
        a,
        (1, (2, 1)),
        2,
        &[cell(1, Orientation::IDENTITY), 0],
    );
    map_layer(
        &mut doc,
        b,
        (0, (4, 4)),
        1,
        &[cell(2, Orientation::rotate90())],
    );
    doc
}

fn save(doc: &Document, at: &Path) -> PathBuf {
    fs::write(at, piks::save(doc)).expect("write project");
    at.to_path_buf()
}

#[test]
fn a_map_and_its_atlases_are_the_bytes_the_library_plans() {
    let d = folder("tmj-bytes");
    let doc = two_frames();
    let project = save(&doc, &d.join("level.piks"));
    let run = cli(&[&"export", &project, &"--format", &"tmj", &"--frame", &"2"]);
    assert_eq!(run.code, 0, "{}", run.err);
    let names = ["level-tileset1.png", "level-tileset2.png", "level.tmj"];
    let printed: Vec<String> = names
        .iter()
        .map(|n| d.join(n).display().to_string())
        .collect();
    assert_eq!(run.out.lines().collect::<Vec<_>>(), printed);
    assert!(
        run.err
            .contains("note: A TMJ holds tilemap layers only: 1 paint layer(s)"),
        "{}",
        run.err
    );
    let loaded = piks::load(&fs::read(&project).expect("read")).expect("loads");
    let map = tmj_map(&loaded, 1, &d.join("level.tmj")).expect("a map");
    assert_eq!(
        fs::read_to_string(d.join("level.tmj")).expect("map"),
        map_text(&map)
    );
    let other = folder("tmj-bytes-library");
    let mut planned = tmj_outputs(&loaded, 1, other.join("level.tmj")).expect("planned");
    write_outputs(&mut planned.outputs, &mut |_, _| Ok(())).expect("written");
    for name in names {
        assert_eq!(
            fs::read(d.join(name)).expect(name),
            fs::read(other.join(name)).expect(name),
            "{name}"
        );
    }
    check_level_text(&fs::read_to_string(d.join("level.tmj")).expect("map"));
    fs::remove_dir_all(d).expect("cleanup");
    fs::remove_dir_all(other).expect("cleanup");
}

/// The map of `two_frames` in frame 2: names escaped, atlases named, cells moved by the offset.
fn check_level_text(text: &str) {
    assert!(text.contains("\"name\":\"Things \\\"B\\\"\""), "{text}");
    assert!(text.contains("\"image\":\"level-tileset2.png\""));
    let first = text.find("\"data\":[").expect("data") + "\"data\":[".len();
    let rows: Vec<&str> = text[first..]
        .lines()
        .skip(1)
        .take(2)
        .map(str::trim)
        .collect();
    assert_eq!(
        rows,
        ["0,0,0,0,0,0,", "0,1,0,0,0,0,"],
        "frame 2, one tile in"
    );
}

#[test]
fn files_there_are_kept_without_overwrite_and_nothing_is_half_written() {
    let d = folder("tmj-overwrite");
    let project = save(&two_frames(), &d.join("p.piks"));
    let out = d.join("m.tmj");
    fs::write(d.join("m-tileset2.png"), "theirs").expect("write");
    let run = cli(&[&"export", &project, &"--format", &"tmj", &"-o", &out]);
    assert_eq!(run.code, 4, "{}", run.err);
    assert!(
        run.err.contains("m-tileset2.png already exists"),
        "{}",
        run.err
    );
    assert_eq!(listing(&d), ["m-tileset2.png", "p.piks"]);
    let run = cli(&[
        &"export",
        &project,
        &"--format",
        &"tmj",
        &"-o",
        &out,
        &"--overwrite",
    ]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        listing(&d),
        ["m-tileset1.png", "m-tileset2.png", "m.tmj", "p.piks"]
    );
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn projects_that_cannot_be_a_map_are_refused_before_anything_is_written() {
    let d = folder("tmj-refused-projects");
    let plain = save(
        &Document::new(4, 4, ColorMode::Rgba).expect("size"),
        &d.join("plain.piks"),
    );
    let mut mixed = two_frames();
    let tall = tileset(&mut mixed, "Tall", (4, 8), 1);
    map_layer(&mut mixed, tall, (0, (0, 0)), 1, &[0]);
    let mixed = save(&mixed, &d.join("mixed.piks"));
    let run = cli(&[&"export", &plain, &"--format", &"tmj"]);
    assert_eq!(run.code, 3);
    assert!(run.err.contains("no tilemap layers"), "{}", run.err);
    let run = cli(&[&"export", &mixed, &"--format", &"tmj"]);
    assert_eq!(run.code, 3);
    assert!(
        run.err.contains("A Tiled map has one tile size"),
        "{}",
        run.err
    );
    assert_eq!(listing(&d), ["mixed.piks", "plain.piks"]);
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn options_a_map_does_not_have_are_refused_before_anything_is_written() {
    let d = folder("tmj-refused");
    let project = save(&two_frames(), &d.join("p.piks"));
    let png = d.join("m.png");
    let cases: [(Vec<&dyn AsRef<std::ffi::OsStr>>, i32, &str); 8] = [
        (
            vec![&"--layer", &"Layer 1"],
            1,
            "--layer is not for --format tmj",
        ),
        (vec![&"--scale", &"2"], 1, "--scale is not for --format tmj"),
        (vec![&"--indexed"], 1, "--indexed is not for --format tmj"),
        (vec![&"--frames", &"1-2"], 1, "A TMJ holds one frame"),
        (vec![&"-o", &png], 1, "does not end in .tmj"),
        (
            vec![&"--frame", &"3"],
            3,
            "There is no frame 3: the project has 2",
        ),
        (
            vec![&"--columns", &"2"],
            1,
            "--columns is for --format sheet",
        ),
        (
            vec![&"--loop", &"2"],
            1,
            "--loop is for --format gif and apng",
        ),
    ];
    for (extra, code, said) in cases {
        let mut args: Vec<&dyn AsRef<std::ffi::OsStr>> =
            vec![&"export", &project, &"--format", &"tmj"];
        args.extend(extra);
        let run = cli(&args);
        assert_eq!(run.code, code, "{said}: {}", run.err);
        assert!(run.err.contains(said), "{said}: {}", run.err);
    }
    assert_eq!(listing(&d), ["p.piks"]);
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn the_help_names_the_format() {
    let run = cli(&[&"help", &"export"]);
    assert!(run.out.contains("png|sheet|sequence|gif|apng|tmj"));
    assert!(run.out.contains("A TMJ is a Tiled map"));
}

/// The projects of the interop check, by name. Each hides its paint layer, so the PNG export
/// draws exactly the shown tilemap layers.
fn interop_cases() -> Vec<(&'static str, Document)> {
    let mut cases = vec![
        ("orientations", case_orientations()),
        ("offsets", case_offsets()),
        ("groups-hidden", case_groups_hidden()),
        ("opacity", case_opacity()),
        ("multiply", case_multiply()),
        ("palette", case_palette()),
    ];
    for (_, doc) in &mut cases {
        doc.layers_mut()[0].visible = false;
    }
    cases
}

/// The "orientations" interop project.
fn case_orientations() -> Document {
    let mut doc = Document::new(24, 16, ColorMode::Rgba).expect("size");
    let set = tileset(&mut doc, "Eight", (4, 4), 4);
    let all: Vec<u32> = Orientation::ALL.iter().map(|&o| cell(3, o)).collect();
    map_layer(&mut doc, set, (0, (0, 0)), 4, &all);
    let mixed: Vec<u32> = (0..24)
        .map(|i| cell(1 + i % 4, Orientation::ALL[(i as usize * 5) % 8]))
        .collect();
    map_layer(&mut doc, set, (0, (0, 8)), 6, &mixed);
    doc
}

/// The "offsets" interop project.
fn case_offsets() -> Document {
    let mut doc = Document::new(22, 19, ColorMode::Rgba).expect("size");
    let set = tileset(&mut doc, "Off", (5, 5), 3);
    let block: Vec<u32> = (0..12)
        .map(|i| {
            if i % 5 == 4 {
                0
            } else {
                cell(1 + i % 3, Orientation::ALL[i as usize % 8])
            }
        })
        .collect();
    map_layer(&mut doc, set, (0, (7, -3)), 4, &block);
    map_layer(&mut doc, set, (0, (-6, 11)), 3, &block[..9]);
    map_layer(&mut doc, set, (0, (10, 5)), 2, &block[..4]);
    doc
}

/// The "groups-hidden" interop project.
fn case_groups_hidden() -> Document {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    let set = tileset(&mut doc, "Grouped", (4, 4), 2);
    let fill: Vec<u32> = (0..16)
        .map(|i| cell(1 + i % 2, Orientation::IDENTITY))
        .collect();
    map_layer(&mut doc, set, (0, (0, 0)), 4, &fill);
    map_layer(&mut doc, set, (0, (4, 4)), 2, &fill[..4]);
    map_layer(&mut doc, set, (0, (8, 0)), 2, &fill[..4]);
    doc.add_layer(doc.layers().len(), ColorMode::Rgba)
        .expect("room");
    map_layer(&mut doc, set, (0, (0, 8)), 2, &fill[..4]);
    doc.add_layer(doc.layers().len(), ColorMode::Rgba)
        .expect("room");
    {
        let layers = doc.layers_mut();
        layers[2].depth = 1;
        layers[3].depth = 1;
        layers[3].visible = false;
        layers[4].kind = LayerKind::Group;
        layers[5].depth = 1;
        layers[6].kind = LayerKind::Group;
        layers[6].visible = false;
    }
    doc
}

/// The "opacity" interop project.
fn case_opacity() -> Document {
    let mut doc = Document::new(12, 12, ColorMode::Rgba).expect("size");
    let set = doc
        .add_tileset("Soft", ColorMode::Rgba, (4, 4))
        .expect("room");
    for n in 0..3 {
        doc.add_tile(set, tile((4, 4), n + 1, true)).expect("fits");
    }
    let fill: Vec<u32> = (0..9)
        .map(|i| cell(1 + i % 3, Orientation::IDENTITY))
        .collect();
    map_layer(&mut doc, set, (0, (0, 0)), 3, &fill);
    let top = map_layer(&mut doc, set, (0, (2, 2)), 2, &fill[..4]);
    doc.layers_mut()[top].opacity = 160;
    let next = map_layer(&mut doc, set, (0, (4, 0)), 2, &fill[..4]);
    doc.layers_mut()[next].opacity = 64;
    doc
}

/// The "multiply" interop project.
fn case_multiply() -> Document {
    let mut doc = Document::new(12, 8, ColorMode::Rgba).expect("size");
    let set = tileset(&mut doc, "Blend", (4, 4), 3);
    let fill: Vec<u32> = (0..6)
        .map(|i| cell(1 + i % 3, Orientation::IDENTITY))
        .collect();
    map_layer(&mut doc, set, (0, (0, 0)), 3, &fill);
    let top = map_layer(&mut doc, set, (0, (4, 0)), 2, &fill[..4]);
    doc.layers_mut()[top].blend = BlendMode::Multiply;
    doc
}

/// The "palette" interop project.
fn case_palette() -> Document {
    let mut doc = Document::new(10, 10, ColorMode::Palette).expect("size");
    let colors = [
        rgba(250, 20, 20, 255),
        rgba(20, 250, 20, 255),
        rgba(20, 20, 250, 255),
    ];
    let ids: Vec<u32> = colors
        .iter()
        .map(|&c| Arc::make_mut(&mut doc.colors).id_for(c).expect("room"))
        .collect();
    let set = doc
        .add_tileset("Indexed", ColorMode::Palette, (3, 3))
        .expect("room");
    for n in 0..3usize {
        let px: Vec<u32> = (0..9)
            .map(|i| if i % 4 == 0 { 0 } else { ids[(n + i) % 3] })
            .collect();
        doc.add_tile(set, Arc::new(Grid::from_cells(3, 3, &px).into_image()))
            .expect("fits");
    }
    let fill: Vec<u32> = (0..16)
        .map(|i| cell(1 + i % 3, Orientation::ALL[i as usize % 8]))
        .collect();
    map_layer(&mut doc, set, (0, (-1, 0)), 4, &fill);
    doc
}

#[test]
fn the_interop_projects_are_sound_maps() {
    for (name, doc) in interop_cases() {
        doc.check_groups().expect(name);
        let map = tmj_map(&doc, 0, Path::new("x.tmj")).expect(name);
        assert!(!map.tilesets.is_empty(), "{name}");
    }
}

#[test]
#[ignore = "writes the interop projects for spikes/check-tmj.py"]
fn write_interop_fixtures() {
    let Some(dir) = std::env::var_os("PIKSLAR_TMJ_FIXTURES") else {
        panic!("set PIKSLAR_TMJ_FIXTURES to a folder");
    };
    let dir = PathBuf::from(dir);
    fs::create_dir_all(&dir).expect("folder");
    for (name, doc) in interop_cases() {
        save(&doc, &dir.join(format!("{name}.piks")));
    }
}
