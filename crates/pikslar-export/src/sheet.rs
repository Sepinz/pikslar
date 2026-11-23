//! Sprite sheets: an image of every frame, and a description game engines can read.

use crate::frames::{describe_durations, tags_within};
use crate::palette::{palette_for, push_format, Palette};
use crate::sheet_layout::{arrange, JsonShape, Sheet, SheetOptions};
use crate::sheet_slices::push_slices;
use crate::stream::write_sheet_png;
use crate::Picture;
use crate::{json, ExportError};
use pikslar_core::{Direction, Document, Tag};
use std::fmt::Write;
use std::ops::RangeInclusive;
use std::path::Path;

pub(crate) const OWNER_MARK: &str = "\"app\":\"Pikslar\"";

/// The name a direction has in descriptions.
#[inline(never)]
pub fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Forward => "forward",
        Direction::Reverse => "reverse",
        Direction::PingPong => "pingpong",
    }
}

/// One entry per frame in the sheet, in a list or keyed by name as `shape` says. `first` is
/// the document frame the sheet starts at.
fn push_frames(out: &mut String, sheet: &Sheet, at: (&Document, usize, Size), names: Named) {
    let (doc, first, (w, h, _)) = at;
    let times = describe_durations(doc, first..first + sheet.places.len());
    let (stem, shape) = names;
    out.push(match shape {
        JsonShape::Array => '[',
        JsonShape::Hash => '{',
    });
    for (position, &(x, y)) in sheet.places.iter().enumerate() {
        let frame = first + position;
        if position > 0 {
            out.push(',');
        }
        let name = format!("{stem} {frame}");
        match shape {
            JsonShape::Array => {
                out.push_str("{\"filename\":");
                json::push_string(out, &name);
                out.push(',');
            }
            JsonShape::Hash => {
                json::push_string(out, &name);
                out.push_str(":{");
            }
        }
        let duration = times[position];
        let _ = write!(
            out,
            "\"frame\":{{\"x\":{x},\"y\":{y},\"w\":{w},\"h\":{h}}},\"rotated\":false,\
             \"trimmed\":false,\"spriteSourceSize\":{{\"x\":0,\"y\":0,\"w\":{w},\"h\":{h}}},\
             \"sourceSize\":{{\"w\":{w},\"h\":{h}}},\"duration\":{duration}}}"
        );
    }
    out.push(match shape {
        JsonShape::Array => ']',
        JsonShape::Hash => '}',
    });
}

/// The stem frames are named after, and how they are listed.
type Named<'a> = (&'a str, JsonShape);

pub(crate) fn push_tags(out: &mut String, tags: &[Tag]) {
    for (index, tag) in tags.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        json::push_string(out, &tag.name);
        let _ = write!(
            out,
            ",\"from\":{},\"to\":{},\"direction\":\"{}\"",
            tag.from,
            tag.to,
            direction_name(tag.direction)
        );
        // As Aseprite writes it: a string, and only for a tag that does not play forever.
        if tag.repeat != 0 {
            let _ = write!(out, ",\"repeat\":\"{}\"", tag.repeat);
        }
        out.push('}');
    }
}

/// A frame's width and height as exported, and the scale that made them.
type Size = (usize, usize, usize);

/// The JSON atlas that accompanies a sheet: frame rectangles, timing, tags, and how its pixels
/// are written.
fn describe(
    sheet: &Sheet,
    at: (&Document, usize, Size),
    tags: &[Tag],
    names: (&str, Named),
    palette: Option<&Palette>,
) -> String {
    let (image, named) = names;
    let mut out = String::from("{\"frames\":");
    push_frames(&mut out, sheet, at, named);
    let _ = write!(out, ",\"meta\":{{{OWNER_MARK},\"image\":");
    json::push_string(&mut out, image);
    out.push(',');
    push_format(&mut out, palette);
    let _ = write!(
        out,
        ",\"size\":{{\"w\":{},\"h\":{}}},\"scale\":\"{}\",\"frameTags\":[",
        sheet.width, sheet.height, at.2 .2
    );
    push_tags(&mut out, tags);
    out.push(']');
    let doc = at.0;
    let (w, h, scale) = at.2;
    // The picture's size is the canvas's times the scale.
    push_slices(&mut out, &doc.slices, (w / scale, h / scale), scale);
    out.push_str("}}\n");
    out
}

/// A sheet laid out and described, its image not yet made.
pub struct SheetPlan {
    pub sheet: Sheet,
    /// The document frame at the first place.
    pub first: usize,
    pub description: String,
    /// For an indexed picture, the sheet's one palette, the space between frames included.
    pub palette: Option<Palette>,
}

