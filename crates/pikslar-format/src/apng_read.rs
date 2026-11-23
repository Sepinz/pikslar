//! A strict APNG reader for tests only, written from the APNG specification. It panics on any
//! departure from it: a bad CRC, a sequence number out of order, a chunk in the wrong place,
//! data of the wrong length, a frame outside the image. Both crates' tests use it; it is never
//! built into a program. Pillow, ffmpeg and a browser check the same files outside the build.

// Each crate's tests read the fields they check.
#![allow(dead_code)]

use pikslar_format::inflate::inflate;

/// One frame as the file states it.
#[derive(Debug, Clone)]
pub struct Frame {
    /// x, y, width, height.
    pub area: (usize, usize, usize, usize),
    pub delay: (u16, u16),
    /// 0 none, 1 background, 2 previous.
    pub dispose: u8,
    /// 0 source, 1 over.
    pub blend: u8,
    /// The area's straight RGBA cells, as `0xAABBGGRR`.
    pub cells: Vec<u32>,
}

/// What a test reads back from an APNG.
#[derive(Debug, Default)]
pub struct Read {
    pub size: (usize, usize),
    pub plays: u32,
    pub frames: Vec<Frame>,
    /// Chunk kinds in file order.
    pub kinds: Vec<[u8; 4]>,
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// The CRC-32 of PNG, bit by bit, so it shares nothing with the writer's table.
fn crc(parts: &[&[u8]]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &byte in parts.iter().flat_map(|p| p.iter()) {
        c ^= byte as u32;
        for _ in 0..8 {
            c = if c & 1 == 1 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// Every chunk: its kind and body, each CRC checked.
fn chunks(bytes: &[u8]) -> Vec<([u8; 4], &[u8])> {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "signature");
    let (mut at, mut out) = (8, Vec::new());
    while at < bytes.len() {
        let len = be32(&bytes[at..]) as usize;
        assert!(len <= i32::MAX as usize, "chunk length");
        let kind: [u8; 4] = bytes[at + 4..at + 8].try_into().expect("kind");
        let body = &bytes[at + 8..at + 8 + len];
        let stated = be32(&bytes[at + 8 + len..]);
        assert_eq!(
            crc(&[&kind, body]),
            stated,
            "CRC of {}",
            String::from_utf8_lossy(&kind)
        );
        out.push((kind, body));
        at += 12 + len;
    }
    assert_eq!(at, bytes.len(), "the last chunk ends the file");
    out
}

/// A frame's rows from its zlib stream: filter 0, the Adler checksum checked.
fn cells(stream: &[u8], (w, h): (usize, usize)) -> Vec<u32> {
    assert_eq!(stream[0] & 0x0f, 8, "deflate");
    assert_eq!(
        (u16::from(stream[0]) << 8 | u16::from(stream[1])) % 31,
        0,
        "zlib header"
    );
    let size = h * (1 + 4 * w);
    let raw = inflate(&stream[2..], size).expect("inflates");
    assert_eq!(raw.len(), size, "a frame's data is its rows exactly");
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    assert_eq!(be32(&stream[stream.len() - 4..]), (b << 16) | a, "Adler-32");
    let mut out = Vec::with_capacity(w * h);
    for row in raw.chunks_exact(1 + 4 * w) {
        assert_eq!(row[0], 0, "this writer uses filter 0");
        out.extend(
            row[1..]
                .chunks_exact(4)
                .map(|p| u32::from_le_bytes([p[0], p[1], p[2], p[3]])),
        );
    }
    out
}

fn control(body: &[u8], size: (usize, usize)) -> Frame {
    assert_eq!(body.len(), 26, "fcTL length");
    let n = |i: usize| be32(&body[4 + 4 * i..]) as usize;
    let (w, h, x, y) = (n(0), n(1), n(2), n(3));
    assert!(
        w > 0 && h > 0 && x + w <= size.0 && y + h <= size.1,
        "frame inside the image"
    );
    let delay = (
        u16::from_be_bytes([body[20], body[21]]),
        u16::from_be_bytes([body[22], body[23]]),
    );
    assert!(body[24] <= 2 && body[25] <= 1, "dispose and blend");
    Frame {
        area: (x, y, w, h),
        delay,
        dispose: body[24],
        blend: body[25],
        cells: Vec::new(),
    }
}

/// Reads an APNG whose first frame is its default image.
pub fn read(bytes: &[u8]) -> Read {
    let all = chunks(bytes);
    let mut r = Read {
        kinds: all.iter().map(|c| c.0).collect(),
        ..Read::default()
    };
    assert_eq!(&all[0].0, b"IHDR");
    let ih = all[0].1;
    assert_eq!(ih.len(), 13);
    r.size = (be32(ih) as usize, be32(&ih[4..]) as usize);
    assert_eq!(&ih[8..], &[8, 6, 0, 0, 0], "8-bit RGBA, not interlaced");
    assert_eq!(&all[1].0, b"acTL", "acTL before any data");
    let declared = be32(all[1].1) as usize;
    r.plays = be32(&all[1].1[4..]);
    assert_eq!(&all.last().expect("chunks").0, b"IEND");
    let (mut sequence, mut stream, mut idat_seen) = (0u32, Vec::new(), false);
    let finish = |r: &mut Read, stream: &mut Vec<u8>| {
        if let Some(f) = r.frames.last_mut() {
            f.cells = cells(stream, (f.area.2, f.area.3));
        }
        stream.clear();
    };
    for (i, (kind, body)) in all.iter().enumerate().skip(2) {
        match kind {
            b"fcTL" | b"fdAT" => {
                assert_eq!(be32(body), sequence, "sequence number");
                sequence += 1;
            }
            _ => {}
        }
        match kind {
            b"fcTL" => {
                finish(&mut r, &mut stream);
                r.frames.push(control(body, r.size));
            }
            b"IDAT" => {
                assert_eq!(r.frames.len(), 1, "the default image is the first frame");
                assert!(
                    i == 3 || &all[i - 1].0 == b"IDAT",
                    "IDAT chunks are consecutive"
                );
                idat_seen = true;
                stream.extend_from_slice(body);
            }
            b"fdAT" => {
                assert!(
                    idat_seen && r.frames.len() > 1,
                    "fdAT after the default image"
                );
                stream.extend_from_slice(&body[4..]);
            }
            b"IEND" => finish(&mut r, &mut stream),
            other => panic!("unexpected chunk {}", String::from_utf8_lossy(other)),
        }
    }
    assert_eq!(r.frames.len(), declared, "acTL counts the frames");
    let first = r.frames.first().expect("a frame");
    assert_eq!(
        first.area,
        (0, 0, r.size.0, r.size.1),
        "the first frame is the whole image"
    );
    r
}

/// `top` drawn over `under`, both straight RGBA, as the specification's OVER.
fn over(top: u32, under: u32) -> u32 {
    let (ta, ua) = (top >> 24, under >> 24);
    match (ta, ua) {
        (255, _) | (_, 0) => top,
        (0, _) => under,
        _ => {
            let out_a = ta * 255 + ua * (255 - ta);
            let mix = |s: u32| {
                let (t, u) = ((top >> s) & 255, (under >> s) & 255);
                ((t * ta * 255 + u * ua * (255 - ta) + out_a / 2) / out_a) & 255
            };
            ((out_a + 127) / 255) << 24 | mix(16) << 16 | mix(8) << 8 | mix(0)
        }
    }
}

/// What a player shows after each frame, played `rounds` times, starting each play on a
/// transparent black canvas as the specification says.
pub fn played(r: &Read, rounds: usize) -> Vec<Vec<u32>> {
    let (w, h) = r.size;
    let mut shown = Vec::new();
    for _ in 0..rounds {
        let mut canvas = vec![0u32; w * h];
        for (i, f) in r.frames.iter().enumerate() {
            let (x, y, fw, fh) = f.area;
            let before = canvas.clone();
            for row in 0..fh {
                for col in 0..fw {
                    let at = (y + row) * w + x + col;
                    let top = f.cells[row * fw + col];
                    canvas[at] = if f.blend == 0 {
                        top
                    } else {
                        over(top, canvas[at])
                    };
                }
            }
            shown.push(canvas.clone());
            let dispose = if i == 0 && f.dispose == 2 {
                1
            } else {
                f.dispose
            };
            for row in 0..fh {
                for col in 0..fw {
                    let at = (y + row) * w + x + col;
                    match dispose {
                        1 => canvas[at] = 0,
                        2 => canvas[at] = before[at],
                        _ => {}
                    }
                }
            }
        }
    }
    shown
}

/// The cells of a plain 8-bit RGBA PNG exactly as written, colour behind clear pixels kept.
pub fn png_cells(bytes: &[u8]) -> ((usize, usize), Vec<u32>) {
    let all = chunks(bytes);
    assert_eq!(&all[0].0, b"IHDR");
    let ih = all[0].1;
    assert_eq!(&ih[8..], &[8, 6, 0, 0, 0], "8-bit RGBA, not interlaced");
    let size = (be32(ih) as usize, be32(&ih[4..]) as usize);
    let stream: Vec<u8> = all
        .iter()
        .filter(|c| &c.0 == b"IDAT")
        .flat_map(|c| c.1.iter().copied())
        .collect();
    (size, cells(&stream, size))
}
