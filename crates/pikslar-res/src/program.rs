//! The version block both program files carry (`pikslar.exe`, `pikslar-cli.exe`), from the
//! build's environment: `CARGO_PKG_VERSION`, and `PIKSLAR_COMMIT` and `PIKSLAR_COPYRIGHT`,
//! which the release script sets (`spikes/make-release.ps1`).

use crate::version::{self, VersionInfo};
use std::env;
use std::path::Path;

/// The environment values a build script must be re-run for when they change.
pub const ENV_INPUTS: [&str; 2] = ["PIKSLAR_COMMIT", "PIKSLAR_COPYRIGHT"];
/// Shown until the owner chooses the copyright line (plan D1, D2).
pub const COPYRIGHT_PLACEHOLDER: &str = "Copyright notice to be set";

/// The version block for `file` (its original file name) and `description`, from the
/// environment of a build script.
pub fn version_info(file: &str, description: &str) -> Result<VersionInfo, String> {
    let text = env::var("CARGO_PKG_VERSION").map_err(|e| format!("CARGO_PKG_VERSION: {e}"))?;
    let commit = optional_env("PIKSLAR_COMMIT", 64)?;
    let copyright = optional_env("PIKSLAR_COPYRIGHT", 200)?;
    version_info_of(
        &text,
        commit.as_deref(),
        copyright.as_deref(),
        file,
        description,
    )
}

/// The version block from explicit values.
pub fn version_info_of(
    text: &str,
    commit: Option<&str>,
    copyright: Option<&str>,
    file: &str,
    description: &str,
) -> Result<VersionInfo, String> {
    let (numbers, prerelease) = version::parse(text)?;
    let strings = [
        ("ProductName", "Pikslar"),
        ("FileDescription", description),
        ("ProductVersion", text),
        ("FileVersion", text),
        ("OriginalFilename", file),
        ("LegalCopyright", copyright.unwrap_or(COPYRIGHT_PLACEHOLDER)),
    ];
    let mut strings: Vec<(String, String)> = strings
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    strings.push((
        "Comments".into(),
        format!("Commit {}", commit.unwrap_or("unknown")),
    ));
    Ok(VersionInfo {
        numbers,
        prerelease,
        strings,
    })
}

/// Windows' classic path limit; gcc cannot open a `-B` file whose full path reaches it.
const MAX_PATH: usize = 260;

/// Puts an empty `default-manifest.o` in `out` and tells the linker to look there first, so
/// the MinGW-w64 gcc driver links it instead of its own default manifest, whose language
/// collides with the program's manifest (`crates/pikslar/build.rs` explains). Prints the
/// `cargo:` lines; for build scripts only.
pub fn replace_default_manifest(out: &Path) -> Result<(), String> {
    let path = out.join("default-manifest.o");
    std::fs::write(&path, crate::empty_object()).map_err(|e| format!("{}: {e}", path.display()))?;
    if path.as_os_str().len() >= MAX_PATH {
        println!(
            "cargo:warning=the target folder's path is too long for gcc to find {}; a MinGW              default manifest may be linked beside Pikslar's (use a shorter CARGO_TARGET_DIR)",
            path.display()
        );
    }
    println!("cargo:rustc-link-arg-bins=-B{}/", out.display());
    Ok(())
}

/// An environment value that is set and not blank, refused if it has control characters or is
/// longer than `max` characters.
fn optional_env(name: &str, max: usize) -> Result<Option<String>, String> {
    let value = match env::var(name) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        Ok(_) | Err(env::VarError::NotPresent) => return Ok(None),
        Err(e) => return Err(format!("{name}: {e}")),
    };
    if value.chars().count() > max || value.chars().any(char::is_control) {
        return Err(format!("{name} must be at most {max} printable characters"));
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a>(info: &'a VersionInfo, key: &str) -> Option<&'a str> {
        info.strings
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn the_block_names_the_file_version_commit_and_copyright() {
        let info = version_info_of(
            "0.9.0-beta.1",
            Some("abc1234"),
            None,
            "pikslar-cli.exe",
            "Pikslar command-line tool",
        )
        .expect("a block");
        assert_eq!(info.numbers, [0, 9, 0, 0]);
        assert!(info.prerelease);
        assert_eq!(get(&info, "OriginalFilename"), Some("pikslar-cli.exe"));
        assert_eq!(get(&info, "ProductVersion"), Some("0.9.0-beta.1"));
        assert_eq!(get(&info, "Comments"), Some("Commit abc1234"));
        assert_eq!(get(&info, "LegalCopyright"), Some(COPYRIGHT_PLACEHOLDER));
        let named = version_info_of("1.0.0", None, Some("(c) X"), "a.exe", "A").expect("ok");
        assert_eq!(get(&named, "Comments"), Some("Commit unknown"));
        assert_eq!(get(&named, "LegalCopyright"), Some("(c) X"));
        assert!(version_info_of("1.0", None, None, "a.exe", "A").is_err());
    }
}
