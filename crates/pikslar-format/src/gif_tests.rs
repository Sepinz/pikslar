//! The GIF writer against the test reader in `gif_read.rs`.

use crate::gif_read::read;
use crate::gif_read::shown;
use crate::gif_write::{table_bits, Area, Disposal, GifStream, MAX_PLAYS};

/// `n` opaque colours.
fn colours(n: usize) -> Vec<u32> {
    (0..n as u32)
        .map(|i| 0xff00_0000 | (i * 0x0001_0203))
        .collect()
}

/// A GIF of `frames`, each `w` by `h` cells, sent `band` rows at a time.
fn gif(
    (w, h): (usize, usize),
    (palette, clear): (&[u32], bool),
    frames: &[(u16, Vec<u32>)],
    (plays, band): (u32, usize),
) -> Vec<u8> {
    let mut g = GifStream::new(Vec::new(), (w, h), (palette, clear), plays).expect("starts");
    let after = if clear {
        Disposal::Clear
    } else {
        Disposal::Keep
    };
    for (delay, cells) in frames {
        let mut f = g.frame(*delay, whole(w, h), after).expect("frame");
        for rows in cells.chunks(w * band) {
            f.rows(rows).expect("rows");
        }
        f.end().expect("ends");
    }
    g.finish().expect("finishes")
}

fn whole(width: usize, height: usize) -> Area {
    Area {
        x: 0,
        y: 0,
        width,
        height,
    }
}

/// Cells of `w` by `h` from a little generator, drawn from `palette` and clear if `clear`.
fn noise(w: usize, h: usize, palette: &[u32], clear: bool, seed: u32) -> Vec<u32> {
    let mut x = seed.wrapping_mul(2_654_435_761) | 1;
    let choices = palette.len() + usize::from(clear);
    (0..w * h)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let pick = x as usize % choices;
            palette.get(pick).copied().unwrap_or(x & 0x00ff_ffff)
        })
        .collect()
}

/// What index each cell should read back as: clear cells the clear entry, 0.
fn expected(cells: &[u32], palette: &[u32], clear: bool) -> Vec<u8> {
    cells
        .iter()
        .map(|&c| match (c >> 24, clear) {
            (0, true) => 0,
            _ => {
                (palette.iter().position(|&p| p == c).expect("in palette") + usize::from(clear))
                    as u8
            }
        })
        .collect()
}

#[test]
fn every_palette_size_reads_back_cell_for_cell() {
    for n in [1, 2, 3, 4, 5, 15, 16, 17, 127, 128, 129, 255, 256] {
        for clear in [false, true] {
            if n + usize::from(clear) > 256 {
                continue;
            }
            let palette = colours(n);
            let cells = noise(37, 23, &palette, clear, n as u32);
            let bytes = gif((37, 23), (&palette, clear), &[(7, cells.clone())], (0, 5));
            let r = read(&bytes);
            assert_eq!(r.size, (37, 23));
            let entries = n + usize::from(clear);
            assert_eq!(r.table.len(), 1 << table_bits(entries), "{n} {clear}");
            for (i, &c) in palette.iter().enumerate() {
                let at = i + usize::from(clear);
                assert_eq!(r.table[at], [c as u8, (c >> 8) as u8, (c >> 16) as u8]);
            }
            let f = &r.frames[0];
            assert_eq!(f.indices, expected(&cells, &palette, clear), "{n} {clear}");
            assert_eq!(f.transparent, clear.then_some(0));
            assert_eq!(
                f.min,
                table_bits(entries).max(2),
                "the fewest bits LZW allows"
            );
            assert_eq!(r.background, 0);
        }
    }
}

/// Whatever code comes last, the end code after it is as wide as a reader expects: a reader
/// grows its table on that last code too, so it may read the end code one bit wider.
#[test]
fn the_end_code_is_read_whatever_length_the_image_has() {
    let short = 1..=2100;
    // With 256 colours of noise the table fills, 12 bits wide, about here.
    let full = 3700..=4400;
    for (n, clear, lens) in [
        (2, false, short.clone()),
        (3, false, short.clone()),
        (1, true, short.clone()),
        (16, false, short.clone()),
        (200, true, short),
        (256, false, full),
    ] {
        let palette = colours(n);
        for len in lens {
            let cells = noise(len, 1, &palette, clear, len as u32);
            let r = read(&gif(
                (len, 1),
                (&palette, clear),
                &[(3, cells.clone())],
                (0, 1),
            ));
            assert_eq!(
                r.frames[0].indices,
                expected(&cells, &palette, clear),
                "{n} {clear} {len}"
            );
        }
    }
}