/// Lays out `frames` as `options` say and describes the result. `names` are the image's file
/// name and the stem frames are named after, as the description should give them.
#[inline(never)]
pub fn sheet_plan(
    picture: &Picture<'_>,
    frames: RangeInclusive<usize>,
    names: (&str, &str),
    options: &SheetOptions,
) -> Result<SheetPlan, ExportError> {
    let doc = picture.doc();
    let frames = crate::frames::within(doc, &frames)?;
    let (w, h, first) = (picture.width(), picture.height(), *frames.start());
    let tags = tags_within(&doc.tags, &frames);
    let scale = picture.scale();
    let layout = options.layout;
    let sheet = arrange((w, h), frames.count(), &tags, layout).map_err(|e| {
        match (layout == Default::default(), scale) {
            (false, _) => ExportError::LayoutTooLarge { scale },
            (true, 1) => ExportError::Limit(e),
            (true, _) => ExportError::SheetTooLarge { scale },
        }
    })?;
    let covered = sheet.places.len() * w * h;
    let gaps = covered < sheet.width * sheet.height;
    let shown = first..first + sheet.places.len();
    let palette = palette_for(picture, shown, (gaps, None))?;
    let size = (w, h, scale);
    let names = (names.0, (names.1, options.json));
    let description = describe(&sheet, (doc, first, size), &tags, names, palette.as_ref());
    Ok(SheetPlan {
        sheet,
        first,
        description,
        palette,
    })
}

/// A sheet's image and description, made but not yet written.
pub struct SheetFiles {
    pub image: Vec<u8>,
    pub description: String,
}

/// [`sheet_plan`], with the image made in memory.
#[inline(never)]
pub fn sheet_files(
    doc: &Document,
    frames: RangeInclusive<usize>,
    names: (&str, &str),
) -> Result<SheetFiles, ExportError> {
    let picture = Picture::visible(doc);
    let plan = sheet_plan(&picture, frames, names, &SheetOptions::default())?;
    let mut image = Vec::new();
    write_sheet_png(&picture, (&plan.sheet, plan.first), None, &mut image)?;
    Ok(SheetFiles {
        image,
        description: plan.description,
    })
}

/// The name a file has, and the stem frames in its description are named after.
pub fn names_for(path: &Path) -> (String, String) {
    let part = |p: Option<&std::ffi::OsStr>| {
        p.map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    };
    (part(path.file_name()), part(path.file_stem()))
}

