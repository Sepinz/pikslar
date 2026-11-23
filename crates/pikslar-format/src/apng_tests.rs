//! The APNG writer against the strict test reader in `apng_read.rs`.

use crate::apng_read::{played, read};
use crate::apng_write::{ApngStream, Area, Blend, Control, Dispose};
use crate::png_write::{PngStream, IDAT_MAX};

fn whole(w: usize, h: usize) -> Area {
    Area {
        x: 0,
        y: 0,
        width: w,
        height: h,
    }
}

fn kept(area: Area, delay_ms: u16) -> Control {
    Control {
        area,
        delay: (delay_ms, 1000),
        after: Dispose::Keep,
        blend: Blend::Source,
    }
}

/// An APNG of `frames` in a `size` image, each `(control, cells)` sent `band` rows at a time.
fn apng(
    size: (usize, usize),
    frames: &[(Control, Vec<u32>)],
    (plays, band): (u32, usize),
) -> Vec<u8> {
    let mut a = ApngStream::new(Vec::new(), size, frames.len() as u32, plays).expect("starts");
    for (control, cells) in frames {
        let mut f = a.frame(*control).expect("frame");
        for rows in cells.chunks(control.area.width * band) {
            f.rows(rows).expect("rows");
        }
        f.end().expect("ends");
    }
    a.finish().expect("finishes")
}

/// A `w` by `h` frame of varied colours and alphas, partial ones included.
fn varied(w: usize, h: usize, seed: u32) -> Vec<u32> {
    (0..(w * h) as u32)
        .map(|i| {
            let v = i
                .wrapping_mul(2_654_435_761)
                .wrapping_add(seed.wrapping_mul(40_503));
            match v % 5 {
                0 => 0,
                1 => v & 0x00ff_ffff,
                _ => v,
            }
        })
        .collect()
}

#[test]
fn chunks_are_in_the_order_the_specification_gives() {
    let (w, h) = (5, 4);
    let frames = [
        (kept(whole(w, h), 100), varied(w, h, 1)),
        (kept(whole(w, h), 250), varied(w, h, 2)),
        (kept(whole(w, h), 60_000), varied(w, h, 3)),
    ];
    let r = read(&apng((w, h), &frames, (0, 3)));
    let kinds: Vec<&[u8]> = r.kinds.iter().map(|k| k.as_slice()).collect();
    assert_eq!(
        kinds,
        [
            &b"IHDR"[..],
            b"acTL",
            b"fcTL",
            b"IDAT",
            b"fcTL",
            b"fdAT",
            b"fcTL",
            b"fdAT",
            b"IEND"
        ]
    );
    assert_eq!(r.plays, 0);
    let delays: Vec<_> = r.frames.iter().map(|f| f.delay).collect();
    assert_eq!(
        delays,
        [(100, 1000), (250, 1000), (60_000, 1000)],
        "milliseconds, exactly"
    );
    for (f, (_, cells)) in r.frames.iter().zip(&frames) {
        assert_eq!(
            &f.cells, cells,
            "every cell exactly, partial alpha and hidden colour too"
        );
        assert_eq!((f.dispose, f.blend), (0, 0));
    }
}

#[test]
fn a_frame_replaces_what_it_covers_so_clear_pixels_show_nothing_before() {
    let (w, h) = (4, 3);
    let solid = vec![0xff12_3456; w * h];
    let clear = vec![0u32; w * h];
    let corner = Area {
        x: 1,
        y: 1,
        width: 2,
        height: 1,
    };
    let frames = [
        (kept(whole(w, h), 40), solid.clone()),
        (kept(whole(w, h), 40), clear.clone()),
        (kept(corner, 40), vec![0x80ff_0000, 0x0000_00ff]),
    ];
    let r = read(&apng((w, h), &frames, (2, 1)));
    let shown = played(&r, 2);
    let mut third = clear.clone();
    third[w + 1] = 0x80ff_0000;
    third[w + 2] = 0x0000_00ff;
    let once = vec![solid, clear, third];
    assert_eq!(shown, [once.clone(), once].concat());
    assert_eq!(r.plays, 2);
}

#[test]
fn rows_may_arrive_in_any_number_at_a_time() {
    let (w, h) = (9, 37);
    let frames = [
        (kept(whole(w, h), 10), varied(w, h, 7)),
        (kept(whole(w, h), 10), varied(w, h, 8)),
    ];
    let one = apng((w, h), &frames, (1, 1));
    for band in [2, 16, 36, 37] {
        assert_eq!(
            apng((w, h), &frames, (1, band)),
            one,
            "{band} rows at a time"
        );
    }
}

#[test]
fn the_default_image_is_the_png_of_the_first_frame() {
    let (w, h) = (6, 5);
    let first = varied(w, h, 4);
    let a = apng((w, h), &[(kept(whole(w, h), 10), first.clone())], (1, 2));
    let mut png = PngStream::new(Vec::new(), w, h).expect("png");
    png.rows(&first).expect("rows");
    let png = png.finish().expect("png");
    let data = |b: &[u8]| -> Vec<u8> {
        read_chunks(b)
            .into_iter()
            .filter(|c| &c.0 == b"IDAT")
            .flat_map(|c| c.1)
            .collect()
    };
    assert_eq!(data(&a), data(&png), "the same compressed rows");
    let decoded = crate::png::decode(&a).expect("a PNG reader that skips APNG chunks reads it");
    let cleared: Vec<u32> = first
        .iter()
        .map(|&c| if c >> 24 == 0 { 0 } else { c })
        .collect();
    assert_eq!(
        decoded.px, cleared,
        "the app's reader makes clear pixels (0, 0, 0, 0)"
    );
}

