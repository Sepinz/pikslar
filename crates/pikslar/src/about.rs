//! Which Pikslar this is: `pikslar.exe --version` and the Keys dialog's footer line.
//!
//! The version comes from the workspace `Cargo.toml`; the commit from `PIKSLAR_COMMIT`, which
//! the release script (`spikes/make-release.ps1`) sets, and is "unknown" otherwise.

/// The version, as in the file's version information.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The commit this program was built from.
pub(crate) const COMMIT: &str = match option_env!("PIKSLAR_COMMIT") {
    Some(c) if !c.is_empty() => c,
    _ => "unknown",
};

/// "Pikslar 0.9.0-beta.1 (abc1234)".
pub(crate) fn line() -> String {
    format!("Pikslar {VERSION} ({})", COMMIT.trim())
}

/// Whether the arguments ask for the version only.
pub(crate) fn asked(args: impl IntoIterator<Item = std::ffi::OsString>) -> bool {
    args.into_iter().any(|a| a == "--version")
}

/// When asked for the version, prints its line and exits, before the program opens a window,
/// reads settings or writes anything.
pub(crate) fn exit_if_asked() {
    if asked(std::env::args_os().skip(1)) {
        pikslar_platform::console::print_line(&line());
        std::process::exit(0);
    }
}

/// The text the Keys dialog's footer has room for in `width` pixels: the whole line, the
/// version alone, or nothing at very large text sizes.
pub(crate) fn footer_text(width: i32, text_scale: i32) -> Option<String> {
    let whole = format!("Pikslar {VERSION}");
    [whole, VERSION.to_string()]
        .into_iter()
        .find(|t| pikslar_ui::text_width(t, text_scale) <= width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_the_program_version_and_commit() {
        assert_eq!(
            line(),
            format!("Pikslar {} ({})", env!("CARGO_PKG_VERSION"), COMMIT)
        );
        assert!(!COMMIT.is_empty());
    }

    #[test]
    fn only_the_version_switch_asks_for_it() {
        let ask = |a: &[&str]| asked(a.iter().map(std::ffi::OsString::from));
        assert!(ask(&["--version"]));
        assert!(ask(&["a.piks", "--version"]));
        assert!(!ask(&[]));
        assert!(!ask(&["--versions", "version", "-v"]));
    }

    #[test]
    fn the_footer_shortens_rather_than_overflows() {
        let whole = format!("Pikslar {VERSION}");
        let room = pikslar_ui::text_width(&whole, 1);
        assert_eq!(footer_text(room, 1), Some(whole));
        assert_eq!(footer_text(room - 1, 1), Some(VERSION.to_string()));
        assert_eq!(footer_text(1, 1), None);
    }
}
