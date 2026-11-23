//! Building project files by hand. Written apart from the format code, so a bug there cannot
//! hide itself here.

/// CRC-32, as used by the `.piks` trailer.
pub fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(!0u32, |mut crc, &byte| {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
        crc
    })
}

/// Recomputes the trailer, so whatever was changed is what the parser actually sees.
pub fn resealed(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.len() >= 4 {
        let end = bytes.len() - 4;
        let crc = crc32(&bytes[..end]);
        bytes[end..].copy_from_slice(&crc.to_le_bytes());
    }
    bytes
}

/// Adds sections to a project of version 2 or later that has none.
pub fn with_sections(plain: &[u8], sections: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let count_at = plain.len() - 8;
    let mut out = plain[..count_at].to_vec();
    out.extend_from_slice(&(sections.len() as u32).to_le_bytes());
    for (tag, data) in sections {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
    }
    out.extend_from_slice(&[0; 4]);
    resealed(out)
}

/// A tag section: `count`, then each tag's direction, range, name length and name.
pub fn tags_section(tags: &[(u8, u32, u32, &[u8])]) -> Vec<u8> {
    let mut out = (tags.len() as u32).to_le_bytes().to_vec();
    for (direction, from, to, name) in tags {
        out.push(*direction);
        out.extend_from_slice(&from.to_le_bytes());
        out.extend_from_slice(&to.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name);
    }
    out
}

/// A raw DEFLATE stream of stored blocks.
pub fn stored(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + 16);
    let mut blocks = raw.chunks(0xffff).peekable();
    while let Some(block) = blocks.next() {
        out.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
const ADAM7: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

fn channels(color: u8) -> usize {
    match color {
        2 => 3,
        4 => 2,
        6 => 4,
        _ => 1,
    }
}

pub fn stream_size(width: usize, height: usize, depth: u8, color: u8, interlaced: bool) -> usize {
    let bits = channels(color) * depth as usize;
    let rows = |w: usize, h: usize| ((w * bits).div_ceil(8) + 1) * h;
    if !interlaced {
        return rows(width, height);
    }
    ADAM7
        .iter()
        .map(|&(x0, y0, dx, dy)| {
            let w = width.saturating_sub(x0).div_ceil(dx);
            let h = height.saturating_sub(y0).div_ceil(dy);
            if w == 0 || h == 0 {
                0
            } else {
                rows(w, h)
            }
        })
        .sum()
}

/// One valid image in the given format.
pub fn png_seed(width: u32, height: u32, depth: u8, color: u8, interlaced: bool) -> Vec<u8> {
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[depth, color, 0, 0, u8::from(interlaced)]);
    let size = stream_size(width as usize, height as usize, depth, color, interlaced);
    // Every byte is below 5, so whichever bytes start a row are valid filters.
    let raw: Vec<u8> = (0..size).map(|i| (i % 5) as u8).collect();
    let mut data = vec![0x78, 0x01];
    data.extend_from_slice(&stored(&raw));
    data.extend_from_slice(&[0; 4]);
    let mut out = PNG_SIGNATURE.to_vec();
    png_chunk(&mut out, b"IHDR", &header);
    if color == 3 {
        let palette: Vec<u8> = (0..768).map(|i| (i * 5) as u8).collect();
        png_chunk(&mut out, b"PLTE", &palette);
        png_chunk(&mut out, b"tRNS", &[0, 128, 255]);
    }
    png_chunk(&mut out, b"IDAT", &data);
    png_chunk(&mut out, b"IEND", &[]);
    out
}

/// An RGBA image around an already compressed stream.
pub fn png_from_stream(width: u32, height: u32, interlaced: bool, stream: &[u8]) -> Vec<u8> {
    png_of_depth(width, height, 8, interlaced, stream)
}

/// As [`png_from_stream`], with `depth` bits a channel (8 or 16).
pub fn png_of_depth(
    width: u32,
    height: u32,
    depth: u8,
    interlaced: bool,
    stream: &[u8],
) -> Vec<u8> {
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[depth, 6, 0, 0, u8::from(interlaced)]);
    let mut data = vec![0x78, 0x01];
    data.extend_from_slice(stream);
    data.extend_from_slice(&[0; 4]);
    let mut out = PNG_SIGNATURE.to_vec();
    png_chunk(&mut out, b"IHDR", &header);
    png_chunk(&mut out, b"IDAT", &data);
    png_chunk(&mut out, b"IEND", &[]);
    out
}

/// Every color format PNG allows, plain and interlaced.
pub fn png_seeds() -> Vec<Vec<u8>> {
    const FORMATS: [(u8, u8); 15] = [
        (1, 0),
        (2, 0),
        (4, 0),
        (8, 0),
        (16, 0),
        (1, 3),
        (2, 3),
        (4, 3),
        (8, 3),
        (8, 2),
        (16, 2),
        (8, 4),
        (16, 4),
        (8, 6),
        (16, 6),
    ];
    FORMATS
        .iter()
        .flat_map(|&(depth, color)| {
            [false, true].map(|interlaced| png_seed(11, 7, depth, color, interlaced))
        })
        .collect()
}

/// Recomputes every chunk checksum that can be found, so damage reaches the decoder.
pub fn resealed_png(mut bytes: Vec<u8>) -> Vec<u8> {
    let mut pos = 8;
    while let Some(head) = bytes.get(pos..pos + 4) {
        let len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let Some(end) = (pos + 8).checked_add(len).filter(|e| e + 4 <= bytes.len()) else {
            break;
        };
        let crc = crc32(&bytes[pos + 4..end]);
        bytes[end..end + 4].copy_from_slice(&crc.to_be_bytes());
        pos = end + 4;
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn every_crafted_image_is_valid() {
        for seed in png_seeds() {
            let image = pikslar_format::png::decode(&seed).expect("a valid seed");
            assert_eq!((image.width, image.height), (11, 7));
        }
    }

    #[test]
    fn resealing_restores_a_damaged_image() {
        let mut bytes = png_seed(11, 7, 8, 6, false);
        let at = bytes.len() - 30;
        bytes[at] = u8::from(bytes[at] == 0);
        assert!(pikslar_format::png::decode(&bytes).is_err());
        assert!(pikslar_format::png::decode(&resealed_png(bytes)).is_ok());
    }

    #[test]
    fn resealing_leaves_short_input_alone() {
        assert_eq!(resealed(vec![1, 2]), vec![1, 2]);
    }
}
