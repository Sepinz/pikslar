//! How the M6 corpus files were made, kept so their history is plain. It only runs when asked,
//! and writes to the folder in `PIKSLAR_CORPUS_OUT`, never into the corpus: frozen files are
//! never regenerated.
//!
//! `PIKSLAR_CORPUS_OUT=<dir> cargo test -p pikslar-safety --test corpus_writer -- --ignored`

use pikslar_core::command::CanvasCmd;
use pikslar_core::{
    rgba, BlendMode, ColorMode, Document, Editor, FrameCmd, Grid, LayerCmd, LayerId, Slice,
    SliceCmd, SliceRect,
};
use pikslar_format::piks;
use std::path::PathBuf;
use std::sync::Arc;

/// A drawing that differs by layer and frame: a band, a dot and a corner.
fn drawn(side: usize, layer: usize, frame: usize) -> Grid {
    let mut cells = vec![0u32; side * side];
    let shade = (layer * 29 + frame * 41) as u8;
    for y in 0..side {
        for x in 0..side {
            let band = (x + layer * 3 + frame) % 11 < 4;
            let dot = x == (layer * 5 + frame * 3) % side && y == (layer * 7) % side;
            if band || dot || (x < 3 && y < 3 + layer) {
                cells[y * side + x] = rgba(shade, 200 - shade / 2, 40 + shade / 3, 160 + shade / 3);
            }
        }
    }
    Grid::from_cells(side, side, &cells)
}

/// `layers` drawn layers over `frames` frames, `side` square.
fn drawn_document(side: usize, layers: usize, frames: usize) -> Document {
    let mut doc = Document::new(side, side, ColorMode::Rgba).expect("valid");
    for at in 1..layers {
        doc.add_layer(at, ColorMode::Rgba).expect("room");
    }
    for at in 1..frames {
        doc.add_frame(at, false).expect("room");
    }
    for layer in 0..layers {
        for frame in 0..frames {
            doc.set_cel(layer, frame, drawn(side, layer, frame))
                .expect("fits");
        }
    }
    doc
}

fn run(e: &mut Editor, command: LayerCmd) {
    e.apply(command).expect("done");
}

/// Runs the command made for the layer at `index`.
fn on(e: &mut Editor, index: usize, command: impl FnOnce(LayerId) -> LayerCmd) {
    let layer = e.layer_id(index).expect("a layer there");
    run(e, command(layer));
}

/// Every blend mode, lock, opacity, a rename, a duplicate and a merge, on a canvas made wider.
/// The merge is of the two plain layers on top, since a merged layer is always Normal.
fn layers() -> Editor {
    let mut e = Editor::new(drawn_document(32, 11, 2));
    for (index, blend) in BlendMode::ALL.into_iter().enumerate() {
        on(&mut e, index, |layer| LayerCmd::SetBlend { layer, blend });
        let opacity = 255 - index as u8 * 12;
        on(&mut e, index, |layer| LayerCmd::SetOpacity {
            layer,
            opacity,
        });
    }
    let name = "Ombre ü 影".to_string();
    on(&mut e, 4, |layer| LayerCmd::Rename { layer, name });
    on(&mut e, 2, |layer| LayerCmd::Duplicate { layer });
    on(&mut e, 11, |layer| LayerCmd::MergeDown { layer });
    on(&mut e, 5, |layer| LayerCmd::SetLocked {
        layer,
        locked: true,
    });
    let resize = CanvasCmd::Resize {
        width: 40,
        height: 36,
        dx: 6,
        dy: -2,
    };
    e.apply(resize).expect("resized");
    e
}

/// A picture placed as a reference over three frames, part of it past the canvas.
fn reference_picture() -> Arc<pikslar_core::Image> {
    let mut cells = vec![0u32; 40 * 24];
    for (i, cell) in cells.iter_mut().enumerate() {
        if i % 7 < 3 {
            *cell = rgba(30, (i % 251) as u8, 220, 255);
        }
    }
    Arc::new(Grid::from_cells(40, 24, &cells).into_image())
}

