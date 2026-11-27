//! How a saved project takes its file's place: `ReplaceFileW` with a backup when the file
//! exists, which keeps the person's permissions, streams and creation time, and a move that
//! never replaces anything for a first save (`docs/b1-spikes-save.md`, Spike A).

use crate::outside::{hold, still_same, ChangedWhileSaving, Seen};
use pikslar_format::publish::Publish;
use std::io;
use std::path::Path;

/// The system's way of putting a file in place.
#[derive(Default)]
pub(crate) struct SystemPublish {
    /// How the target must still look when it is replaced (`outside.rs`).
    pub(crate) expect: Option<Seen>,
}

impl Publish for SystemPublish {
    /// With a file expected, holds it so no other program can open it to write, looks at it
    /// once more, and replaces it while still holding it. A program writing it now makes the
    /// hold fail with error 32, which is tried again like any file in use.
    fn replace(&mut self, target: &Path, temp: &Path, backup: &Path) -> io::Result<()> {
        let held = match &self.expect {
            None => None,
            Some(seen) => {
                let held = hold(target)?;
                if still_same(target, seen).is_none() {
                    let name = target.file_name().unwrap_or_default().to_string_lossy();
                    return Err(io::Error::other(ChangedWhileSaving(name.into_owned())));
                }
                Some(held)
            }
        };
        let replaced = pikslar_platform::replace_file(target, temp, backup);
        drop(held);
        replaced
    }

    fn place_new(&mut self, temp: &Path, target: &Path) -> io::Result<()> {
        pikslar_platform::move_new(temp, target)
    }
}

/// What a save to `path` publishes through: the system, or in a test the disk set for it.
pub(crate) fn publisher(path: &Path, expect: Option<Seen>) -> Box<dyn Publish> {
    #[cfg(test)]
    if let Some(disk) = test_disk(path) {
        return disk();
    }
    #[cfg(not(test))]
    let _ = path;
    Box::new(SystemPublish { expect })
}

/// A misbehaving disk a test puts under saves to one path, through the real save thread.
#[cfg(test)]
pub(crate) type TestDisk = fn() -> Box<dyn Publish>;

#[cfg(test)]
static TEST_DISKS: std::sync::Mutex<Vec<(std::path::PathBuf, TestDisk)>> =
    std::sync::Mutex::new(Vec::new());

/// Saves to `path` go through `disk` until [`clear_test_disk`].
#[cfg(test)]
pub(crate) fn set_test_disk(path: &Path, disk: TestDisk) {
    let mut disks = TEST_DISKS.lock().expect("test disks");
    disks.retain(|(p, _)| p != path);
    disks.push((path.to_path_buf(), disk));
}

#[cfg(test)]
pub(crate) fn clear_test_disk(path: &Path) {
    TEST_DISKS
        .lock()
        .expect("test disks")
        .retain(|(p, _)| p != path);
}

#[cfg(test)]
fn test_disk(path: &Path) -> Option<TestDisk> {
    let disks = TEST_DISKS.lock().expect("test disks");
    disks.iter().find(|(p, _)| p == path).map(|(_, d)| *d)
}

#[cfg(test)]
#[path = "system_publish_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "upgrade_save_tests.rs"]
mod upgrade_tests;

#[cfg(test)]
#[path = "long_path_save_tests.rs"]
mod long_path_tests;
