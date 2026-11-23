//! Builds Aseprite files byte by byte from the public specification, for tests. Nothing here
//! reads a file, so a reader bug cannot hide in a builder that shares its code. Also included
//! by the robustness test in `tests/`, which is why it names the crate by its own name.

#![allow(dead_code)]

use pikslar_format::deflate::deflate;

fn u16s(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn u32s(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn string(out: &mut Vec<u8>, s: &str) {
    u16s(out, s.len() as u16);
    out.extend_from_slice(s.as_bytes());
}

/// A whole file: the header fields a test varies, and the frames.
pub struct Ase {
    pub width: u16,
    pub height: u16,
    pub depth: u16,
    pub flags: u32,
    pub speed: u16,
    pub transparent: u8,
    pub ratio: (u8, u8),
    pub frames: Vec<(u16, Vec<Vec<u8>>)>,
}

impl Ase {
    pub fn new(width: u16, height: u16, depth: u16) -> Self {
        Ase {
            width,
            height,
            depth,
            flags: 0,
            speed: 100,
            transparent: 0,
            ratio: (1, 1),
            frames: Vec::new(),
        }
    }

    /// Adds a frame of this length holding these chunks, each as [`chunk`] makes it.
    pub fn frame(mut self, duration: u16, chunks: Vec<Vec<u8>>) -> Self {
        self.frames.push((duration, chunks));
        self
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut body = Vec::new();
        for (duration, chunks) in &self.frames {
            let inside: Vec<u8> = chunks.concat();
            u32s(&mut body, (16 + inside.len()) as u32);
            u16s(&mut body, 0xF1FA);
            u16s(&mut body, chunks.len() as u16);
            u16s(&mut body, *duration);
            body.extend_from_slice(&[0, 0]);
            u32s(&mut body, chunks.len() as u32);
            body.extend_from_slice(&inside);
        }
        let mut out = Vec::new();
        u32s(&mut out, (128 + body.len()) as u32);
        u16s(&mut out, 0xA5E0);
        u16s(&mut out, self.frames.len() as u16);
        u16s(&mut out, self.width);
        u16s(&mut out, self.height);
        u16s(&mut out, self.depth);
        u32s(&mut out, self.flags);
        u16s(&mut out, self.speed);
        out.extend_from_slice(&[0; 8]);
        out.push(self.transparent);
        out.extend_from_slice(&[0; 3]);
        u16s(&mut out, 0);
        out.extend_from_slice(&[self.ratio.0, self.ratio.1]);
        out.extend_from_slice(&[0; 8]);
        out.resize(128, 0);
        out.extend_from_slice(&body);
        out
    }
}

/// A chunk: its size (counting itself and its type), its type, and its data.
pub fn chunk(kind: u16, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    u32s(&mut out, (6 + data.len()) as u32);
    u16s(&mut out, kind);
    out.extend_from_slice(data);
    out
}

/// A layer chunk. `kind` is 0 for an image, 1 for a group, 2 for a tilemap.
pub fn layer(flags: u16, kind: u16, level: u16, blend: u16, opacity: u8, name: &str) -> Vec<u8> {
    let mut d = Vec::new();
    for v in [flags, kind, level, 0, 0, blend] {
        u16s(&mut d, v);
    }
    d.push(opacity);
    d.extend_from_slice(&[0; 3]);
    string(&mut d, name);
    if kind == 2 {
        u32s(&mut d, 0);
    }
    chunk(0x2004, &d)
}

/// An ordinary visible, editable layer.
pub fn plain(name: &str) -> Vec<u8> {
    layer(3, 0, 0, 0, 255, name)
}

/// A cel chunk of any type, with `payload` after the fixed fields.
pub fn cel(layer: u16, x: i16, y: i16, opacity: u8, kind: u16, z: i16, payload: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    u16s(&mut d, layer);
    d.extend_from_slice(&x.to_le_bytes());
    d.extend_from_slice(&y.to_le_bytes());
    d.push(opacity);
    u16s(&mut d, kind);
    d.extend_from_slice(&z.to_le_bytes());
    d.extend_from_slice(&[0; 5]);
    d.extend_from_slice(payload);
    chunk(0x2005, &d)
}

fn sized(w: u16, h: u16, pixels: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    u16s(&mut p, w);
    u16s(&mut p, h);
    p.extend_from_slice(pixels);
    p
}

/// A raw cel (type 0) at (`x`, `y`).
pub fn cel_raw(layer: u16, x: i16, y: i16, w: u16, h: u16, pixels: &[u8]) -> Vec<u8> {
    cel(layer, x, y, 255, 0, 0, &sized(w, h, pixels))
}

/// A zlib cel (type 2) at (`x`, `y`).
pub fn cel_zlib(layer: u16, x: i16, y: i16, w: u16, h: u16, pixels: &[u8]) -> Vec<u8> {
    cel(layer, x, y, 255, 2, 0, &sized(w, h, &zlib(pixels)))
}

/// A linked cel (type 1) showing the drawing of `frame`.
pub fn cel_linked(layer: u16, frame: u16) -> Vec<u8> {
    cel(layer, 0, 0, 255, 1, 0, &frame.to_le_bytes())
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    b << 16 | a
}

/// A zlib stream of the existing compressor's output.
pub fn zlib(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x9c];
    out.extend_from_slice(&deflate(data));
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// A zlib stream of one stored (uncompressed) block.
pub fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01, 1];
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(&(!(data.len() as u16)).to_le_bytes());
    out.extend_from_slice(data);
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// A new palette chunk (0x2019) of `size` colors, with `entries` from color `first`.
pub fn palette(size: u32, first: u32, entries: &[[u8; 4]]) -> Vec<u8> {
    let mut d = Vec::new();
    u32s(&mut d, size);
    u32s(&mut d, first);
    u32s(&mut d, first + entries.len() as u32 - 1);
    d.extend_from_slice(&[0; 8]);
    for (i, e) in entries.iter().enumerate() {
        // Every other entry has a name, so both shapes are read.
        u16s(&mut d, (i % 2) as u16);
        d.extend_from_slice(e);
        if i % 2 == 1 {
            string(&mut d, "named");
        }
    }
    chunk(0x2019, &d)
}

/// An old palette chunk, 0x0004 or 0x0011, from (skip, colors) packets.
pub fn old_palette(kind: u16, packets: &[(u8, Vec<[u8; 3]>)]) -> Vec<u8> {
    let mut d = Vec::new();
    u16s(&mut d, packets.len() as u16);
    for (skip, colors) in packets {
        d.push(*skip);
        d.push(colors.len() as u8);
        colors.iter().for_each(|c| d.extend_from_slice(c));
    }
    chunk(kind, &d)
}

/// A tags chunk from (from, to, direction, repeat, name).
pub fn tags(list: &[(u16, u16, u8, u16, &str)]) -> Vec<u8> {
    let mut d = Vec::new();
    u16s(&mut d, list.len() as u16);
    d.extend_from_slice(&[0; 8]);
    for &(from, to, direction, repeat, name) in list {
        u16s(&mut d, from);
        u16s(&mut d, to);
        d.push(direction);
        u16s(&mut d, repeat);
        d.extend_from_slice(&[0; 6 + 3 + 1]);
        string(&mut d, name);
    }
    chunk(0x2018, &d)
}

/// RGBA bytes of a `w` by `h` picture whose pixel (x, y) is distinct and opaque.
pub fn rgba_picture(w: usize, h: usize, seed: u8) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = ((i % w) as u8, (i / w) as u8);
            [
                seed.wrapping_add(i as u8),
                y.wrapping_mul(9),
                x.wrapping_mul(7),
                255,
            ]
        })
        .collect()
}