fn place_reference(e: &mut Editor, below: usize) {
    let image = reference_picture();
    let name = "Sketch".to_string();
    run(e, LayerCmd::PlaceReference { below, image, name });
}

fn reference() -> Editor {
    let mut e = Editor::new(drawn_document(32, 2, 3));
    place_reference(&mut e, 0);
    e
}

/// Groups two deep, one folded, one hidden, one locked, and a reference inside a group.
fn groups() -> Editor {
    let mut e = Editor::new(drawn_document(24, 6, 2));
    on(&mut e, 1, |layer| LayerCmd::Group { layer });
    // Layer 2 steps down into the new group at 2, then the group is grouped again.
    on(&mut e, 3, |layer| LayerCmd::Step { layer, up: false });
    on(&mut e, 3, |layer| LayerCmd::Group { layer });
    let outer = e.layer;
    on(&mut e, outer, |layer| LayerCmd::SetLocked {
        layer,
        locked: true,
    });
    on(&mut e, outer + 2, |layer| LayerCmd::Group { layer });
    let hidden = e.layer;
    on(&mut e, hidden, |layer| LayerCmd::SetVisible {
        layer,
        visible: false,
    });
    on(&mut e, hidden + 1, |layer| LayerCmd::Group { layer });
    let folded = e.layer;
    on(&mut e, folded, |layer| LayerCmd::SetFolded {
        layer,
        folded: true,
    });
    // Placed above the layer in the group, so it goes in the group too.
    place_reference(&mut e, folded - 1);
    e.doc().check_groups().expect("a valid list");
    e
}

#[test]
#[ignore = "writes corpus candidates; run by hand"]
fn write_the_m6_corpus_candidates() {
    let Some(out) = std::env::var_os("PIKSLAR_CORPUS_OUT").map(PathBuf::from) else {
        panic!("set PIKSLAR_CORPUS_OUT to an empty folder");
    };
    let files = [
        ("v3-layers.piks", layers()),
        ("v3-reference.piks", reference()),
        ("v3-groups.piks", groups()),
    ];
    for (name, editor) in files {
        let path = out.join(name);
        assert!(!path.exists(), "{} exists", path.display());
        std::fs::write(&path, piks::save(editor.doc())).expect("written");
    }
}

/// Timed by 24 frames a second, frames of one, two and six ticks, and two slices, one with a
/// pivot and a centre, one plain. Everything goes through the program's own commands.
fn rate_and_slices() -> Editor {
    let mut e = Editor::new(drawn_document(24, 2, 3));
    let retime = FrameCmd::Retime {
        from: 0,
        durations: vec![100, 40, 250],
    };
    e.apply(retime).expect("retimed");
    e.apply(FrameCmd::SetRate { fps: Some(24) }).expect("rate");
    let rect = |x, y, width, height| SliceRect {
        x,
        y,
        width,
        height,
    };
    let plain = Slice {
        name: "hit box".into(),
        color: 0xff_00_80_ff,
        bounds: rect(-2, 3, 8, 5),
        pivot: None,
        center: None,
    };
    let full = Slice {
        name: "panel ü".into(),
        color: 0x01_02_03_04,
        bounds: rect(1, 1, 18, 14),
        pivot: Some((4, -1)),
        center: Some(rect(5, 5, 8, 4)),
    };
    for slice in [plain, full] {
        e.apply(SliceCmd::Add {
            slice: Box::new(slice),
        })
        .expect("added");
    }
    e
}

#[test]
#[ignore = "writes corpus candidates; run by hand"]
fn write_the_phase_7_corpus_candidates() {
    let Some(out) = std::env::var_os("PIKSLAR_CORPUS_OUT").map(PathBuf::from) else {
        panic!("set PIKSLAR_CORPUS_OUT to an empty folder");
    };
    let path = out.join("v3-rate-slices.piks");
    assert!(!path.exists(), "{} exists", path.display());
    std::fs::write(&path, piks::save(rate_and_slices().doc())).expect("written");
}