/// Only an earlier export may be replaced. The dialog asked about the image, and the command
/// line's --overwrite is for files an export makes, not this file when someone else wrote it.
/// A file larger than any description an export writes is not read, and is not one.
#[inline(never)]
pub fn may_write_description(path: &Path) -> bool {
    use std::io::Read;
    let mut text = String::new();
    let read = std::fs::File::open(path)
        .and_then(|file| file.take(DESCRIPTION_LIMIT + 1).read_to_string(&mut text));
    match read {
        Ok(len) => len as u64 <= DESCRIPTION_LIMIT && text.contains(OWNER_MARK),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

/// Far above the largest description an export writes, which for 4096 frames is under 1 MiB.
const DESCRIPTION_LIMIT: u64 = 16 << 20;

/// Writes the whole document as a sheet and, beside it, its description, unless a description
/// is there that an earlier export did not write. Returns false if that was left alone. Both
/// files are one set, as on the command line: the old description goes before the new image
/// arrives, and the new one last, so a description never stands beside an image it does not fit.
#[inline(never)]
pub fn export_sheet(path: &Path, doc: &Document) -> Result<bool, ExportError> {
    let all = crate::resolve(doc, &crate::Frames::All)?;
    let picture = Picture::visible(doc);
    let options = SheetOptions::default();
    let mut outputs = crate::outputs::sheet_outputs(&picture, all, path.to_path_buf(), &options)?;
    let described = outputs
        .last()
        .is_some_and(|description| may_write_description(&description.path));
    if !described {
        outputs.truncate(1);
    }
    crate::outputs::write_outputs(&mut outputs, &mut |_, _| Ok(())).map_err(|e| {
        let kind = match &e.cause {
            pikslar_format::FormatError::Io(io) => io.kind(),
            _ => std::io::ErrorKind::Other,
        };
        ExportError::from(std::io::Error::new(kind, e.to_string()))
    })?;
    Ok(described)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::{Cel, ColorMode, Document, Grid};

    fn document(frames: usize) -> Document {
        let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("valid");
        for at in 1..frames {
            doc.add_frame(at, false).expect("room");
        }
        doc
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pikslar-sheet-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn the_description_has_rectangles_timing_and_tags() {
        let mut doc = document(2);
        doc.frames[1].duration_ms = 250;
        doc.add_tag("run", 0, 1).expect("valid");
        doc.tags[0].direction = Direction::PingPong;
        let text = sheet_files(&doc, 0..=1, ("a.png", "a"))
            .expect("made")
            .description;
        assert!(text.contains("\"frame\":{\"x\":8,\"y\":0,\"w\":8,\"h\":8}"));
        assert!(text.contains("\"duration\":250"));
        assert!(text.contains("\"name\":\"run\",\"from\":0,\"to\":1,\"direction\":\"pingpong\""));
        assert!(text.contains("\"image\":\"a.png\""));
    }

    #[test]
    fn a_sheet_shows_each_frame_through_its_offset_and_clips_the_rest() {
        const RED: u32 = 0xff00_00ff;
        let mut doc = document(2);
        let mut row = vec![0; 20];
        (row[0], row[9], row[10]) = (RED, RED, RED);
        let picture = || Grid::from_cells(10, 2, &row).into_image();
        // Frame 0: local (0, 0) lands at (-2, 3), off the canvas; (9, 0) at (7, 3).
        doc.put_cel(0, 0, Cel::at(picture(), -2, 3).expect("in range"))
            .expect("frame");
        // Frame 1: local (0, 0) lands at (4, -1), off the canvas; (0, 1) at (4, 0).
        doc.put_cel(0, 1, Cel::at(picture(), 4, -1).expect("in range"))
            .expect("frame");
        let dir = temp("offsets");
        let path = dir.join("walk.png");
        export_sheet(&path, &doc).expect("exports");
        let image = pikslar_format::png::decode(&std::fs::read(&path).expect("read")).expect("png");
        std::fs::remove_dir_all(dir).expect("cleanup");
        let opaque: Vec<(usize, usize)> = (0..image.px.len())
            .filter(|&i| image.px[i] != 0)
            .map(|i| (i % image.width, i / image.width))
            .collect();
        assert_eq!((image.width, image.height), (16, 8));
        assert_eq!(opaque, vec![(12, 0), (7, 3)]);
    }

    #[test]
    fn exporting_writes_both_files_and_can_repeat() {
        let dir = temp("both");
        let path = dir.join("walk.png");
        let doc = document(2);
        assert!(export_sheet(&path, &doc).expect("exports"));
        assert!(export_sheet(&path, &doc).expect("exports again"));
        assert!(dir.join("walk.json").exists());
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn someone_elses_json_is_left_alone() {
        let dir = temp("other");
        std::fs::write(dir.join("walk.json"), "{\"mine\":1}").expect("write");
        let doc = document(1);
        assert!(!export_sheet(&dir.join("walk.png"), &doc).expect("exports"));
        let kept = std::fs::read_to_string(dir.join("walk.json")).expect("read");
        assert_eq!(kept, "{\"mine\":1}");
        assert!(dir.join("walk.png").exists(), "the image is still written");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_sheet_of_some_frames_keeps_their_names_and_counts_tags_from_its_start() {
        let mut doc = document(5);
        doc.frames[3].duration_ms = 70;
        doc.add_tag("walk", 2, 3).expect("valid");
        doc.add_tag("rest", 4, 4).expect("valid");
        let files = sheet_files(&doc, 2..=3, ("w.png", "w")).expect("made");
        let text = files.description;
        assert!(text.contains("\"filename\":\"w 2\""), "{text}");
        assert!(text.contains("\"filename\":\"w 3\""), "{text}");
        assert!(
            !text.contains("\"w 1\"") && !text.contains("\"w 4\""),
            "{text}"
        );
        assert!(text.contains("\"duration\":70"), "{text}");
        assert!(
            text.contains("\"name\":\"walk\",\"from\":0,\"to\":1"),
            "{text}"
        );
        assert!(!text.contains("rest"), "{text}");
        let image = pikslar_format::png::decode(&files.image).expect("png");
        assert_eq!((image.width, image.height), (16, 8));
    }

    #[test]
    fn a_description_that_cannot_be_replaced_stops_the_image_too() {
        let dir = temp("locked");
        let path = dir.join("walk.png");
        assert!(export_sheet(&path, &document(2)).expect("first export"));
        let (image, json) = (std::fs::read(&path).expect("image"), dir.join("walk.json"));
        let text = std::fs::read(&json).expect("description");
        let mut perms = std::fs::metadata(&json).expect("meta").permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&json, perms.clone()).expect("read-only");
        export_sheet(&path, &document(5)).expect_err("refused");
        assert_eq!(
            std::fs::read(&path).expect("image"),
            image,
            "the old image stays"
        );
        assert_eq!(std::fs::read(&json).expect("description"), text);
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&json, perms).expect("writable");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn frames_the_document_does_not_have_are_refused() {
        let doc = document(3);
        for (frames, frame) in [(5..=9, 9), (2..=3, 3)] {
            match sheet_files(&doc, frames, ("w.png", "w")) {
                Err(ExportError::NoSuchFrame {
                    frame: f,
                    frames: 3,
                }) => assert_eq!(f, frame),
                other => panic!("{:?}", other.map(|_| ())),
            }
        }
        #[allow(clippy::reversed_empty_ranges)]
        let backwards = 2..=1;
        assert!(matches!(
            sheet_files(&doc, backwards, ("w.png", "w")),
            Err(ExportError::Backwards { from: 2, to: 1 })
        ));
    }

    #[test]
    fn a_description_too_large_to_be_one_is_not_read_as_ours() {
        let dir = temp("large");
        let json = dir.join("walk.json");
        let mut text = format!("{{{OWNER_MARK}}}");
        assert!(text.len() < 64);
        std::fs::write(&json, &text).expect("small");
        assert!(may_write_description(&json));
        text.push_str(&" ".repeat(DESCRIPTION_LIMIT as usize));
        std::fs::write(&json, &text).expect("large");
        assert!(!may_write_description(&json));
        std::fs::remove_dir_all(dir).expect("cleanup");
    }
}
