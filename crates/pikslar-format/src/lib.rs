//! Reading and writing files. All input is treated as untrusted.

#[cfg(test)]
extern crate self as pikslar_format;

#[cfg(test)]
pub(crate) mod apng_read;
#[cfg(test)]
mod apng_tests;
pub mod apng_write;
mod ase_blending;
#[cfg(test)]
mod ase_build;
mod ase_cel;
mod ase_chunks;
#[cfg(test)]
mod ase_expand;
mod ase_names;
mod ase_palette;
pub mod ase_read;
#[cfg(test)]
mod ase_refused_tests;
mod ase_slice;
#[cfg(test)]
mod ase_tests;
mod ase_tileset;
pub mod atomic;
pub mod atomic_stream;
pub mod brush_lib;
pub mod deflate;
#[cfg(test)]
mod deflate_reference;
#[cfg(test)]
pub(crate) mod gif_read;
#[cfg(test)]
mod gif_tests;
pub mod gif_write;
pub mod inflate;
mod lzw;
pub mod palette_file;
pub mod piks;
mod piks_anim;
#[cfg(test)]
mod piks_anim_tests;
#[cfg(test)]
mod piks_blending_tests;
mod piks_doc_id;
mod piks_float;
#[cfg(test)]
mod piks_group_tests;
mod piks_ids;
mod piks_ids_hash;
mod piks_palette;
#[cfg(test)]
mod piks_palette_tests;
#[cfg(test)]
mod piks_rate_tests;
#[cfg(test)]
mod piks_reference_tests;
mod piks_sections;
#[cfg(test)]
mod piks_slice_tests;
mod piks_slices;
mod piks_terrain;
mod piks_tilesets;
mod piks_v3;
pub mod png;
pub mod png_index;
#[cfg(test)]
mod png_index_tests;
pub mod png_write;
pub mod publish;
pub mod publish_back;
pub mod publish_lock;
mod publish_retry;
pub mod room;
pub mod set_lock;
pub mod upgrade;

pub use ase_read::{import_ase, import_ase_until, AseWarning, Tally};
pub use atomic::write_atomic;
use std::fmt;

#[derive(Debug)]
pub enum FormatError {
    Io(std::io::Error),
    /// The file is damaged or not what it claims to be.
    Corrupt(&'static str),
    /// A valid file using a feature we do not read.
    Unsupported(&'static str),
    TooLarge,
    /// The file is bigger than can be opened, or than there is memory for.
    FileTooLarge,
    /// The read was asked to stop before it finished.
    Stopped,
    /// The tiles, each stored on its own, are more than a project file keeps.
    TilesTooLarge,
    /// The file's name leaves no room for the temporary file a save writes beside it; a name
    /// of at most `most` characters would (`room.rs`). Said before anything is written.
    NameTooLong {
        most: usize,
    },
    /// The file's whole path is past what Windows takes (`room.rs`). Said before anything is
    /// written.
    PathTooLong,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::Io(e) => write!(f, "Could not access the file: {e}"),
            FormatError::Corrupt(what) => write!(f, "This file is damaged: {what}"),
            FormatError::Unsupported(what) => write!(f, "Not supported yet: {what}"),
            FormatError::TooLarge => f.write_str("This image is larger than 8192 pixels"),
            FormatError::FileTooLarge => f.write_str("This file is too large to open"),
            FormatError::Stopped => f.write_str("Opening was stopped"),
            FormatError::TilesTooLarge => f.write_str(
                "The tiles take more than the 416 MiB a project file keeps: remove or reuse some tiles",
            ),
            FormatError::NameTooLong { most } => write!(
                f,
                "The file name is too long to save safely: Windows allows 255 characters in a \
                 name, and saving needs room in it for a temporary copy beside the file. Nothing \
                 was changed. Use Save As with a name of at most {most} characters"
            ),
            FormatError::PathTooLong => f.write_str(
                "The file's full path is longer than Windows allows (32,767 characters). Nothing \
                 was changed. Use Save As to choose a folder with a shorter path",
            ),
        }
    }
}

impl std::error::Error for FormatError {}

impl From<std::io::Error> for FormatError {
    fn from(e: std::io::Error) -> Self {
        FormatError::Io(e)
    }
}

pub(crate) fn crc32(parts: &[&[u8]]) -> u32 {
    const fn table() -> [u32; 256] {
        let mut t = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
                k += 1;
            }
            t[i] = c;
            i += 1;
        }
        t
    }
    static TABLE: [u32; 256] = table();
    let mut crc = !0u32;
    for byte in parts.iter().flat_map(|p| p.iter()) {
        crc = TABLE[((crc ^ *byte as u32) & 255) as usize] ^ (crc >> 8);
    }
    !crc
}

/// The largest file Pikslar will try to read. Section lengths are 32-bit.
pub const MAX_FILE_BYTES: u64 = 1 << 32;

/// Reads a whole file, refusing rather than aborting if it cannot fit in memory.
pub fn read_limited(path: &std::path::Path, limit: u64) -> Result<Vec<u8>, FormatError> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len > limit {
        return Err(FormatError::FileTooLarge);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len as usize)
        .map_err(|_| FormatError::FileTooLarge)?;
    // The take guards against a file that grows while it is being read.
    file.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_standard_check_value() {
        assert_eq!(crc32(&[b"123456789"]), 0xcbf4_3926);
        assert_eq!(crc32(&[b"12345", b"6789"]), 0xcbf4_3926);
    }

    #[test]
    fn reading_a_file_over_the_limit_is_refused() {
        let dir = std::env::temp_dir().join(format!("pikslar-read-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("a.bin");
        std::fs::write(&path, [1, 2, 3, 4]).expect("write");
        assert_eq!(read_limited(&path, 4).expect("fits"), vec![1, 2, 3, 4]);
        assert!(matches!(
            read_limited(&path, 3),
            Err(FormatError::FileTooLarge)
        ));
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp() {
        let dir = std::env::temp_dir().join(format!("pikslar-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("a.bin");
        write_atomic(&path, b"one").expect("first");
        write_atomic(&path, b"two").expect("second");
        assert_eq!(std::fs::read(&path).expect("read"), b"two");
        assert!(!atomic::temp_path(&path).exists());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
