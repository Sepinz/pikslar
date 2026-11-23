//! Embeds the program's Windows resources: the icon (`res/pikslar.ico`), version information
//! and the application manifest (`res/pikslar.manifest`).
//!
//! The GNU toolchain has no resource compiler that Pikslar can rely on, so `pikslar-res` writes
//! a COFF object with a `.rsrc` section and this script hands it to the linker for the `pikslar`
//! binary only (tests keep no resources).
//!
//! The MinGW-w64 gcc driver adds `default-manifest.o` to every link when that file exists in its
//! library folders (a WinLibs or MSYS2 gcc on `PATH` has one; rustup's own does not). Its
//! manifest has a non-zero language, so ld cannot drop it in favour of ours and reports
//! "multiple non-default manifests". gcc looks for the file in `-B` folders first, so an empty
//! object of that name in `OUT_DIR`, with `-B<OUT_DIR>/`, takes its place.
//!
//! Inputs from the environment: `PIKSLAR_COMMIT` (the commit hash, shown in the Comments field)
//! and `PIKSLAR_COPYRIGHT` (the copyright line; the owner decides its text).

use pikslar_res::{ico, program, version, Resource, LANG_EN_US, RT_MANIFEST, RT_VERSION};
use std::env;
use std::path::{Path, PathBuf};

/// The icon group's id; `pikslar-platform` loads this id for the window class.
const ICON_GROUP: u16 = 1;
/// The first image id of the icon group.
const ICON_FIRST_IMAGE: u16 = 1;
/// The manifest id Windows reads for an exe (`CREATEPROCESS_MANIFEST_RESOURCE_ID`).
const MANIFEST_ID: u16 = 1;
const ICON: &str = "res/pikslar.ico";
const MANIFEST: &str = "res/pikslar.manifest";

fn main() -> Result<(), String> {
    for path in ["build.rs", ICON, MANIFEST] {
        println!("cargo:rerun-if-changed={path}");
    }
    for var in program::ENV_INPUTS {
        println!("cargo:rerun-if-env-changed={var}");
    }
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" {
        return Ok(());
    }
    if target_env != "gnu" {
        println!("cargo:warning=Windows resources are only embedded for the GNU target");
        return Ok(());
    }
    let out = PathBuf::from(env::var("OUT_DIR").map_err(|e| format!("OUT_DIR: {e}"))?);
    let object = pikslar_res::object(&resources()?)?;
    let object_path = out.join("pikslar-resources.o");
    write(&object_path, &object)?;
    println!("cargo:rustc-link-arg-bins={}", object_path.display());
    program::replace_default_manifest(&out)
}

/// The icon images and group, the version block and the manifest.
fn resources() -> Result<Vec<Resource>, String> {
    let icon = read(ICON)?;
    let mut all = ico::resources(&icon, ICON_GROUP, ICON_FIRST_IMAGE, LANG_EN_US)
        .map_err(|e| format!("{ICON}: {e}"))?;
    let manifest = read(MANIFEST)?;
    std::str::from_utf8(&manifest).map_err(|e| format!("{MANIFEST} is not UTF-8: {e}"))?;
    all.push(resource(RT_MANIFEST, MANIFEST_ID, manifest));
    let info = program::version_info("pikslar.exe", "Pikslar pixel-art editor")?;
    all.push(resource(RT_VERSION, 1, version::block(&info)));
    Ok(all)
}

fn resource(ty: u16, id: u16, data: Vec<u8>) -> Resource {
    Resource {
        ty,
        id,
        lang: LANG_EN_US,
        data,
    }
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}
