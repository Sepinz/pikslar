//! Windows resources for the program file, written without `windres` or `rc.exe`.
//!
//! The GNU linker cannot read a `.res` file, and the Rust MinGW toolchain ships no resource
//! compiler, so this crate writes a COFF object itself, in the layout `windres -O coff` emits:
//! one `.rsrc` section holding the resource directory tree, the data entries and the data, with
//! an `IMAGE_REL_AMD64_ADDR32NB` relocation on each data entry's address. The build script of
//! `crates/pikslar` links it with `cargo:rustc-link-arg-bins`.
//!
//! Output is deterministic: no time stamps, and resources are sorted, so identical input gives
//! byte-identical objects. Used at build time and by tests only; nothing here ships.

mod bytes;
pub mod coff;
pub mod ico;
pub mod pe;
pub mod program;
pub mod read;
pub mod version;

pub use coff::object;

/// `RT_ICON`: one image of an icon.
pub const RT_ICON: u16 = 3;
/// `RT_GROUP_ICON`: the directory that lists an icon's images.
pub const RT_GROUP_ICON: u16 = 14;
/// `RT_VERSION`: the `VERSIONINFO` block.
pub const RT_VERSION: u16 = 16;
/// `RT_MANIFEST`: the application manifest; an exe's is id 1.
pub const RT_MANIFEST: u16 = 24;
/// English (United States), the language all of Pikslar's resources are written in.
pub const LANG_EN_US: u16 = 0x0409;

/// One resource: a type, a numeric id, a language and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    pub ty: u16,
    pub id: u16,
    pub lang: u16,
    pub data: Vec<u8>,
}

/// A COFF object with no sections and no symbols. Placed where gcc looks for
/// `default-manifest.o`, it stands in for the MinGW default manifest, which would otherwise
/// collide with the program's own (see docs in `crates/pikslar/build.rs`).
pub fn empty_object() -> Vec<u8> {
    let mut v = Vec::with_capacity(20);
    bytes::put16(&mut v, coff::MACHINE_AMD64);
    v.resize(20, 0);
    v
}