#[test]
fn the_table_fills_and_clears_many_times_in_one_frame() {
    let palette = colours(256);
    let cells = noise(512, 300, &palette, false, 9);
    let r = read(&gif(
        (512, 300),
        (&palette, false),
        &[(5, cells.clone())],
        (0, 16),
    ));
    assert_eq!(r.frames[0].indices, expected(&cells, &palette, false));
    let flat = vec![palette[3]; 8192 * 3];
    let r = read(&gif(
        (8192, 3),
        (&palette, false),
        &[(5, flat.clone())],
        (0, 1),
    ));
    assert_eq!(r.frames[0].indices, expected(&flat, &palette, false));
}

#[test]
fn how_rows_arrive_does_not_change_a_byte() {
    let palette = colours(40);
    let cells = noise(64, 48, &palette, true, 3);
    let one = gif((64, 48), (&palette, true), &[(4, cells.clone())], (0, 1));
    for band in [2, 7, 16, 48] {
        assert_eq!(
            gif((64, 48), (&palette, true), &[(4, cells.clone())], (0, band)),
            one
        );
    }
}

#[test]
fn every_frame_is_whole_restored_to_clear_and_timed() {
    let palette = colours(6);
    let frames: Vec<(u16, Vec<u32>)> = (0..5)
        .map(|i| (2 + i as u16 * 3, noise(9, 4, &palette, true, i)))
        .collect();
    let r = read(&gif((9, 4), (&palette, true), &frames, (0, 4)));
    assert_eq!(r.frames.len(), 5);
    for (f, (delay, cells)) in r.frames.iter().zip(&frames) {
        assert_eq!(f.delay, *delay);
        assert_eq!(f.disposal, 2, "restore to background");
        assert_eq!(f.bounds, (0, 0, 9, 4), "the whole canvas");
        assert_eq!(f.indices, expected(cells, &palette, true));
    }
}

#[test]
fn plays_become_repeats_and_one_play_has_no_loop() {
    let palette = colours(2);
    let frames = [(10, vec![palette[0]; 4])];
    let repeats = |plays| read(&gif((2, 2), (&palette, false), &frames, (plays, 2))).repeats;
    assert_eq!(repeats(0), Some(0), "forever");
    assert_eq!(repeats(1), None);
    assert_eq!(repeats(2), Some(1));
    assert_eq!(repeats(MAX_PLAYS), Some(u16::MAX));
    assert!(GifStream::new(Vec::new(), (2, 2), (&palette, false), MAX_PLAYS + 1).is_err());
}

#[test]
fn colours_that_are_not_exactly_in_the_palette_are_errors() {
    let palette = colours(3);
    let mut g = GifStream::new(Vec::new(), (2, 1), (&palette, false), 0).expect("starts");
    let mut f = g.frame(2, whole(2, 1), Disposal::Keep).expect("frame");
    assert!(
        f.rows(&[palette[0], 0]).is_err(),
        "clear, with no clear entry"
    );
    assert!(
        f.rows(&[palette[0], 0x80ff_ffff & palette[1]]).is_err(),
        "translucent"
    );
    assert!(
        f.rows(&[palette[0], palette[1] ^ 1]).is_err(),
        "a colour not there"
    );
    assert!(f.rows(&[palette[0]]).is_err(), "half a row");
    f.rows(&[palette[0], palette[2]])
        .expect("a row of its colours");
    assert!(
        f.rows(&[palette[0], palette[2]]).is_err(),
        "more rows than it has"
    );
    let mut g = GifStream::new(Vec::new(), (2, 2), (&palette, false), 0).expect("starts");
    let mut f = g.frame(2, whole(2, 2), Disposal::Keep).expect("frame");
    f.rows(&[palette[0], palette[1]]).expect("row");
    assert!(f.end().is_err(), "ended a row short");
}

#[test]
fn palettes_and_sizes_out_of_range_are_refused() {
    let new = |size, palette: &[u32], clear| GifStream::new(Vec::new(), size, (palette, clear), 0);
    assert!(new((0, 1), &colours(1), false).is_err());
    assert!(new((65_536, 1), &colours(1), false).is_err());
    assert!(new((1, 1), &[], false).is_err());
    assert!(new((1, 1), &[], true).is_ok(), "an image all clear");
    assert!(new((1, 1), &colours(256), true).is_err(), "257 entries");
    assert!(new((1, 1), &colours(256), false).is_ok());
    assert!(
        new((1, 1), &[0x80ff_ffff], false).is_err(),
        "a translucent entry"
    );
}

