//! Every file an export makes, each with what writes it, so the whole set can be checked
//! before anything is written and then written without holding any file in memory.

use crate::palette::{palette_for, Palette};
use crate::sequence::sequence_plan;
use crate::sheet::{may_write_description, names_for, sheet_plan};
use crate::sheet_layout::SheetOptions;
use crate::stream::{write_frame_png, write_sheet_png};
use crate::{io_error, ExportError, Picture};
use pikslar_format::atomic::{SetError, StepCheck};
use pikslar_format::atomic_stream::{write_set_streamed_as, Producer, Streamed};
use pikslar_format::set_lock::{free_to_replace, SetLock};
use std::io::{self, Write};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

/// A file to be written: where, and what writes it.
pub struct Output<'a> {
    pub path: PathBuf,
    pub write: Producer<'a>,
    /// A description. Replaced only if an earlier export wrote it, and removed before any
    /// other file of the set takes its place, so a set cut short is never described as whole.
    pub ours_only: bool,
}

fn frame_producer<'a>(
    picture: &'a Picture<'a>,
    frame: usize,
    palette: Option<Palette>,
) -> Producer<'a> {
    Box::new(move |out: &mut dyn Write| {
        write_frame_png(picture, frame, palette.as_ref(), out).map_err(io_error)
    })
}

fn text_producer(text: String) -> Producer<'static> {
    Box::new(move |out: &mut dyn Write| out.write_all(text.as_bytes()))
}

/// One frame as a PNG at `path`. An indexed picture's colours are found here, before anything
/// is written.
#[inline(never)]
pub fn frame_output<'a>(
    picture: &'a Picture<'a>,
    frame: usize,
    path: PathBuf,
) -> Result<Output<'a>, ExportError> {
    let frames = picture.doc().frames.len();
    if frame >= frames {
        return Err(ExportError::NoSuchFrame { frame, frames });
    }
    let palette = palette_for(picture, [frame], (false, Some(frame)))?;
    Ok(Output {
        path,
        write: frame_producer(picture, frame, palette),
        ours_only: false,
    })
}

/// A sheet of `frames` at `path`, laid out and described as `options` say, and its description
/// beside it.
#[inline(never)]
pub fn sheet_outputs<'a>(
    picture: &'a Picture<'a>,
    frames: RangeInclusive<usize>,
    path: PathBuf,
    options: &SheetOptions,
) -> Result<Vec<Output<'a>>, ExportError> {
    let (image, stem) = names_for(&path);
    let plan = sheet_plan(picture, frames, (&image, &stem), options)?;
    let (sheet, first, palette) = (plan.sheet, plan.first, plan.palette);
    let description = path.with_extension("json");
    Ok(vec![
        Output {
            path,
            write: Box::new(move |out: &mut dyn Write| {
                write_sheet_png(picture, (&sheet, first), palette.as_ref(), out).map_err(io_error)
            }),
            ours_only: false,
        },
        Output {
            path: description,
            write: text_producer(plan.description),
            ours_only: true,
        },
    ])
}

/// A PNG of each of `frames` beside `path`, numbered from 1, and the description last.
#[inline(never)]
pub fn sequence_outputs<'a>(
    picture: &'a Picture<'a>,
    frames: RangeInclusive<usize>,
    path: &Path,
) -> Result<Vec<Output<'a>>, ExportError> {
    let plan = sequence_plan(picture, frames, path)?;
    let mut outputs: Vec<Output<'a>> = plan
        .members
        .into_iter()
        .zip(plan.palettes)
        .map(|((path, frame), palette)| Output {
            path,
            write: frame_producer(picture, frame, palette),
            ours_only: false,
        })
        .collect();
    outputs.push(Output {
        path: plan.description,
        write: text_producer(plan.text),
        ours_only: true,
    });
    Ok(outputs)
}

