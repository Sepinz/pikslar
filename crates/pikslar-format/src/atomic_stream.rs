//! Sets of files whose bytes are produced while they are written, so none is ever whole in
//! memory. Each is checked by reading it back against a hash taken as it was written.

use crate::atomic::{new_file, or_remove, set_with, temp_path, SaveStep, SetError, StepCheck};
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

/// Writes one file's bytes into whatever it is given, in as many pieces as it likes.
pub type Producer<'a> = Box<dyn FnMut(&mut dyn Write) -> io::Result<()> + 'a>;

/// One member of a set: where it goes and what writes it.
pub struct Streamed<'a> {
    pub path: &'a Path,
    pub write: Producer<'a>,
}

const PIECE: usize = 1 << 16;

/// FNV-1a, 64 bits, fed in pieces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Fnv(u64);

impl Fnv {
    const START: Fnv = Fnv(0xcbf2_9ce4_8422_2325);

    fn update(self, bytes: &[u8]) -> Fnv {
        Fnv(bytes.iter().fold(self.0, |h, &b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        }))
    }
}

/// Passes bytes on to the file while counting and hashing them. The first bytes to arrive are
/// the moment a test may treat as half written.
struct Tally<'a, 'b> {
    out: BufWriter<File>,
    hash: Fnv,
    len: u64,
    half: Option<(StepCheck<'a>, &'b Path)>,
}

impl Write for Tally<'_, '_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.out.write(buf)?;
        self.hash = self.hash.update(&buf[..written]);
        self.len += written as u64;
        if written > 0 {
            if let Some((at, temp)) = self.half.take() {
                at(SaveStep::HalfWritten, temp)?;
            }
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

fn read_back(temp: &Path) -> io::Result<(Fnv, u64)> {
    let mut file = File::open(temp)?;
    let mut buffer = vec![0u8; PIECE];
    let (mut hash, mut len) = (Fnv::START, 0u64);
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok((hash, len));
        }
        hash = hash.update(&buffer[..read]);
        len += read as u64;
    }
}

/// Makes `temp` new, writes it from `produce`, syncs it, and reads it back. Whatever is at the
/// name already is kept and stops the write; a failure afterwards removes the file this made.
fn write_streamed(temp: &Path, produce: &mut Producer<'_>, at: StepCheck<'_>) -> io::Result<()> {
    let file = new_file(temp)?;
    or_remove(temp, || fill(file, temp, produce, at))
}

fn fill(file: File, temp: &Path, produce: &mut Producer<'_>, at: StepCheck<'_>) -> io::Result<()> {
    at(SaveStep::Created, temp)?;
    let mut tally = Tally {
        out: BufWriter::with_capacity(PIECE, file),
        hash: Fnv::START,
        len: 0,
        half: Some((&mut *at, temp)),
    };
    produce(&mut tally)?;
    let Tally {
        out,
        hash,
        len,
        half,
    } = tally;
    if let Some((at, temp)) = half {
        at(SaveStep::HalfWritten, temp)?;
    }
    let file = out.into_inner().map_err(|e| e.into_error())?;
    at(SaveStep::Written, temp)?;
    file.sync_all()?;
    drop(file);
    at(SaveStep::Synced, temp)?;
    if read_back(temp)? != (hash, len) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "what was written did not read back correctly",
        ));
    }
    at(SaveStep::Verified, temp)
}