#[test]
fn a_clear_cell_is_the_clear_entry_whatever_it_hides() {
    let palette = colours(2);
    let cells = vec![0x0012_3456, palette[1], 0, 0x00ff_ffff];
    let r = read(&gif((2, 2), (&palette, true), &[(3, cells)], (0, 2)));
    assert_eq!(r.frames[0].indices, [0, 2, 0, 0]);
}

/// FNV-1a, to pin bytes without a crate.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The same input always gives these bytes, which ffmpeg and Pillow read back. A change to how
/// strings are found or when the table clears would still decode, so it is pinned here.
#[test]
fn the_bytes_of_known_images_are_pinned() {
    let palette = colours(200);
    let cells = noise(300, 200, &palette, true, 11);
    let busy = gif((300, 200), (&palette, true), &[(4, cells)], (0, 16));
    let two = colours(2);
    let small = gif(
        (5, 3),
        (&two, false),
        &[(9, noise(5, 3, &two, false, 2))],
        (3, 1),
    );
    assert_eq!(
        (busy.len(), fnv(&busy)),
        (81_935, 15_104_112_605_136_685_655),
        "busy"
    );
    assert_eq!(
        (small.len(), fnv(&small)),
        (65, 857_713_112_401_516_613),
        "small"
    );
}

/// A frame in part of the canvas, and what it leaves: kept, or cleared to the clear entry.
#[test]
fn areas_and_disposals_are_written_as_given_and_shown_so() {
    let palette = colours(3);
    let (a, b, c) = (palette[0], palette[1], palette[2]);
    let mut g = GifStream::new(Vec::new(), (4, 3), (&palette, true), 0).expect("starts");
    let steps: [(Area, Disposal, Vec<u32>); 3] = [
        (whole(4, 3), Disposal::Keep, vec![a; 12]),
        (
            Area {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            },
            Disposal::Clear,
            vec![b, 0, c, b],
        ),
        (
            Area {
                x: 3,
                y: 0,
                width: 1,
                height: 1,
            },
            Disposal::Keep,
            vec![c],
        ),
    ];
    for (area, after, cells) in &steps {
        let mut f = g.frame(5, *area, *after).expect("frame");
        f.rows(cells).expect("rows");
        f.end().expect("ends");
    }
    let r = read(&g.finish().expect("finishes"));
    let got: Vec<_> = r.frames.iter().map(|f| (f.bounds, f.disposal)).collect();
    assert_eq!(
        got,
        [((0, 0, 4, 3), 1), ((1, 1, 2, 2), 2), ((3, 0, 1, 1), 1)]
    );
    let frames = shown(&r);
    assert_eq!(
        frames[1],
        [a, a, a, a, a, b, a, a, a, c, b, a],
        "a clear cell keeps what is under"
    );
    assert_eq!(
        frames[2],
        [a, a, a, c, a, 0, 0, a, a, 0, 0, a],
        "the area cleared, then drawn on"
    );
}

#[test]
fn areas_off_the_canvas_empty_or_cleared_without_a_clear_entry_are_refused() {
    let palette = colours(2);
    let mut g = GifStream::new(Vec::new(), (4, 3), (&palette, false), 0).expect("starts");
    let area = |x, y, width, height| Area {
        x,
        y,
        width,
        height,
    };
    for bad in [
        area(0, 0, 0, 1),
        area(0, 0, 1, 0),
        area(3, 0, 2, 1),
        area(0, 2, 1, 2),
    ] {
        assert!(g.frame(2, bad, Disposal::Keep).is_err(), "{bad:?}");
    }
    assert!(
        g.frame(2, whole(4, 3), Disposal::Clear).is_err(),
        "cleared to a background that is a colour"
    );
    let mut f = g
        .frame(2, area(1, 1, 2, 2), Disposal::Keep)
        .expect("inside");
    assert!(
        f.rows(&[palette[0]; 4 * 2]).is_err(),
        "rows as wide as the canvas"
    );
    f.rows(&[palette[0]; 2 * 2])
        .expect("rows as wide as the area");
    f.end().expect("ends");
}
