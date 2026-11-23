//! Indexed exports against the RGBA exports of the same pictures: every corpus project, frame,
//! choice of layers and scale, as a frame, a sheet and a sequence. An indexed file must read
//! back as exactly the pixels of the RGBA one, with a palette of exactly its colours, or be
//! refused because there are more than 256.

use crate::{
    frame_output, sequence_outputs, sheet_outputs, ExportError, Layers, Output, Picture,
    SheetOptions,
};
use pikslar_core::{BlendMode, ColorMode, Direction, Document, Grid, Tag};
use pikslar_format::png;
use pikslar_format::png_index::depth_for;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn corpus() -> Vec<Document> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../pikslar-safety/corpus");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .expect("corpus")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "piks"))
        .collect();
    names.sort();
    let load = |p: &PathBuf| pikslar_format::piks::load(&std::fs::read(p).expect("read"));
    names.iter().map(|p| load(p).expect("loads")).collect()
}

fn written(mut output: Output<'_>) -> Vec<u8> {
    let mut bytes = Vec::new();
    (output.write)(&mut bytes).expect("written");
    bytes
}

/// Each chunk's kind and body.
fn chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let (mut at, mut out) = (8, Vec::new());
    while at + 8 <= png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().expect("4")) as usize;
        let kind: [u8; 4] = png[at + 4..at + 8].try_into().expect("4");
        out.push((kind, png[at + 8..at + 8 + len].to_vec()));
        at += 12 + len;
    }
    out
}

/// The palette an indexed PNG declares, as straight RGBA, and its bits a pixel.
pub(crate) fn declared(png: &[u8]) -> (Vec<u32>, u8) {
    let parts = chunks(png);
    assert_eq!(parts[0].1[9], 3, "colour type 3");
    let plte = &parts.iter().find(|c| &c.0 == b"PLTE").expect("PLTE").1;
    let trns = parts.iter().find(|c| &c.0 == b"tRNS").map(|c| c.1.clone());
    let alpha = |i: usize| trns.as_ref().and_then(|t| t.get(i)).map_or(255, |&a| a);
    let colours = plte
        .chunks_exact(3)
        .enumerate()
        .map(|(i, c)| u32::from_le_bytes([c[0], c[1], c[2], alpha(i)]))
        .collect();
    (colours, parts[0].1[8])
}

/// The cells as stored, with the colour behind a clear pixel kept, which a decoder for showing
/// images, this crate's own included, rightly drops. Every row the export writes is unfiltered.
fn stored(png: &[u8]) -> (usize, usize, Vec<u32>) {
    let parts = chunks(png);
    let header = &parts[0].1;
    let side = |at: usize| u32::from_be_bytes(header[at..at + 4].try_into().expect("4")) as usize;
    let (w, h, depth) = (side(0), side(4), usize::from(header[8]));
    let data: Vec<u8> = parts
        .iter()
        .filter(|c| &c.0 == b"IDAT")
        .flat_map(|c| c.1.clone())
        .collect();
    let raw = pikslar_format::inflate::inflate(&data[2..], 1 << 30).expect("inflates");
    let palette = (header[9] == 3).then(|| declared(png).0);
    let row_bytes = match &palette {
        None => w * 4,
        Some(_) => (w * depth).div_ceil(8),
    };
    let mut px = Vec::with_capacity(w * h);
    for row in raw.chunks_exact(row_bytes + 1) {
        assert_eq!(row[0], 0, "unfiltered");
        let cells = &row[1..];
        match &palette {
            None => px.extend(
                cells
                    .chunks_exact(4)
                    .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])),
            ),
            Some(p) => px.extend((0..w).map(|x| {
                let bit = x * depth;
                let i = (cells[bit / 8] >> (8 - depth - bit % 8)) & ((1u16 << depth) - 1) as u8;
                p[usize::from(i)]
            })),
        }
    }
    assert_eq!(px.len(), w * h);
    (w, h, px)
}