/// Links across layers as an artist makes them: a cel of layer 0 linked by an Alt drag to two
/// frames of layer 2, one of them moved alone, and a hold on layer 1 that stays on its layer.
fn links() -> Editor {
    use pikslar_core::{CelCmd, CelDrag, DragKind, Landing, StepCmd};
    let mut e = Editor::new(drawn_document(24, 3, 3));
    let link = CelDrag {
        layers: (0, 0),
        frames: (0, 0),
        to_layer: 2,
        to_frame: 1,
        kind: DragKind::Link,
        landing: Landing::Overwrite,
    };
    e.apply(CelCmd::Drag(link)).expect("linked");
    e.apply(CelCmd::Drag(CelDrag {
        to_frame: 2,
        ..link
    }))
    .expect("linked");
    let layer = e.layer_id(2).expect("a layer there");
    let apart = CelCmd::MoveOne {
        layer,
        frame: 2,
        dx: 7,
        dy: -4,
    };
    e.apply(apart).expect("moved");
    e.apply(StepCmd::EndDrag).expect("ended");
    let held = e.layer_id(1).expect("a layer there");
    let hold = FrameCmd::HoldAcross {
        layer: held,
        source: 0,
        from: 0,
        to: 1,
    };
    e.apply(hold).expect("held");
    e
}

#[test]
#[ignore = "writes corpus candidates; run by hand"]
fn write_the_links_corpus_candidate() {
    let Some(out) = std::env::var_os("PIKSLAR_CORPUS_OUT").map(PathBuf::from) else {
        panic!("set PIKSLAR_CORPUS_OUT to an empty folder");
    };
    let path = out.join("v3-links.piks");
    assert!(!path.exists(), "{} exists", path.display());
    std::fs::write(&path, piks::save(links().doc())).expect("written");
}

/// A tile of `w` by `h` pixels: a colour with a darker top left corner, so every turn differs.
fn tile_picture(w: usize, h: usize, shade: u8) -> Arc<pikslar_core::Image> {
    let cells: Vec<u32> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let corner = x < w / 2 && y < h / 2;
            rgba(shade, if corner { 40 } else { 220 }, 255 - shade, 255)
        })
        .collect();
    Arc::new(Grid::from_cells(w, h, &cells).into_image())
}

/// A map cel of `w` by `h` cells.
fn map_cel(w: usize, h: usize, cells: &[u32]) -> pikslar_core::Cel {
    pikslar_core::Cel::new(Grid::from_cells(w, h, cells))
}

/// A painted layer over two hidden tilemap layers: one on a tileset of 8 by 8 tiles in which a
/// tile was removed (so its next id is 4 and the ids are 1 and 3), turned every way, and one on
/// a tileset of 8 by 4 tiles. A third tileset was added and removed, so the counter is 4. The
/// map layers are hidden so that the frame hashes stay what they are whether or not the renderer
/// draws tilemaps.
fn tilemap() -> Editor {
    use pikslar_core::orient::Orientation;
    use pikslar_core::tilemap::MapCell;
    use pikslar_core::tileset::TileId;
    let mut doc = drawn_document(32, 1, 2);
    let square = doc
        .add_tileset("Walls ü", ColorMode::Rgba, (8, 8))
        .expect("room");
    let oblong = doc
        .add_tileset("Planks", ColorMode::Rgba, (8, 4))
        .expect("room");
    let spare = doc
        .add_tileset("Spare", ColorMode::Rgba, (4, 4))
        .expect("room");
    let ids: Vec<TileId> = (0..3u8)
        .map(|n| {
            doc.add_tile(square, tile_picture(8, 8, 60 * n))
                .expect("fits")
        })
        .collect();
    doc.remove_tile(square, ids[1]).expect("removed");
    let planks: Vec<TileId> = (0..2u8)
        .map(|n| {
            doc.add_tile(oblong, tile_picture(8, 4, 90 * n))
                .expect("fits")
        })
        .collect();
    doc.remove_tileset(spare).expect("unused");
    doc.add_tilemap_layer(1, square).expect("room");
    doc.add_tilemap_layer(2, oblong).expect("room");
    let place = |tile: TileId, turn: u8| {
        let o = Orientation::from_index(turn).expect("one of eight");
        MapCell::place(tile, o).bits()
    };
    let walls: Vec<u32> = (0..12u8)
        .map(|i| {
            let tile = ids[(i as usize) % 2 * 2];
            if i % 5 == 4 {
                0
            } else {
                place(tile, i % 8)
            }
        })
        .collect();
    doc.put_cel(1, 0, map_cel(4, 3, &walls)).expect("fits");
    doc.put_cel(1, 1, map_cel(2, 2, &walls[..4])).expect("fits");
    // Oblong tiles take only the turns that keep their shape: the flips.
    let flips: Vec<u8> = (0..8u8)
        .filter(|&n| {
            !Orientation::from_index(n)
                .expect("one of eight")
                .swaps_axes()
        })
        .collect();
    let boards: Vec<u32> = (0..6usize)
        .map(|i| place(planks[i % 2], flips[i % flips.len()]))
        .collect();
    doc.put_cel(2, 0, map_cel(3, 2, &boards)).expect("fits");
    doc.layers_mut()[1].visible = false;
    doc.layers_mut()[2].visible = false;
    Editor::new(doc)
}