/// A file with every supported thing in it, for the robustness test to damage: groups, three
/// frames, raw, zlib and linked cels, a palette, tags and chunks that are skipped.
pub fn rich() -> Vec<u8> {
    let first = vec![
        layer(3, 0, 0, 0, 255, "base"),
        layer(3, 1, 0, 0, 255, "group"),
        layer(7, 0, 1, 1, 128, "inner"),
        layer(1, 0, 1, 6, 255, "top"),
        palette(3, 0, &[[1, 2, 3, 255], [4, 5, 6, 255], [7, 8, 9, 0]]),
        tags(&[(0, 1, 2, 3, "walk"), (1, 2, 0, 0, "idle")]),
        chunk(0x2007, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        cel_zlib(0, 0, 0, 20, 18, &rgba_picture(20, 18, 1)),
        cel_raw(2, -3, 2, 4, 3, &rgba_picture(4, 3, 9)),
        cel_zlib(3, 5, 5, 40, 2, &rgba_picture(40, 2, 3)),
    ];
    let second = vec![
        cel_linked(0, 0),
        cel_zlib(2, 0, 0, 3, 3, &rgba_picture(3, 3, 5)),
        chunk(0x2020, &[1, 0, 0, 0, 2, 0, b'h', b'i']),
    ];
    let third = vec![
        cel_linked(0, 1),
        cel_linked(3, 0),
        slice(
            "s",
            &[SliceKey {
                frame: 0,
                bounds: (1, 1, 4, 4),
                center: Some((1, 1, 2, 2)),
                pivot: Some((0, 0)),
            }],
        ),
    ];
    let mut ase = Ase::new(32, 24, 32);
    ase.flags = 3;
    ase.frame(100, first)
        .frame(0, second)
        .frame(70, third)
        .bytes()
}

/// An indexed file with an old and a new palette, a background layer and a link.
pub fn rich_indexed() -> Vec<u8> {
    let indices: Vec<u8> = (0..36u8).map(|i| i % 5).collect();
    let first = vec![
        layer(11, 0, 0, 0, 255, "background"),
        layer(3, 0, 0, 0, 255, "ink"),
        old_palette(0x0004, &[(0, vec![[1, 2, 3]; 3]), (2, vec![[9, 9, 9]])]),
        old_palette(0x0011, &[(0, vec![[63, 0, 31]; 2])]),
        palette(5, 0, &[[0, 0, 0, 255], [255, 0, 0, 255], [0, 255, 0, 255]]),
        cel_zlib(0, 0, 0, 6, 6, &indices),
        cel_raw(1, 1, 1, 6, 6, &indices),
    ];
    let second = vec![cel_linked(0, 0), cel_linked(1, 0)];
    let mut ase = Ase::new(8, 8, 8);
    ase.transparent = 2;
    ase.frame(90, first).frame(90, second).bytes()
}

/// One key of a slice chunk: its frame, bounds, an optional nine-slice centre and pivot.
pub struct SliceKey {
    pub frame: u32,
    pub bounds: (i32, i32, u32, u32),
    pub center: Option<(i32, i32, u32, u32)>,
    pub pivot: Option<(i32, i32)>,
}

/// A slice chunk (0x2022). The flags say whether the keys carry a centre and a pivot, so every
/// key should have both or neither, as a real file does.
pub fn slice(name: &str, keys: &[SliceKey]) -> Vec<u8> {
    let nine = keys.iter().any(|k| k.center.is_some());
    let pivot = keys.iter().any(|k| k.pivot.is_some());
    let mut d = Vec::new();
    u32s(&mut d, keys.len() as u32);
    u32s(&mut d, u32::from(nine) | u32::from(pivot) << 1);
    u32s(&mut d, 0);
    string(&mut d, name);
    let i32s = |d: &mut Vec<u8>, v: i32| d.extend_from_slice(&v.to_le_bytes());
    for k in keys {
        u32s(&mut d, k.frame);
        i32s(&mut d, k.bounds.0);
        i32s(&mut d, k.bounds.1);
        u32s(&mut d, k.bounds.2);
        u32s(&mut d, k.bounds.3);
        if let Some(c) = k.center.filter(|_| nine) {
            i32s(&mut d, c.0);
            i32s(&mut d, c.1);
            u32s(&mut d, c.2);
            u32s(&mut d, c.3);
        }
        if let Some(p) = k.pivot.filter(|_| pivot) {
            i32s(&mut d, p.0);
            i32s(&mut d, p.1);
        }
    }
    chunk(0x2022, &d)
}

/// The payload of a raw cel: its size, then its pixels, for [`cel`].
pub fn sized_payload(w: u16, h: u16, pixels: &[u8]) -> Vec<u8> {
    sized(w, h, pixels)
}

/// A tilemap layer chunk naming the tileset the file numbers `tileset`.
pub fn tilemap_layer(flags: u16, level: u16, name: &str, tileset: u32) -> Vec<u8> {
    let mut d = Vec::new();
    for v in [flags, 2, level, 0, 0, 0] {
        u16s(&mut d, v);
    }
    d.push(255);
    d.extend_from_slice(&[0; 3]);
    string(&mut d, name);
    u32s(&mut d, tileset);
    chunk(0x2004, &d)
}

/// The fixed part of a tileset chunk: number, flags, tile count, tile size and name.
fn tileset_head(id: u32, flags: u32, count: u32, (w, h): (u16, u16), name: &str) -> Vec<u8> {
    let mut d = Vec::new();
    u32s(&mut d, id);
    u32s(&mut d, flags);
    u32s(&mut d, count);
    u16s(&mut d, w);
    u16s(&mut d, h);
    d.extend_from_slice(&1i16.to_le_bytes());
    d.extend_from_slice(&[0; 14]);
    string(&mut d, name);
    d
}

/// A tileset chunk with its tiles inside, as `pixels` (all the tiles one under another, in the
/// file's color depth) packed. `flags` is the tileset flags without the one for inside tiles.
pub fn tileset_chunk(
    id: u32,
    flags: u32,
    count: u32,
    size: (u16, u16),
    name: &str,
    pixels: &[u8],
) -> Vec<u8> {
    tileset_packed(id, flags, count, size, name, &zlib(pixels))
}

/// As [`tileset_chunk`] with the packed stream given as it is, so that it may be wrong.
pub fn tileset_packed(
    id: u32,
    flags: u32,
    count: u32,
    size: (u16, u16),
    name: &str,
    stream: &[u8],
) -> Vec<u8> {
    let mut d = tileset_head(id, flags | 2, count, size, name);
    u32s(&mut d, stream.len() as u32);
    d.extend_from_slice(stream);
    chunk(0x2023, &d)
}

/// A tileset chunk that names a tileset in another file and holds no tiles.
pub fn tileset_external(id: u32, count: u32, size: (u16, u16), name: &str) -> Vec<u8> {
    let mut d = tileset_head(id, 1 | 4, count, size, name);
    u32s(&mut d, 1);
    u32s(&mut d, 0);
    chunk(0x2023, &d)
}

/// The masks of a tilemap cel: tile number, x flip, y flip, diagonal flip.
pub const MASKS_NEW: [u32; 4] = [0x1fff_ffff, 0x8000_0000, 0x4000_0000, 0x2000_0000];
/// The masks the older files in use have: the x flip and the diagonal flip trade places.
pub const MASKS_OLD: [u32; 4] = [0x1fff_ffff, 0x2000_0000, 0x4000_0000, 0x8000_0000];

/// A tilemap cel (type 3) of `w` by `h` tiles at (`x`, `y`): `bits` a tile (8, 16 or 32), the
/// four masks, and the values.
pub fn cel_tilemap(
    layer: u16,
    (x, y): (i16, i16),
    (w, h): (u16, u16),
    bits: u16,
    masks: [u32; 4],
    values: &[u32],
) -> Vec<u8> {
    let step = usize::from(bits / 8);
    let raw: Vec<u8> = values
        .iter()
        .flat_map(|v| v.to_le_bytes()[..step].to_vec())
        .collect();
    cel_tilemap_packed(layer, (x, y), (w, h), bits, masks, &zlib(&raw))
}

/// As [`cel_tilemap`] with the packed stream given as it is.
pub fn cel_tilemap_packed(
    layer: u16,
    (x, y): (i16, i16),
    (w, h): (u16, u16),
    bits: u16,
    masks: [u32; 4],
    stream: &[u8],
) -> Vec<u8> {
    let mut p = Vec::new();
    u16s(&mut p, w);
    u16s(&mut p, h);
    u16s(&mut p, bits);
    masks.iter().for_each(|&m| u32s(&mut p, m));
    p.extend_from_slice(&[0; 10]);
    p.extend_from_slice(stream);
    cel(layer, x, y, 255, 3, 0, &p)
}

/// A file with every kind of tilemap thing in it, for the robustness test to damage: two
/// tilesets (one in the old numbering), one in another file, maps of 8, 16 and 32 bits, flips,
/// a linked map cel and a map in a second frame.
pub fn rich_tiles() -> Vec<u8> {
    let tiles = |count: usize, w: usize, h: usize| -> Vec<u8> {
        (0..count * w * h)
            .flat_map(|i| [(i / (w * h) * 40) as u8, (i % w) as u8, (i / w) as u8, 255])
            .collect()
    };
    let [id, x, y, d] = MASKS_NEW;
    let first = vec![
        layer(3, 0, 0, 0, 255, "base"),
        tileset_chunk(3, 4, 4, (4, 4), "new", &tiles(4, 4, 4)),
        tileset_chunk(5, 0, 3, (2, 3), "old", &tiles(3, 2, 3)),
        tileset_external(8, 6, (4, 4), "elsewhere"),
        tilemap_layer(3, 0, "map a", 3),
        tilemap_layer(7, 0, "map b", 5),
        tilemap_layer(3, 0, "map c", 3),
        tilemap_layer(3, 0, "ext", 8),
        cel_zlib(0, 0, 0, 8, 8, &rgba_picture(8, 8, 2)),
        cel_tilemap(
            1,
            (1, 1),
            (3, 2),
            32,
            MASKS_NEW,
            &[1, 2 | x, 3 | y, 0, 1 | d, 2 | x | y | d],
        ),
        cel_tilemap(
            2,
            (0, 2),
            (2, 2),
            16,
            [0x3fff, 0x8000, 0x4000, 0x2000],
            &[0, 1, 2, 0xffff],
        ),
        cel_tilemap(
            3,
            (-2, 0),
            (2, 2),
            8,
            [0x3f, 0x80, 0x40, 0x20],
            &[3, 0x81, 0xc2, 1],
        ),
        cel_tilemap(4, (0, 0), (1, 1), 32, MASKS_NEW, &[1]),
    ];
    let second = vec![
        cel_linked(1, 0),
        cel_tilemap(2, (0, 0), (2, 1), 32, [id, y, x, d], &[1 | x, 2]),
    ];
    Ase::new(16, 12, 32)
        .frame(100, first)
        .frame(100, second)
        .bytes()
}

/// An indexed file with a tileset and two maps of it.
pub fn rich_tiles_indexed() -> Vec<u8> {
    let pixels: Vec<u8> = (0..3 * 4 * 4).map(|i| (i % 7) as u8).collect();
    let first = vec![
        palette(7, 0, &[[1, 2, 3, 255], [4, 5, 6, 255], [7, 8, 9, 255]]),
        tileset_chunk(0, 4, 3, (4, 4), "pal", &pixels),
        tilemap_layer(3, 0, "one", 0),
        tilemap_layer(3, 0, "two", 0),
        cel_tilemap(0, (0, 0), (2, 2), 32, MASKS_NEW, &[1, 2, 2, 1]),
        cel_tilemap(1, (4, 4), (1, 2), 32, MASKS_OLD, &[2, 1]),
    ];
    let second = vec![cel_linked(0, 0)];
    let mut ase = Ase::new(12, 12, 8);
    ase.transparent = 6;
    ase.frame(100, first).frame(100, second).bytes()
}
