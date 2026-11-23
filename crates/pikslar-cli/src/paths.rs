//! Where output may go. Every target is checked before anything is written.

use crate::failure::Failure;
use pikslar_export::targets::{self, Refused};
use std::path::Path;

/// Image output must end as its format does (.png or .gif), so a slip cannot write an image
/// over a project.
pub fn require_extension(path: &Path, ending: &str) -> Result<(), Failure> {
    let fits = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ending));
    if fits {
        return Ok(());
    }
    Err(Failure::Usage(format!(
        "{} does not end in .{ending}",
        path.display()
    )))
}

/// A name Windows would refuse, change or treat as a device.
pub fn check_name(path: &Path) -> Result<(), Failure> {
    targets::check_name(path).map_err(written)
}

/// Refuses a target that cannot or should not be written. See [`targets::check_target`].
pub fn check_target(path: &Path, project: &Path, overwrite: bool) -> Result<(), Failure> {
    targets::check_target(path, Some(project), overwrite).map_err(written)
}

fn written(refused: Refused) -> Failure {
    match refused {
        Refused::Exists(_) => Failure::Write(format!("{refused}. Add --overwrite to replace it")),
        Refused::Other(message) => Failure::Write(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_and_trailing_dots_are_refused_in_any_case() {
        for name in [
            "CON.png",
            "nul.png",
            "Com1.png",
            "lpt9.x.png",
            "aux .png",
            "a.png.",
            "a.png ",
            "ab:c.png",
            "x?.png",
            "a*.png",
            "a<b.png",
            "a|b.png",
            "a\"b.png",
            "a\u{7}.png",
        ] {
            let err = check_name(Path::new(name)).expect_err(name);
            assert_eq!(err.code(), 4, "{name}");
        }
        for name in ["console.png", "con1.png", "a.png", "COM10.png"] {
            check_name(Path::new(name)).expect(name);
        }
    }

    #[test]
    fn only_png_names_are_images() {
        assert!(require_extension(Path::new("a.PNG"), "png").is_ok());
        assert!(require_extension(Path::new("a.Gif"), "gif").is_ok());
        assert!(require_extension(Path::new("a.png"), "gif").is_err());
        for name in ["a.piks", "a", "a.png.json"] {
            assert_eq!(
                require_extension(Path::new(name), "png")
                    .expect_err(name)
                    .code(),
                1
            );
        }
    }
}