/// Writes the set: every file beside its target and checked, then descriptions removed, then
/// each file renamed into place in order. A description someone else wrote is never touched.
/// One export at a time may write a set: a lock named for its last file is held throughout.
/// Each file is written first under this run's own temporary name, recorded in the lock before
/// it is made, so a run stopped part way leaves only files the next run knows to be its own.
pub fn write_outputs(outputs: &mut [Output<'_>], at: StepCheck<'_>) -> Result<(), SetError> {
    let Some(last) = outputs.last().map(|o| o.path.clone()) else {
        return Ok(());
    };
    write_outputs_locked(outputs, &last, at)
}

/// As `write_outputs`, with the set's lock named for `set`, the last file the plan made. A
/// caller that leaves a file out, such as a description someone else wrote, still waits for
/// and excludes every other export of the same set.
#[inline(never)]
pub fn write_outputs_locked(
    outputs: &mut [Output<'_>],
    set: &Path,
    at: StepCheck<'_>,
) -> Result<(), SetError> {
    if outputs.is_empty() {
        return Ok(());
    }
    let mut held = SetLock::take(set).map_err(refused_by)?;
    let temps: Vec<PathBuf> = outputs.iter().map(|o| held.temp_for(&o.path)).collect();
    held.note(&temps).map_err(refused_by)?;
    let retire = descriptions_to_remove(outputs)?;
    let targets: Vec<(PathBuf, bool)> = outputs
        .iter()
        .map(|o| (o.path.clone(), o.ours_only))
        .collect();
    let retire_paths: Vec<&Path> = retire.iter().map(PathBuf::as_path).collect();
    let mut files: Vec<Streamed<'_>> = outputs
        .iter_mut()
        .map(|o| Streamed {
            path: &o.path,
            write: Box::new(|out: &mut dyn Write| (o.write)(out)),
        })
        .collect();
    write_set_streamed_as(&mut files, &temps, &retire_paths, at, &mut || {
        ready_to_replace(&targets)
    })
}

fn refused_by(cause: io::Error) -> SetError {
    SetError {
        replaced: Vec::new(),
        cause: cause.into(),
    }
}

fn not_ours(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{} was not written by Pikslar", path.display()),
    )
}

/// The descriptions there now, each written by an earlier export, or why one may not be removed.
fn descriptions_to_remove(outputs: &[Output<'_>]) -> Result<Vec<PathBuf>, SetError> {
    let mut retire = Vec::new();
    for output in outputs.iter().filter(|o| o.ours_only) {
        if !may_write_description(&output.path) {
            return Err(refused_by(not_ours(&output.path)));
        }
        if std::fs::symlink_metadata(&output.path).is_ok() {
            retire.push(output.path.clone());
        }
    }
    Ok(retire)
}

/// Asked once every file is written, which can be minutes after the export began: a
/// description someone wrote meanwhile is still not theirs to lose, and a file another program
/// holds would stop the renames part way, so either stops the export before anything changes.
fn ready_to_replace(targets: &[(PathBuf, bool)]) -> io::Result<()> {
    for (path, ours_only) in targets {
        if *ours_only && !may_write_description(path) {
            return Err(not_ours(path));
        }
        free_to_replace(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::{ColorMode, Document, Grid};
    use pikslar_format::atomic::{SaveStep, TEMP_SUFFIX};
    use std::fs;

    fn doc(frames: usize, seed: u32) -> Document {
        let mut d = Document::new(20, 20, ColorMode::Rgba).expect("valid");
        for f in 1..frames {
            d.add_frame(f, false).expect("room");
        }
        for f in 0..frames {
            let px: Vec<u32> = (0..400)
                .map(|i| 0xff00_0000 | (i ^ f as u32 ^ seed))
                .collect();
            d.set_cel(0, f, Grid::from_cells(20, 20, &px)).expect("cel");
        }
        d
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pikslar-out-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).expect("dir");
        d
    }

    fn snapshot(d: &Path) -> Vec<(String, Vec<u8>)> {
        let mut v: Vec<_> = fs::read_dir(d)
            .expect("list")
            .map(|e| {
                let e = e.expect("entry");
                let bytes = fs::read(e.path()).unwrap_or_default();
                (e.file_name().to_string_lossy().into_owned(), bytes)
            })
            .collect();
        v.sort();
        v
    }

    /// Whether `temp` is a run's temporary file for `target`.
    fn temp_of(temp: &Path, target: &Path) -> bool {
        let (Some(t), Some(n)) = (temp.file_name(), target.file_name()) else {
            return false;
        };
        let (t, n) = (t.to_string_lossy(), n.to_string_lossy());
        temp.parent() == target.parent()
            && t.starts_with(&format!("{n}."))
            && t.ends_with(TEMP_SUFFIX)
            && t.len() == n.len() + pikslar_format::atomic::RUN_TEMP_EXTRA
    }

    fn export(d: &Path, doc: &Document, at: StepCheck<'_>) -> Result<(), SetError> {
        let source = Picture::visible(doc);
        let mut outputs =
            sequence_outputs(&source, 0..=doc.frames.len() - 1, &d.join("w.png")).expect("planned");
        write_outputs(&mut outputs, at)
    }

    #[test]
    fn a_disk_that_fills_part_way_leaves_the_old_export_as_it_was() {
        let d = dir("full");
        export(&d, &doc(4, 1), &mut |_, _| Ok(())).expect("first export");
        let before = snapshot(&d);
        for member in ["w_0001.png", "w_0003.png", "w.json"] {
            let full = d.join(member);
            let mut fill = |step: SaveStep, temp: &Path| {
                if step == SaveStep::HalfWritten && temp_of(temp, &full) {
                    return Err(io::Error::new(io::ErrorKind::StorageFull, "disk full"));
                }
                Ok(())
            };
            let err = export(&d, &doc(4, 2), &mut fill).expect_err("full");
            assert!(err.replaced.is_empty(), "{member}");
            assert!(err.to_string().contains("disk full"), "{err}");
            assert_eq!(snapshot(&d), before, "{member}");
        }
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_rename_that_fails_part_way_leaves_no_description() {
        let d = dir("late");
        export(&d, &doc(4, 1), &mut |_, _| Ok(())).expect("first export");
        fs::remove_file(d.join("w_0003.png")).expect("make room");
        fs::create_dir(d.join("w_0003.png")).expect("a folder in the way");
        let err = export(&d, &doc(4, 2), &mut |_, _| Ok(())).expect_err("rename fails");
        assert_eq!(
            err.replaced,
            ["w.json", "w_0001.png", "w_0002.png"].map(|n| d.join(n))
        );
        assert!(
            !d.join("w.json").exists(),
            "no description over a mixed set"
        );
        let names: Vec<String> = snapshot(&d).into_iter().map(|(n, _)| n).collect();
        assert_eq!(
            names,
            ["w_0001.png", "w_0002.png", "w_0003.png", "w_0004.png"]
        );
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_description_someone_else_wrote_is_never_removed() {
        let d = dir("theirs");
        fs::write(d.join("w.json"), "{}").expect("theirs");
        let err = export(&d, &doc(2, 1), &mut |_, _| Ok(())).expect_err("refused");
        assert!(err.to_string().contains("not written by Pikslar"), "{err}");
        let names: Vec<String> = snapshot(&d).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["w.json"]);
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_description_someone_writes_during_the_export_is_kept() {
        let d = dir("meanwhile");
        let json = d.join("w.json");
        for earlier in [false, true] {
            let _ = fs::remove_file(&json);
            if earlier {
                export(&d, &doc(2, 1), &mut |_, _| Ok(())).expect("first export");
            }
            let before: Vec<_> = snapshot(&d)
                .into_iter()
                .filter(|(n, _)| n != "w.json")
                .collect();
            let mut theirs = |step: SaveStep, temp: &Path| {
                if step == SaveStep::Verified && temp_of(temp, &json) {
                    fs::write(&json, "{\"mine\":1}")?;
                }
                Ok(())
            };
            let err = export(&d, &doc(2, 2), &mut theirs).expect_err("refused");
            assert!(err.to_string().contains("not written by Pikslar"), "{err}");
            assert!(err.replaced.is_empty());
            assert_eq!(fs::read_to_string(&json).expect("kept"), "{\"mine\":1}");
            let after: Vec<_> = snapshot(&d)
                .into_iter()
                .filter(|(n, _)| n != "w.json")
                .collect();
            assert_eq!(after, before, "earlier {earlier}");
        }
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn no_lock_is_left_after_an_export_or_a_failed_one() {
        let d = dir("unlocked");
        export(&d, &doc(2, 1), &mut |_, _| Ok(())).expect("export");
        let mut fail = |_: SaveStep, _: &Path| Err(io::Error::other("stop"));
        export(&d, &doc(2, 2), &mut fail).expect_err("stopped");
        let names: Vec<String> = snapshot(&d).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["w.json", "w_0001.png", "w_0002.png"]);
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[cfg(windows)]
    #[test]
    fn a_second_export_to_the_same_place_waits_for_nothing_and_changes_nothing() {
        let d = dir("twice");
        export(&d, &doc(3, 1), &mut |_, _| Ok(())).expect("first export");
        let before = snapshot(&d);
        let held = SetLock::take(&d.join("w.json")).expect("another export");
        let err = export(&d, &doc(3, 2), &mut |_, _| Ok(())).expect_err("refused");
        assert!(err.to_string().contains("another export"), "{err}");
        drop(held);
        assert_eq!(snapshot(&d), before);
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[cfg(windows)]
    #[test]
    fn a_member_open_in_another_program_stops_the_export_before_any_rename() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = dir("held");
        export(&d, &doc(4, 1), &mut |_, _| Ok(())).expect("first export");
        let before = snapshot(&d);
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1)
            .open(d.join("w_0003.png"))
            .expect("held");
        let err = export(&d, &doc(4, 2), &mut |_, _| Ok(())).expect_err("refused");
        assert!(err.to_string().contains("open in another program"), "{err}");
        assert!(err.replaced.is_empty());
        drop(held);
        assert_eq!(snapshot(&d), before, "the earlier export is whole");
        fs::remove_dir_all(d).expect("cleanup");
    }
}