/// Writes a set whose members are produced as they are written, holding none of them. Every
/// member is written beside its target and checked first; then the `retire` files are removed
/// and the members renamed into place in order. Any failure before the first removal changes
/// nothing. A removal or rename can still fail part way, and the error says what was done.
pub fn write_set_streamed(
    files: &mut [Streamed<'_>],
    retire: &[&Path],
    at: StepCheck<'_>,
) -> Result<(), SetError> {
    write_set_streamed_when(files, retire, at, &mut || Ok(()))
}

/// [`write_set_streamed`], asking `ready` once every member is written and checked, before
/// anything is removed or renamed. An error from it changes no target.
pub fn write_set_streamed_when(
    files: &mut [Streamed<'_>],
    retire: &[&Path],
    at: StepCheck<'_>,
    ready: &mut dyn FnMut() -> io::Result<()>,
) -> Result<(), SetError> {
    let temps: Vec<PathBuf> = files.iter().map(|f| temp_path(f.path)).collect();
    write_set_streamed_as(files, &temps, retire, at, ready)
}

/// [`write_set_streamed_when`], each member written first at its name in `temps`, which is
/// only ever made new.
pub fn write_set_streamed_as(
    files: &mut [Streamed<'_>],
    temps: &[PathBuf],
    retire: &[&Path],
    at: StepCheck<'_>,
    ready: &mut dyn FnMut() -> io::Result<()>,
) -> Result<(), SetError> {
    let paths: Vec<&Path> = files.iter().map(|f| f.path).collect();
    set_with(
        &paths,
        temps,
        retire,
        |index, temp| write_streamed(temp, &mut files[index].write, at),
        ready,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atomic::{temp_path, TEMP_SUFFIX};
    use std::fs;
    use std::path::PathBuf;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pikslar-stream-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).expect("temp dir");
        d
    }

    fn names(d: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(d)
            .expect("list")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    fn pieces(text: &'static [u8], piece: usize) -> Producer<'static> {
        Box::new(move |out| text.chunks(piece).try_for_each(|p| out.write_all(p)))
    }

    #[test]
    fn members_are_written_from_their_producers() {
        let d = dir("whole");
        let (a, b) = (d.join("a.png"), d.join("a.json"));
        let big: &'static [u8] = Box::leak(vec![7u8; 3 * PIECE + 5].into_boxed_slice());
        let mut files = [
            Streamed {
                path: &a,
                write: pieces(big, 1000),
            },
            Streamed {
                path: &b,
                write: pieces(b"", 1),
            },
        ];
        write_set_streamed(&mut files, &[], &mut |_, _| Ok(())).expect("written");
        assert_eq!(fs::read(&a).expect("a"), big);
        assert_eq!(fs::read(&b).expect("b"), b"");
        assert_eq!(names(&d), ["a.json", "a.png"]);
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn every_step_is_offered_once_per_member_in_order() {
        let d = dir("steps");
        let a = d.join("a.png");
        for text in [&b""[..], b"x", b"several bytes"] {
            let mut steps = Vec::new();
            let mut files = [Streamed {
                path: &a,
                write: pieces(text, 2),
            }];
            write_set_streamed(&mut files, &[], &mut |s, _| {
                steps.push(s);
                Ok(())
            })
            .expect("written");
            assert_eq!(steps, SaveStep::ALL, "{text:?}");
        }
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_failure_anywhere_before_the_renames_changes_nothing() {
        let d = dir("early");
        let (a, b, old) = (d.join("a.png"), d.join("a.json"), d.join("old.json"));
        for (path, text) in [(&a, "old a"), (&b, "old b"), (&old, "old")] {
            fs::write(path, text).expect("old");
        }
        let before = names(&d);
        for step in SaveStep::ALL {
            let mut files = [
                Streamed {
                    path: &a,
                    write: pieces(b"new a", 2),
                },
                Streamed {
                    path: &b,
                    write: pieces(b"new b", 2),
                },
            ];
            let mut fail = |at: SaveStep, temp: &Path| {
                if at == step && temp == temp_path(&b) {
                    return Err(io::Error::other("planted"));
                }
                Ok(())
            };
            let err = write_set_streamed(&mut files, &[&old], &mut fail).expect_err("fails");
            assert!(err.replaced.is_empty(), "{step:?}");
            assert_eq!(names(&d), before, "{step:?}");
            assert_eq!(fs::read(&a).expect("a"), b"old a");
        }
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_producer_that_fails_like_a_full_disk_leaves_no_trace() {
        let d = dir("full");
        let a = d.join("a.png");
        let mut files = [Streamed {
            path: &a,
            write: Box::new(|out: &mut dyn Write| {
                out.write_all(&[1; 100_000])?;
                Err(io::Error::new(io::ErrorKind::StorageFull, "no room"))
            }),
        }];
        let err = write_set_streamed(&mut files, &[], &mut |_, _| Ok(())).expect_err("full");
        assert!(err.to_string().contains("no room"), "{err}");
        assert!(names(&d).is_empty(), "{:?}", names(&d));
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn retired_files_go_before_the_first_rename_and_may_be_missing() {
        let d = dir("retire");
        let (a, old, gone) = (d.join("a.png"), d.join("a.json"), d.join("never.json"));
        fs::write(&old, "old").expect("old");
        let mut files = [Streamed {
            path: &a,
            write: pieces(b"new", 1),
        }];
        let mut order = Vec::new();
        write_set_streamed(&mut files, &[&old, &gone], &mut |s, _| {
            order.push((s, old.exists()));
            Ok(())
        })
        .expect("written");
        assert!(
            order.iter().all(|&(_, there)| there),
            "removed only after checking"
        );
        assert_eq!(names(&d), ["a.png"]);
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_file_already_at_a_temporary_name_is_kept_and_stops_the_set() {
        let d = dir("in-the-way");
        let (a, b) = (d.join("a.png"), d.join("a.json"));
        fs::write(temp_path(&b), b"precious").expect("someone's file");
        let mut files = [
            Streamed {
                path: &a,
                write: pieces(b"image", 2),
            },
            Streamed {
                path: &b,
                write: pieces(b"text", 2),
            },
        ];
        let err = write_set_streamed(&mut files, &[], &mut |_, _| Ok(())).expect_err("stops");
        assert!(err.to_string().contains("is in the way"), "{err}");
        assert!(err.replaced.is_empty());
        assert_eq!(fs::read(temp_path(&b)).expect("kept"), b"precious");
        assert_eq!(names(&d), ["a.json.pikslar-tmp"], "the set's own file went");
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_member_that_fails_leaves_the_names_of_later_members_alone() {
        let d = dir("later");
        let (a, b) = (d.join("a.png"), d.join("a.json"));
        let temps = [
            d.join("a.png.t1.pikslar-tmp"),
            d.join("a.json.t1.pikslar-tmp"),
        ];
        fs::write(&temps[1], b"not this set's").expect("someone's file");
        let mut files = [
            Streamed {
                path: &a,
                write: Box::new(|out| {
                    out.write_all(b"half")?;
                    Err(io::Error::other("disk full"))
                }),
            },
            Streamed {
                path: &b,
                write: pieces(b"text", 2),
            },
        ];
        let err =
            write_set_streamed_as(&mut files, &temps, &[], &mut |_, _| Ok(()), &mut || Ok(()))
                .expect_err("fails");
        assert!(err.to_string().contains("disk full"), "{err}");
        assert!(!temps[0].exists(), "its own file went");
        assert_eq!(fs::read(&temps[1]).expect("kept"), b"not this set's");
        assert_eq!(names(&d), ["a.json.t1.pikslar-tmp"]);
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn named_temporary_files_take_their_places() {
        let d = dir("named");
        let (a, b) = (d.join("a.png"), d.join("a.json"));
        fs::write(temp_path(&a), b"left by someone").expect("old leftover");
        let temps = [
            d.join("a.png.t2.pikslar-tmp"),
            d.join("a.json.t2.pikslar-tmp"),
        ];
        let mut files = [
            Streamed {
                path: &a,
                write: pieces(b"image", 2),
            },
            Streamed {
                path: &b,
                write: pieces(b"text", 2),
            },
        ];
        write_set_streamed_as(&mut files, &temps, &[], &mut |_, _| Ok(()), &mut || Ok(()))
            .expect("written");
        assert_eq!(fs::read(&a).expect("a"), b"image");
        assert_eq!(fs::read(&b).expect("b"), b"text");
        assert_eq!(names(&d), ["a.json", "a.png", "a.png.pikslar-tmp"]);
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_member_changed_on_disk_after_writing_is_caught() {
        let d = dir("changed");
        let a = d.join("a.png");
        let mut files = [Streamed {
            path: &a,
            write: pieces(b"the right bytes", 4),
        }];
        let mut tamper = |s: SaveStep, temp: &Path| {
            if s == SaveStep::Synced {
                fs::write(temp, b"the wrong bytes")?;
            }
            Ok(())
        };
        let err = write_set_streamed(&mut files, &[], &mut tamper).expect_err("caught");
        assert!(err.to_string().contains("read back"), "{err}");
        assert!(names(&d).iter().all(|n| !n.ends_with(TEMP_SUFFIX)));
        assert!(!a.exists());
        fs::remove_dir_all(d).expect("cleanup");
    }
}