/// Chunk kinds and bodies, unchecked; `read` checks them.
fn read_chunks(b: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let (mut at, mut out) = (8, Vec::new());
    while at < b.len() {
        let len = u32::from_be_bytes(b[at..at + 4].try_into().expect("len")) as usize;
        let kind = b[at + 4..at + 8].try_into().expect("kind");
        out.push((kind, b[at + 8..at + 8 + len].to_vec()));
        at += 12 + len;
    }
    out
}

#[test]
fn large_frames_split_their_data_and_keep_the_count() {
    let (w, h) = (700, 600);
    let frames = [
        (kept(whole(w, h), 10), varied(w, h, 1)),
        (kept(whole(w, h), 10), varied(w, h, 2)),
    ];
    let bytes = apng((w, h), &frames, (0, 16));
    let r = read(&bytes);
    let fdat = read_chunks(&bytes)
        .into_iter()
        .filter(|c| &c.0 == b"fdAT")
        .count();
    assert!(fdat > 1, "the test needs a frame of more than one chunk");
    assert!(read_chunks(&bytes)
        .iter()
        .all(|c| c.1.len() <= IDAT_MAX + 4));
    assert_eq!(r.frames[1].cells, frames[1].1);
}

#[test]
fn what_is_not_an_animation_is_refused() {
    assert!(
        ApngStream::new(Vec::new(), (0, 1), 1, 0).is_err(),
        "no width"
    );
    assert!(
        ApngStream::new(Vec::new(), (1, 0), 1, 0).is_err(),
        "no height"
    );
    assert!(
        ApngStream::new(Vec::new(), (1, 1), 0, 0).is_err(),
        "no frames"
    );
    assert!(
        ApngStream::new(Vec::new(), (1, 1), 1, 1 << 31).is_err(),
        "plays a PNG cannot hold"
    );
    let mut a = ApngStream::new(Vec::new(), (4, 4), 2, 0).expect("starts");
    let part = Area {
        x: 1,
        y: 1,
        width: 2,
        height: 2,
    };
    assert!(
        a.frame(kept(part, 10)).is_err(),
        "the first frame is the whole image"
    );
    let mut f = a.frame(kept(whole(4, 4), 10)).expect("whole");
    assert!(f.rows(&[0; 3]).is_err(), "part of a row");
    f.rows(&[0; 12]).expect("three rows");
    assert!(f.rows(&[0; 8]).is_err(), "a row too many");
    assert!(f.end().is_err(), "a row missing");
    assert!(a.frame(kept(part, 10)).is_err(), "a frame left unfinished");
}

#[test]
fn frames_outside_the_image_or_past_the_count_are_refused() {
    let mut a = ApngStream::new(Vec::new(), (4, 4), 2, 0).expect("starts");
    let mut f = a.frame(kept(whole(4, 4), 10)).expect("whole");
    f.rows(&[0; 16]).expect("rows");
    f.end().expect("ends");
    for area in [
        Area {
            x: 3,
            y: 0,
            width: 2,
            height: 1,
        },
        Area {
            x: 0,
            y: 4,
            width: 1,
            height: 1,
        },
        Area {
            x: usize::MAX,
            y: 0,
            width: 2,
            height: 1,
        },
        Area {
            x: 0,
            y: 0,
            width: 0,
            height: 1,
        },
        Area {
            x: 0,
            y: 0,
            width: 1,
            height: 0,
        },
    ] {
        assert!(a.frame(kept(area, 10)).is_err(), "{area:?}");
    }
    let mut f = a
        .frame(kept(
            Area {
                x: 3,
                y: 3,
                width: 1,
                height: 1,
            },
            10,
        ))
        .expect("corner");
    f.rows(&[0]).expect("row");
    f.end().expect("ends");
    assert!(
        a.frame(kept(whole(4, 4), 10)).is_err(),
        "a frame more than announced"
    );
    a.finish().expect("finishes");
    let mut early = ApngStream::new(Vec::new(), (1, 1), 2, 0).expect("starts");
    let mut f = early.frame(kept(whole(1, 1), 10)).expect("whole");
    f.rows(&[0]).expect("row");
    f.end().expect("ends");
    assert!(early.finish().is_err(), "a frame missing");
}

#[test]
fn every_dispose_and_blend_is_written_as_given() {
    let (w, h) = (3, 2);
    let small = Area {
        x: 1,
        y: 0,
        width: 1,
        height: 2,
    };
    let mut frames = vec![(kept(whole(w, h), 10), vec![0xff00_ff00; 6])];
    for (after, blend) in [
        (Dispose::Clear, Blend::Over),
        (Dispose::Previous, Blend::Source),
        (Dispose::Keep, Blend::Over),
    ] {
        frames.push((
            Control {
                area: small,
                delay: (10, 1000),
                after,
                blend,
            },
            vec![0x8000_00ff, 0xff00_00ff],
        ));
    }
    let r = read(&apng((w, h), &frames, (1, 1)));
    let ops: Vec<_> = r.frames.iter().map(|f| (f.dispose, f.blend)).collect();
    assert_eq!(ops, [(0, 0), (1, 1), (2, 0), (0, 1)]);
    let shown = played(&r, 1);
    assert_eq!(shown[2][1], 0x8000_00ff, "SOURCE after a cleared area");
    assert_eq!(
        shown[3][1], 0x8000_00ff,
        "OVER a restored clear pixel is the pixel"
    );
    assert_eq!(shown[3][4], 0xff00_00ff);
}