/// Checks `indexed` against `rgba`: the same stored pixels, a palette of exactly their colours,
/// in the order the export promises, at the fewest bits. Gives the number of colours.
fn same_picture(indexed: &[u8], rgba: &[u8]) -> usize {
    let (a, b) = (stored(indexed), stored(rgba));
    assert_eq!((a.0, a.1), (b.0, b.1));
    assert!(a.2 == b.2, "the same pixels");
    let shown = png::decode(indexed).expect("indexed");
    assert!(
        shown.px == png::decode(rgba).expect("rgba").px,
        "the same pixels shown"
    );
    let (palette, depth) = declared(indexed);
    let colours: BTreeSet<u32> = b.2.iter().copied().collect();
    let mut order: Vec<u32> = colours.iter().copied().filter(|c| c >> 24 != 255).collect();
    order.extend(colours.iter().copied().filter(|c| c >> 24 == 255));
    assert_eq!(palette, order, "exactly the image's colours");
    assert_eq!(depth, depth_for(palette.len()));
    palette.len()
}

fn choices(doc: &Document) -> Vec<Layers> {
    let count = doc.layers().len();
    let mut list = vec![Layers::Visible];
    list.extend((0..count).map(|l| Layers::Only(vec![l])));
    list
}

#[test]
fn every_corpus_frame_is_the_rgba_export_or_refused_for_its_colours() {
    let (mut checked, mut refused) = (0, 0);
    for doc in corpus() {
        for layers in choices(&doc) {
            for scale in [1, 2, 3] {
                let Ok(rgba) = Picture::new(&doc, &layers, scale) else {
                    continue;
                };
                let indexed = Picture::new(&doc, &layers, scale)
                    .expect("drawn")
                    .with_indexed(true);
                for frame in 0..doc.frames.len() {
                    let plain = written(frame_output(&rgba, frame, "f.png".into()).expect("rgba"));
                    match frame_output(&indexed, frame, "f.png".into()) {
                        Ok(out) => {
                            same_picture(&written(out), &plain);
                            checked += 1;
                        }
                        Err(ExportError::TooManyColours { frame: Some(f), .. }) => {
                            assert_eq!(f, frame);
                            let px = stored(&plain).2;
                            assert!(px.iter().collect::<BTreeSet<_>>().len() > 256);
                            refused += 1;
                        }
                        Err(e) => panic!("{e}"),
                    }
                }
            }
        }
    }
    assert!(checked > 100, "{checked} checked, {refused} refused");
}

#[test]
fn sheets_and_sequences_are_their_rgba_exports_with_their_own_palettes() {
    for doc in corpus().into_iter().filter(|d| d.frames.len() > 1) {
        let last = doc.frames.len() - 1;
        for scale in [1, 2] {
            let rgba = Picture::new(&doc, &Layers::Visible, scale).expect("drawn");
            let indexed = Picture::new(&doc, &Layers::Visible, scale)
                .expect("drawn")
                .with_indexed(true);
            let plain = sheet_outputs(&rgba, 0..=last, "s.png".into(), &SheetOptions::default())
                .expect("planned");
            match sheet_outputs(&indexed, 0..=last, "s.png".into(), &SheetOptions::default()) {
                Ok(sheet) => {
                    let (mut sheet, mut plain) = (sheet.into_iter(), plain.into_iter());
                    let (a, b) = (sheet.next().expect("png"), plain.next().expect("png"));
                    let png = written(a);
                    same_picture(&png, &written(b));
                    let (a, b) = (sheet.next().expect("json"), plain.next().expect("json"));
                    let expected = indexed_sheet_description(&written(b), &png);
                    assert_eq!(
                        text(written(a)),
                        expected,
                        "the RGBA one, saying it is indexed"
                    );
                }
                Err(e) => assert!(matches!(e, ExportError::TooManyColours { frame: None, .. })),
            }
            let path = std::path::Path::new("q.png");
            let Ok(members) = sequence_outputs(&indexed, 0..=last, path) else {
                continue;
            };
            let plain = sequence_outputs(&rgba, 0..=last, path).expect("planned");
            let mut pngs = Vec::new();
            for (a, b) in members.into_iter().zip(plain) {
                let (a, b) = (written(a), written(b));
                if a.starts_with(b"{") {
                    let expected = indexed_sequence_description(&b, &pngs);
                    assert_eq!(text(a), expected, "the RGBA one, saying each is indexed");
                } else {
                    same_picture(&a, &b);
                    pngs.push(a);
                }
            }
        }
    }
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("utf-8")
}

