//! A GIF reader for tests only, written from the GIF89a specification: the classic
//! table-of-strings LZW decoder, not the encoder's hash table. Both crates' tests use it; it
//! is never built into a program. ffmpeg and Pillow check the same files outside the build.

// Each crate's tests read the fields they check.
#![allow(dead_code)]

/// What a test reads back from a GIF.
#[derive(Debug, Default)]
pub struct Read {
    pub size: (usize, usize),
    /// The global table as straight RGB.
    pub table: Vec<[u8; 3]>,
    pub background: u8,
    /// The loop extension's repeat count, if there is one.
    pub repeats: Option<u16>,
    pub frames: Vec<Frame>,
}

#[derive(Debug)]
pub struct Frame {
    pub delay: u16,
    pub disposal: u8,
    pub transparent: Option<u8>,
    pub bounds: (usize, usize, usize, usize),
    /// Bits an index has as LZW reads it.
    pub min: u8,
    pub indices: Vec<u8>,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

/// The bytes of the sub-blocks starting at `at`, and where they end.
fn sub_blocks(b: &[u8], mut at: usize) -> (Vec<u8>, usize) {
    let mut data = Vec::new();
    while b[at] != 0 {
        let len = b[at] as usize;
        data.extend_from_slice(&b[at + 1..at + 1 + len]);
        at += 1 + len;
    }
    (data, at + 1)
}

fn lzw_decode(min: u8, data: &[u8]) -> Vec<u8> {
    let clear = 1usize << min;
    let reset = || -> Vec<Vec<u8>> {
        let mut d: Vec<Vec<u8>> = (0..clear).map(|i| vec![i as u8]).collect();
        d.push(Vec::new());
        d.push(Vec::new());
        d
    };
    let (mut dict, mut width, mut prev): (Vec<Vec<u8>>, u32, Option<Vec<u8>>) =
        (reset(), u32::from(min) + 1, None);
    let (mut bit, mut out) = (0usize, Vec::new());
    loop {
        assert!(
            bit + width as usize <= data.len() * 8,
            "ran out before the end code"
        );
        let mut code = 0usize;
        for i in 0..width as usize {
            code |= ((data[(bit + i) / 8] >> ((bit + i) % 8)) as usize & 1) << i;
        }
        bit += width as usize;
        if code == clear {
            (dict, width, prev) = (reset(), u32::from(min) + 1, None);
            continue;
        }
        if code == clear + 1 {
            return out;
        }
        let entry = match (dict.get(code), &prev) {
            (Some(e), _) if code != clear + 1 => e.clone(),
            (None, Some(p)) if code == dict.len() => [p.clone(), vec![p[0]]].concat(),
            _ => panic!("code {code} with {} known", dict.len()),
        };
        out.extend_from_slice(&entry);
        if let Some(p) = prev {
            if dict.len() < 4096 {
                dict.push([p, vec![entry[0]]].concat());
            }
        }
        prev = Some(entry);
        if dict.len() == 1 << width && width < 12 {
            width += 1;
        }
    }
}

pub fn read(b: &[u8]) -> Read {
    assert_eq!(&b[..6], b"GIF89a");
    let mut r = Read {
        size: (u16_at(b, 6) as usize, u16_at(b, 8) as usize),
        background: b[11],
        ..Read::default()
    };
    assert_eq!(b[10] & 0x80, 0x80, "a global table");
    let entries = 1 << ((b[10] & 7) + 1);
    r.table = (0..entries)
        .map(|i| [b[13 + 3 * i], b[14 + 3 * i], b[15 + 3 * i]])
        .collect();
    let mut at = 13 + 3 * entries;
    let mut control = None;
    loop {
        match b[at] {
            0x3b => {
                assert_eq!(at + 1, b.len(), "nothing after the trailer");
                return r;
            }
            0x21 if b[at + 1] == 0xf9 => {
                assert_eq!(b[at + 2], 4);
                let packed = b[at + 3];
                let clear = (packed & 1 == 1).then_some(b[at + 6]);
                control = Some((u16_at(b, at + 4), (packed >> 2) & 7, clear));
                assert_eq!(b[at + 7], 0);
                at += 8;
            }
            0x21 if b[at + 1] == 0xff => {
                assert_eq!(&b[at + 2..at + 14], b"\x0bNETSCAPE2.0");
                assert_eq!(&b[at + 14..at + 16], b"\x03\x01");
                r.repeats = Some(u16_at(b, at + 16));
                assert_eq!(b[at + 18], 0);
                at += 19;
            }
            0x2c => {
                let bounds = (
                    u16_at(b, at + 1) as usize,
                    u16_at(b, at + 3) as usize,
                    u16_at(b, at + 5) as usize,
                    u16_at(b, at + 7) as usize,
                );
                assert_eq!(b[at + 9], 0, "no local table, not interlaced");
                let min = b[at + 10];
                let (data, end) = sub_blocks(b, at + 11);
                let (delay, disposal, transparent) = control.take().expect("a control block");
                r.frames.push(Frame {
                    delay,
                    disposal,
                    transparent,
                    bounds,
                    min,
                    indices: lzw_decode(min, &data),
                });
                at = end;
            }
            other => panic!("block {other:#x} at {at}"),
        }
    }
}

/// Every frame as straight RGBA, as a viewer shows it: the canvas starts clear, a frame's clear
/// index leaves what is beneath, and disposal 2 clears the frame's area before the next. A
/// clear pixel reads as (0, 0, 0, 0).
pub fn shown(r: &Read) -> Vec<Vec<u32>> {
    played(r, 1)
}

/// Every frame of `rounds` plays, as a viewer that does not clear the canvas between plays
/// shows them: each play starts from what the last frame of the one before left.
pub fn played(r: &Read, rounds: usize) -> Vec<Vec<u32>> {
    let (w, h) = r.size;
    let mut canvas = vec![0u32; w * h];
    let mut out = Vec::new();
    for f in r.frames.iter().cycle().take(r.frames.len() * rounds) {
        let (fx, fy, fw, _) = f.bounds;
        for (i, &index) in f.indices.iter().enumerate() {
            if Some(index) == f.transparent {
                continue;
            }
            let [red, green, blue] = r.table[index as usize];
            let (x, y) = (fx + i % fw, fy + i / fw);
            canvas[y * w + x] = u32::from_le_bytes([red, green, blue, 255]);
        }
        out.push(canvas.clone());
        match f.disposal {
            2 => {
                for i in 0..f.indices.len() {
                    canvas[(fy + i / fw) * w + fx + i % fw] = 0;
                }
            }
            0 | 1 => {}
            other => panic!("disposal {other} is not used"),
        }
    }
    out
}
