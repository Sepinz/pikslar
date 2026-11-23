//! Embeds version information in `pikslar-cli.exe` (see `crates/pikslar/build.rs` for why a
//! COFF object is written instead of using a resource compiler) and a console manifest of its
//! own (`res/pikslar-cli.manifest`: Windows 10 and 11, long paths, no elevation; no DPI or
//! Common Controls settings, so console behaviour is what Windows gives any console program).
//! It replaces the MinGW default manifest exactly as `pikslar.exe`'s does.

use pikslar_res::{program, version, Resource, LANG_EN_US, RT_MANIFEST, RT_VERSION};
use std::env;
use std::path::PathBuf;

const MANIFEST: &str = "res/pikslar-cli.manifest";

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={MANIFEST}");
    for var in program::ENV_INPUTS {
        println!("cargo:rerun-if-env-changed={var}");
    }
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_env != "gnu" {
        return Ok(());
    }
    let info = program::version_info("pikslar-cli.exe", "Pikslar command-line tool")?;
    let manifest = std::fs::read(MANIFEST).map_err(|e| format!("{MANIFEST}: {e}"))?;
    let resource = |ty, data| Resource {
        ty,
        id: 1,
        lang: LANG_EN_US,
        data,
    };
    let object = pikslar_res::object(&[
        resource(RT_VERSION, version::block(&info)),
        resource(RT_MANIFEST, manifest),
    ])?;
    let out = PathBuf::from(env::var("OUT_DIR").map_err(|e| format!("OUT_DIR: {e}"))?);
    let path = out.join("pikslar-cli-resources.o");
    std::fs::write(&path, object).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("cargo:rustc-link-arg-bins={}", path.display());
    program::replace_default_manifest(&out)
}