/// What an indexed image's description says of it in place of `"RGBA8888"`.
fn indexed_fields(png: &[u8]) -> String {
    let (colours, depth) = declared(png);
    format!("\"bitDepth\":{depth},\"paletteSize\":{}", colours.len())
}

/// The RGBA sheet's description `rgba`, as the indexed sheet `png` should be described.
fn indexed_sheet_description(rgba: &[u8], png: &[u8]) -> String {
    let rgba = String::from_utf8(rgba.to_vec()).expect("utf-8");
    let indexed = format!("\"format\":\"INDEXED\",{}", indexed_fields(png));
    assert_eq!(rgba.matches("\"format\":\"RGBA8888\"").count(), 1);
    rgba.replace("\"format\":\"RGBA8888\"", &indexed)
}

/// The RGBA sequence's description `rgba`, as members `pngs` should be described: each entry
/// with its own bits and palette, and the whole said to be indexed.
fn indexed_sequence_description(rgba: &[u8], pngs: &[Vec<u8>]) -> String {
    let rgba = String::from_utf8(rgba.to_vec()).expect("utf-8");
    let mut parts = rgba.split("{\"filename\":");
    let mut out = parts.next().expect("start").to_string();
    let mut count = 0;
    for (part, png) in parts.zip(pngs) {
        let end = part.find('}').expect("an entry ends");
        out.push_str(&format!(
            "{{\"filename\":{},{}{}",
            &part[..end],
            indexed_fields(png),
            &part[end..]
        ));
        count += 1;
    }
    assert_eq!(count, pngs.len(), "an entry for each member");
    out.replace("\"format\":\"RGBA8888\"", "\"format\":\"INDEXED\"")
}

/// A project of `w` by `h` showing `colours` different colours, every one translucent if
/// `translucent`, and in its first cell the clear colour if there are at least two.
/// Translucent colours have channels of 0 or 255 only, which blending keeps exactly.
fn coloured(colours: usize, (w, h): (usize, usize), translucent: bool) -> Document {
    let colour = |i: usize| match i {
        0 if colours > 1 => 0,
        _ if translucent => {
            let mask = (i % 8) ^ (i / 254);
            let channels = (0..3)
                .filter(|b| (mask >> b) & 1 == 1)
                .map(|b| 0xff << (8 * b));
            ((1 + (i % 254) as u32) << 24) | channels.sum::<u32>()
        }
        _ => 0xff00_0000 | ((i as u32 * 0x01_0305) & 0x00ff_ffff) | 0x0001_0000,
    };
    let cells: Vec<u32> = (0..w * h).map(|i| colour(i % colours)).collect();
    let mut d = Document::new(w, h, ColorMode::Rgba).expect("valid");
    d.set_cel(0, 0, Grid::from_cells(w, h, &cells))
        .expect("cel");
    d
}

fn indexed_and_rgba(doc: &Document) -> Result<(Vec<u8>, Vec<u8>), ExportError> {
    let rgba = Picture::visible(doc);
    let indexed = Picture::visible(doc).with_indexed(true);
    let plain = written(frame_output(&rgba, 0, "f.png".into())?);
    let coded = written(frame_output(&indexed, 0, "f.png".into())?);
    Ok((coded, plain))
}

#[test]
fn one_to_256_colours_are_written_at_the_fewest_bits_and_257_are_refused() {
    for colours in [1, 2, 3, 4, 5, 16, 17, 255, 256] {
        for translucent in [false, true] {
            let doc = coloured(colours, (64, 16), translucent);
            let (indexed, rgba) = indexed_and_rgba(&doc).expect("fits");
            assert_eq!(same_picture(&indexed, &rgba), colours, "{colours} colours");
            let has_trns = chunks(&indexed).iter().any(|c| &c.0 == b"tRNS");
            assert_eq!(
                has_trns,
                translucent || colours > 1,
                "{colours} {translucent}"
            );
            assert_eq!(
                indexed_and_rgba(&doc).expect("again").0,
                indexed,
                "deterministic"
            );
        }
    }
    let over = coloured(257, (64, 16), false);
    let refused = indexed_and_rgba(&over).expect_err("too many");
    assert!(matches!(
        refused,
        ExportError::TooManyColours {
            frame: Some(0),
            hidden: 0
        }
    ));
    assert_eq!(
        refused.to_string(),
        "Frame 1 has more than 256 colours, so it cannot be an indexed PNG without changing \
         some, and no colour is changed. Export it without --indexed"
    );
}