#[test]
#[ignore = "writes corpus candidates; run by hand"]
fn write_the_tilemap_corpus_candidate() {
    let Some(out) = std::env::var_os("PIKSLAR_CORPUS_OUT").map(PathBuf::from) else {
        panic!("set PIKSLAR_CORPUS_OUT to an empty folder");
    };
    let path = out.join("v3-tilemap.piks");
    assert!(!path.exists(), "{} exists", path.display());
    let mut e = tilemap();
    e.doc().check_tilemaps().expect("the rules hold");
    std::fs::write(&path, piks::try_save(e.doc()).expect("writable")).expect("written");
    let hashes = pikslar_safety::picture::frame_hashes(&mut e);
    let hex: Vec<String> = hashes.iter().map(|h| format!("{h:08x}")).collect();
    println!("MANIFEST v3-tilemap.piks 3 32 32 3 2 0 0 {}", hex.join(","));
}

/// Aseprite-compatible blending, set as the Canvas size dialog sets it, over four translucent
/// layers in modes only it draws as themselves, one at part opacity, across two frames.
fn aseprite_blending() -> Editor {
    use pikslar_core::Blending;
    let mut e = Editor::new(drawn_document(24, 4, 2));
    e.apply(CanvasCmd::SetBlending {
        blending: Blending::Aseprite,
    })
    .expect("set");
    let modes = [BlendMode::Hue, BlendMode::ColorBurn, BlendMode::SoftLight];
    for (index, blend) in (1..4).zip(modes) {
        on(&mut e, index, |layer| LayerCmd::SetBlend { layer, blend });
    }
    on(&mut e, 2, |layer| LayerCmd::SetOpacity {
        layer,
        opacity: 170,
    });
    e
}

#[test]
#[ignore = "writes corpus candidates; run by hand"]
fn write_the_aseprite_blending_corpus_candidate() {
    let Some(out) = std::env::var_os("PIKSLAR_CORPUS_OUT").map(PathBuf::from) else {
        panic!("set PIKSLAR_CORPUS_OUT to an empty folder");
    };
    let path = out.join("v3-aseprite-blending.piks");
    assert!(!path.exists(), "{} exists", path.display());
    let mut e = aseprite_blending();
    std::fs::write(&path, piks::try_save(e.doc()).expect("writable")).expect("written");
    let hashes = pikslar_safety::picture::frame_hashes(&mut e);
    let hex: Vec<String> = hashes.iter().map(|h| format!("{h:08x}")).collect();
    println!(
        "MANIFEST v3-aseprite-blending.piks 3 24 24 4 2 0 0 {}",
        hex.join(",")
    );
}
