//! Indexed PNGs written by [`PngStream::indexed`], read back by this crate's own decoder. The
//! export's tests read them with Pillow and ffmpeg as well.

use crate::png::decode;
use crate::png_write::PngStream;

/// `colours` different colours, some translucent and one fully transparent when there are
/// enough, and an image of `w` by `h` using every one.
fn image(colours: usize, w: usize, h: usize) -> (Vec<u32>, Vec<u32>) {
    let palette: Vec<u32> = (0..colours as u32)
        .map(|i| match i % 7 {
            0 if i == 0 && colours > 2 => 0,
            3 => (0x40 + i % 64) << 24 | i.wrapping_mul(2_654_435_761) >> 8,
            _ => 0xff00_0000 | (i.wrapping_mul(2_654_435_761) >> 8),
        })
        .collect();
    let px = (0..w * h)
        .map(|i| palette[(i * 7 + i / w) % colours])
        .collect();
    (palette, px)
}

fn write(palette: &[u32], px: &[u32], w: usize, h: usize, band: usize) -> Vec<u8> {
    let mut png = PngStream::indexed(Vec::new(), w, h, palette).expect("start");
    for rows in px.chunks(w * band) {
        png.rows(rows).expect("rows");
    }
    png.finish().expect("finished")
}

/// Each chunk's kind and body, in order.
fn chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let mut at = 8;
    let mut out = Vec::new();
    while at < png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().expect("4")) as usize;
        let kind: [u8; 4] = png[at + 4..at + 8].try_into().expect("4");
        out.push((kind, png[at + 8..at + 8 + len].to_vec()));
        at += 12 + len;
    }
    out
}

#[test]
fn every_palette_size_reads_back_as_the_same_cells_at_the_fewest_bits() {
    for (colours, depth) in [
        (1, 1),
        (2, 1),
        (3, 2),
        (4, 2),
        (5, 4),
        (16, 4),
        (17, 8),
        (255, 8),
        (256, 8),
    ] {
        for (w, h) in [(1, 1), (7, 3), (33, 17)] {
            let (palette, px) = image(colours, w, h);
            let png = write(&palette, &px, w, h, 5);
            let read = decode(&png).expect("valid");
            assert_eq!((read.width, read.height), (w, h));
            assert_eq!(read.px, px, "{colours} colours at {w} by {h}");
            let parts = chunks(&png);
            assert_eq!(&parts[0].0, b"IHDR");
            assert_eq!(parts[0].1[8..], [depth, 3, 0, 0, 0], "{colours} colours");
            assert_eq!(&parts[1].0, b"PLTE");
            assert_eq!(parts[1].1.len(), 3 * colours);
        }
    }
}

#[test]
fn transparency_is_written_only_for_a_palette_that_has_some() {
    let opaque = [0xff00_0000, 0xff12_3456, 0xffff_ffff];
    let png = write(&opaque, &[opaque[2], opaque[0], opaque[1]], 3, 1, 1);
    let kinds: Vec<[u8; 4]> = chunks(&png).iter().map(|c| c.0).collect();
    assert_eq!(kinds, [*b"IHDR", *b"PLTE", *b"IDAT", *b"IEND"]);
    let clear = [0, 0x8012_3456, 0xff00_00ff];
    let png = write(&clear, &[clear[2], clear[0], clear[1]], 3, 1, 1);
    let parts = chunks(&png);
    assert_eq!(&parts[2].0, b"tRNS");
    assert_eq!(parts[2].1, [0, 0x80]);
    assert_eq!(decode(&png).expect("valid").px, [clear[2], 0, clear[1]]);
}

#[test]
fn the_same_cells_and_palette_give_the_same_bytes_however_rows_arrive() {
    let (palette, px) = image(200, 61, 40);
    let whole = write(&palette, &px, 61, 40, 40);
    for band in [1, 3, 16] {
        assert_eq!(write(&palette, &px, 61, 40, band), whole, "bands of {band}");
    }
}

#[test]
fn a_cell_not_in_the_palette_adds_none_of_its_rows() {
    let palette = [0xff00_0000, 0xff00_00ff];
    let mut png = PngStream::indexed(Vec::new(), 2, 3, &palette).expect("start");
    png.rows(&[palette[0], palette[1]]).expect("row");
    let stray = [palette[1], palette[0], palette[0], 0xff00_ff00];
    assert!(png.rows(&stray).is_err());
    assert!(png
        .rows(&[palette[1], palette[1], palette[0], palette[0]])
        .is_ok());
    let read = decode(&png.finish().expect("finished")).expect("valid");
    assert_eq!(
        read.px,
        [palette[0], palette[1], palette[1], palette[1], palette[0], palette[0]]
    );
}

#[test]
fn a_palette_of_no_colours_or_too_many_starts_no_image() {
    assert!(PngStream::indexed(Vec::new(), 1, 1, &[]).is_err());
    let over: Vec<u32> = (0..257).collect();
    assert!(PngStream::indexed(Vec::new(), 1, 1, &over).is_err());
    assert!(PngStream::indexed(Vec::new(), 0, 1, &[0]).is_err());
}

#[test]
fn the_widest_image_packs_its_rows_whole() {
    let palette = [0, 0xff00_00ff, 0xff00_ff00];
    let w = 8192;
    let px: Vec<u32> = (0..w * 2).map(|i| palette[i % 3]).collect();
    let png = write(&palette, &px, w, 2, 1);
    assert_eq!(decode(&png).expect("valid").px, px);
}