#[test]
fn blended_translucent_and_hidden_layers_give_the_colours_the_rgba_export_shows() {
    let mut doc = coloured(5, (24, 24), false);
    doc.add_layer(1, ColorMode::Rgba).expect("layer");
    doc.add_layer(2, ColorMode::Rgba).expect("layer");
    let ink: Vec<u32> = (0..24 * 24)
        .map(|i| if i % 3 == 0 { 0x80ff_8040 } else { 0 })
        .collect();
    doc.set_cel(1, 0, Grid::from_cells(24, 24, &ink))
        .expect("cel");
    let hidden: Vec<u32> = vec![0xff12_3456; 24 * 24];
    doc.set_cel(2, 0, Grid::from_cells(24, 24, &hidden))
        .expect("cel");
    doc.layers_mut()[1].blend = BlendMode::Multiply;
    doc.layers_mut()[1].opacity = 200;
    doc.layers_mut()[2].visible = false;
    let (indexed, rgba) = indexed_and_rgba(&doc).expect("fits");
    let colours = same_picture(&indexed, &rgba);
    assert!(
        colours > 5,
        "blending made colours the cells do not hold: {colours}"
    );
    let px = png::decode(&indexed).expect("decodes").px;
    assert!(!px.contains(&0xff12_3456), "the hidden layer is not drawn");
    for layers in [Layers::Only(vec![2]), Layers::Only(vec![0, 1, 2])] {
        let rgba = Picture::new(&doc, &layers, 2).expect("drawn");
        let indexed = Picture::new(&doc, &layers, 2)
            .expect("drawn")
            .with_indexed(true);
        let a = written(frame_output(&indexed, 0, "f.png".into()).expect("fits"));
        same_picture(
            &a,
            &written(frame_output(&rgba, 0, "f.png".into()).expect("rgba")),
        );
    }
}

/// Clear cells, each with a different colour stored behind it, and one opaque colour.
fn hidden_colours(clear: u32) -> Vec<u32> {
    (0..64)
        .map(|i| {
            if i % 2 == 0 && i / 2 < clear {
                0x00ff_0000 | i
            } else {
                0xff00_00ff
            }
        })
        .collect()
}

#[test]
fn colours_behind_clear_pixels_are_kept_exactly_as_the_rgba_export_keeps_them() {
    let mut doc = coloured(3, (8, 8), false);
    doc.set_cel(0, 0, Grid::from_cells(8, 8, &hidden_colours(32)))
        .expect("cel");
    let (indexed, rgba) = indexed_and_rgba(&doc).expect("fits");
    assert_eq!(
        same_picture(&indexed, &rgba),
        33,
        "32 clear colours and one opaque"
    );
    assert_eq!(declared(&indexed).0[..2], [0x00ff_0000, 0x00ff_0002]);
    doc.add_layer(1, ColorMode::Rgba).expect("layer");
    let (indexed, rgba) = indexed_and_rgba(&doc).expect("fits");
    assert_eq!(
        same_picture(&indexed, &rgba),
        2,
        "blended layers keep no hidden colour"
    );
    assert_eq!(declared(&indexed).0, [0, 0xff00_00ff]);
}

