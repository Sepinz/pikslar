//! The one folder a session may open and save in, given on the command line by the person who
//! started it. A request names files by a relative path inside it; anything that could reach
//! outside is refused, before and after links are followed.

use crate::failure::Failure;
use crate::fields::Refusal;
use crate::paths::check_name;
use pikslar_plugin::hidden::first_unshown;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub struct Sandbox {
    /// The folder, with every link resolved.
    root: PathBuf,
}

fn outside(given: &str) -> Refusal {
    Refusal::new(
        "outside",
        format!("{given:?} is not inside the session's folder"),
    )
}

impl Sandbox {
    pub fn new(dir: &Path) -> Result<Self, Failure> {
        let fail = |what: &str| Failure::Read(format!("{}: {what}", dir.display()));
        let root = fs::canonicalize(dir).map_err(|e| fail(&e.to_string()))?;
        match root.is_dir() {
            true => Ok(Sandbox { root }),
            false => Err(fail("is not a folder")),
        }
    }

    /// The path a request gave, which must be relative and made only of plain names: no drive,
    /// root, share, `.` or `..`, and no name Windows would refuse or change.
    fn relative(given: &str) -> Result<PathBuf, Refusal> {
        let refuse = |why: &str| Refusal::new("bad_path", format!("{given:?} {why}"));
        if given.is_empty() {
            return Err(refuse("is empty"));
        }
        let path = Path::new(given);
        for part in path.components() {
            let Component::Normal(name) = part else {
                return Err(refuse(
                    "must be a relative path inside the session's folder, with no drive, \
                     root, . or ..",
                ));
            };
            check_name(Path::new(name)).map_err(|e| refuse(&format!("is refused: {e}")))?;
        }
        // The plugin crate's list: what would hide or reorder a name in a listing.
        if let Some(c) = first_unshown(given) {
            return Err(refuse(&format!(
                "holds the character U+{:04X}, which is not shown as itself",
                u32::from(c)
            )));
        }
        Ok(path.to_path_buf())
    }

    /// Whether any existing part of `relative`, from the folder down, is a link or junction.
    fn through_link(&self, relative: &Path) -> bool {
        let mut at = self.root.clone();
        for part in relative.components() {
            at.push(part);
            match fs::symlink_metadata(&at) {
                Ok(m) if m.file_type().is_symlink() => return true,
                Ok(_) => {}
                Err(_) => return false,
            }
        }
        false
    }

    /// Whether `path`, with links resolved, is inside the folder.
    fn inside(&self, path: &Path) -> bool {
        fs::canonicalize(path).is_ok_and(|real| real.starts_with(&self.root))
    }

    /// An existing file to open.
    pub fn existing(&self, given: &str) -> Result<PathBuf, Refusal> {
        let relative = Self::relative(given)?;
        let path = self.root.join(&relative);
        let real = fs::canonicalize(&path).map_err(|e| match self.through_link(&relative) {
            // Where a link leads is not told: a link out of the folder is refused the same
            // whether or not what it names exists.
            true => outside(given),
            false => Refusal::new("not_found", format!("{given:?}: {e}")),
        })?;
        if !real.starts_with(&self.root) {
            return Err(outside(given));
        }
        match real.is_file() {
            true => Ok(real),
            false => Err(Refusal::new(
                "not_found",
                format!("{given:?} is not a file"),
            )),
        }
    }

    /// A file to write, which may not exist yet. Its folder must, inside this one.
    pub fn target(&self, given: &str) -> Result<PathBuf, Refusal> {
        let path = self.root.join(Self::relative(given)?);
        let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(outside(given));
        };
        let folder = fs::canonicalize(folder)
            .map_err(|e| Refusal::new("not_found", format!("{given:?}: {e}")))?;
        if !folder.starts_with(&self.root) {
            return Err(outside(given));
        }
        let target = folder.join(name);
        let link = fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink());
        if link || (target.exists() && !self.inside(&target)) {
            return Err(Refusal::new(
                "bad_target",
                format!("{given:?} is a link; a session replaces files, not what links point to"),
            ));
        }
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_and_hidden_characters_are_refused_as_bad_paths() {
        let dir = std::env::temp_dir().join(format!("pikslar-sandbox-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("dir");
        let sandbox = Sandbox::new(&dir).expect("sandbox");
        for given in [
            "conin$.piks",
            "CONOUT$.piks",
            "conin$",
            "com\u{b9}.piks",
            "LPT\u{b3}",
            "com0.piks",
            "nul.piks",
            "\u{202E}skip.piks",
            "walk.piks\u{200B}",
            "a\u{2066}b.piks",
            "sub/\u{FEFF}x.piks",
            "Mongol\u{180B}.piks",
            "no\u{00A0}break.piks",
        ] {
            for refusal in [sandbox.target(given), sandbox.existing(given)] {
                let refusal = refusal.expect_err(given);
                assert_eq!(refusal.code, "bad_path", "{given:?}: {}", refusal.message);
            }
        }
        sandbox.target("walk.piks").expect("a plain name");
        sandbox
            .target("K\u{e4}fer.piks")
            .expect("a name with an accent");
        fs::remove_dir_all(dir).expect("cleanup");
    }
}
