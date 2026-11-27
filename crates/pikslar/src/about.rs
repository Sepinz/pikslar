//! Which Pikslar this is: `pikslar.exe --version` and the Keys dialog's footer line.
//!
//! The version comes from the workspace `Cargo.toml`; the commit from `PIKSLAR_COMMIT`, which
//! the release script (`spikes/make-release.ps1`) and the usability kit script
//! (`spikes/make-usability-kit.ps1`) set, and is "unknown" otherwise. `PIKSLAR_BUILD_LABEL`
//! names a build that is not a release candidate, such as "U1" for the first usability round;
//! release builds leave it unset, so their line is unchanged.

/// The version, as in the file's version information.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The commit this program was built from.
pub(crate) const COMMIT: &str = match option_env!("PIKSLAR_COMMIT") {
    Some(c) if !c.is_empty() => c,
    _ => "unknown",
};

/// The label of a build that is not a release candidate, or nothing.
pub(crate) const LABEL: Option<&str> = match option_env!("PIKSLAR_BUILD_LABEL") {
    Some(l) if !l.is_empty() => Some(l),
    _ => None,
};

/// "Pikslar 0.9.0-beta.1 (abc1234)", or "Pikslar 0.9.0-beta.1 U1 (abc1234)" for a labelled build.
pub(crate) fn line() -> String {
    line_of(VERSION, LABEL, COMMIT)
}

fn line_of(version: &str, label: Option<&str>, commit: &str) -> String {
    match label.map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) => format!("Pikslar {version} {l} ({})", commit.trim()),
        None => format!("Pikslar {version} ({})", commit.trim()),
    }
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

/// The text the Keys dialog's footer has room for in `width` pixels: the whole line, a shorter
/// one, or nothing at very large text sizes.
pub(crate) fn footer_text(width: i32, text_scale: i32) -> Option<String> {
    footer_candidates(VERSION, LABEL, COMMIT)
        .into_iter()
        .find(|t| pikslar_ui::text_width(t, text_scale) <= width)
}

/// Longest first. A release build shows the version; a labelled build always keeps its label
/// and commit, so that a tester's screen can never be taken for a release candidate's.
fn footer_candidates(version: &str, label: Option<&str>, commit: &str) -> Vec<String> {
    match label.map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) => {
            let c = commit.trim();
            vec![
                format!("Pikslar {version} {l} ({c})"),
                format!("{version} {l} ({c})"),
                format!("{l} ({c})"),
                l.to_string(),
            ]
        }
        None => vec![format!("Pikslar {version}"), version.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_the_program_version_and_commit() {
        let want = match LABEL {
            Some(l) => format!("Pikslar {} {l} ({})", env!("CARGO_PKG_VERSION"), COMMIT),
            None => format!("Pikslar {} ({})", env!("CARGO_PKG_VERSION"), COMMIT),
        };
        assert_eq!(line(), want);
        assert!(!COMMIT.is_empty());
    }

    #[test]
    fn an_unlabelled_build_keeps_the_release_line() {
        assert_eq!(
            line_of("0.9.0-beta.1", None, "unknown"),
            "Pikslar 0.9.0-beta.1 (unknown)"
        );
        assert_eq!(
            line_of("0.9.0-beta.1", Some(" "), "abc1234"),
            "Pikslar 0.9.0-beta.1 (abc1234)"
        );
    }

    #[test]
    fn a_labelled_build_names_its_label_and_commit() {
        assert_eq!(
            line_of("0.9.0-beta.1", Some("U1"), "abc1234"),
            "Pikslar 0.9.0-beta.1 U1 (abc1234)"
        );
        let c = footer_candidates("0.9.0-beta.1", Some("U1"), "abc1234");
        assert_eq!(c[0], "Pikslar 0.9.0-beta.1 U1 (abc1234)");
        assert!(
            c.iter().all(|t| t.contains("U1")),
            "the label is never dropped: {c:?}"
        );
        assert!(c[..3].iter().all(|t| t.contains("abc1234")));
        assert_eq!(
            footer_candidates("0.9.0-beta.1", None, "abc1234"),
            ["Pikslar 0.9.0-beta.1", "0.9.0-beta.1"]
        );
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
        let all = footer_candidates(VERSION, LABEL, COMMIT);
        let room = pikslar_ui::text_width(&all[0], 1);
        assert_eq!(footer_text(room, 1), Some(all[0].clone()));
        assert_eq!(footer_text(room - 1, 1), Some(all[1].clone()));
        assert_eq!(footer_text(1, 1), None);
    }
}