#[test]
fn a_refusal_counts_the_clear_colours_that_made_it_too_many() {
    let mut doc = Document::new(300, 1, ColorMode::Rgba).expect("valid");
    let cells: Vec<u32> = (0..300)
        .map(|i| if i < 200 { i << 8 } else { 0xff00_0000 | i })
        .collect();
    doc.set_cel(0, 0, Grid::from_cells(300, 1, &cells))
        .expect("cel");
    let refused = indexed_and_rgba(&doc).expect_err("too many");
    assert_eq!(
        refused.to_string(),
        "Frame 1 has more than 256 colours, so it cannot be an indexed PNG without changing \
         some, and no colour is changed. Export it without --indexed. At least 199 of them \
         are clear pixels with a colour stored behind them, which an indexed PNG keeps exactly \
         as the RGBA one does"
    );
    let mut gapped = coloured(256, (16, 16), false);
    let opaque: Vec<u32> = (0..256).map(|i| 0xff00_0000 | i as u32).collect();
    gapped
        .set_cel(0, 0, Grid::from_cells(16, 16, &opaque))
        .expect("cel");
    gapped.add_frame(1, true).expect("frame");
    gapped.add_frame(2, true).expect("frame");
    gapped.tags.push(Tag {
        name: "end".into(),
        from: 2,
        to: 2,
        direction: Direction::Forward,
        repeat: 0,
        id: pikslar_core::TagId::fresh(),
    });
    let picture = Picture::visible(&gapped).with_indexed(true);
    let refused = sheet_outputs(&picture, 0..=2, "s.png".into(), &SheetOptions::default())
        .err()
        .expect("the space is a 257th");
    assert!(
        matches!(
            refused,
            ExportError::TooManyColours {
                frame: None,
                hidden: 0
            }
        ),
        "the space between frames hides no colour: {refused:?}"
    );
    let sheet = ExportError::TooManyColours {
        frame: None,
        hidden: 0,
    };
    assert_eq!(
        sheet.to_string(),
        "These frames together have more than 256 colours, so one sheet of them cannot be an \
         indexed PNG without changing some, and no colour is changed. Choose fewer frames, \
         --format sequence, or no --indexed"
    );
}

#[test]
fn a_sheet_with_space_between_frames_counts_the_space_as_a_colour() {
    let mut doc = coloured(256, (16, 16), false);
    doc.add_frame(1, true).expect("frame");
    doc.add_frame(2, true).expect("frame");
    let picture = Picture::visible(&doc).with_indexed(true);
    let sheet = sheet_outputs(&picture, 0..=2, "s.png".into(), &SheetOptions::default())
        .expect("one row, no space");
    let rgba = Picture::visible(&doc);
    let plain =
        sheet_outputs(&rgba, 0..=2, "s.png".into(), &SheetOptions::default()).expect("planned");
    let png = written(sheet.into_iter().next().expect("png"));
    same_picture(&png, &written(plain.into_iter().next().expect("png")));
    let mut opaque = coloured(256, (16, 16), false);
    let full: Vec<u32> = (0..256).map(|i| 0xff00_0000 | i as u32).collect();
    opaque
        .set_cel(0, 0, Grid::from_cells(16, 16, &full))
        .expect("cel");
    for f in 1..5 {
        opaque.add_frame(f, true).expect("frame");
    }
    opaque.tags.push(Tag {
        name: "run".into(),
        from: 2,
        to: 4,
        direction: Direction::Forward,
        repeat: 0,
        id: pikslar_core::TagId::fresh(),
    });
    let picture = Picture::visible(&opaque).with_indexed(true);
    assert!(
        sheet_outputs(&picture, 0..=1, "s.png".into(), &SheetOptions::default()).is_ok(),
        "two frames fill a row"
    );
    let refused = sheet_outputs(&picture, 0..=4, "s.png".into(), &SheetOptions::default())
        .err()
        .expect("a row of two under a row of three leaves space, a 257th colour");
    assert!(matches!(
        refused,
        ExportError::TooManyColours { frame: None, .. }
    ));
}

#[test]
fn the_longest_and_widest_frames_are_written_whole() {
    for size in [(8192, 2), (2, 8192), (8192, 1)] {
        let doc = coloured(3, size, true);
        let (indexed, rgba) = indexed_and_rgba(&doc).expect("fits");
        assert_eq!(same_picture(&indexed, &rgba), 3);
    }
    let doc = coloured(2, (2, 1), false);
    let big = Picture::new(&doc, &Layers::Visible, 4096).expect("8192 wide");
    let a = written(frame_output(&big.with_indexed(true), 0, "f.png".into()).expect("fits"));
    let image = png::decode(&a).expect("decodes");
    assert_eq!((image.width, image.height), (8192, 4096));
    assert!(image.px[..4096].iter().all(|&p| p == 0));
    assert!(image.px[4096..8192]
        .iter()
        .all(|&p| p == image.px[4096] && p >> 24 == 255));
}
