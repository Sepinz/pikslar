//! PNG sequences: one file per frame, numbered from 1 in the order exported, and a description
//! written last, so that its presence means the files it lists are one whole export.

use crate::frames::tags_within;
use crate::json;
use crate::palette::{palette_for, push_palette_fields, Palette};
use crate::sheet::{push_tags, OWNER_MARK};
use crate::{ExportError, Picture};
use std::fmt::Write;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

/// Digits in a member's number. A document has at most 4096 frames, so four always suffice and
/// the files sort in order by name.
const DIGITS: usize = 4;

/// The name of the member at `position`, counted from 0: `walk_0001.png` for the first.
pub fn member_name(stem: &str, position: usize) -> String {
    format!("{stem}_{:0DIGITS$}.png", position + 1)
}

/// The files of a sequence, worked out but not written.
pub struct SequencePlan {
    /// Each member's path and the document frame it shows, in order.
    pub members: Vec<(PathBuf, usize)>,
    /// For an indexed picture, each member's own palette, in the same order; otherwise none.
    pub palettes: Vec<Option<Palette>>,
    /// The description's path: the stem with `.json`.
    pub description: PathBuf,
    pub text: String,
}

/// The stem every member is named from, and the folder they go in, from the path asked for.
fn stem_and_folder(path: &Path) -> (String, PathBuf) {
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let folder = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    (stem, folder)
}

/// Names each frame of `frames` beside `path`, which itself is not written, and describes them.
#[inline(never)]
pub fn sequence_plan(
    picture: &Picture<'_>,
    frames: RangeInclusive<usize>,
    path: &Path,
) -> Result<SequencePlan, ExportError> {
    let doc = picture.doc();
    let frames = crate::frames::within(doc, &frames)?;
    let (stem, folder) = stem_and_folder(path);
    let first = *frames.start();
    let members: Vec<(PathBuf, usize)> = frames
        .clone()
        .enumerate()
        .map(|(position, frame)| (folder.join(member_name(&stem, position)), frame))
        .collect();
    let palettes = members
        .iter()
        .map(|&(_, frame)| palette_for(picture, [frame], (false, Some(frame))))
        .collect::<Result<Vec<_>, _>>()?;
    let times = crate::frames::describe_durations(doc, members.iter().map(|&(_, f)| f));
    let mut text = String::from("{\"frames\":[");
    for (position, (&(_, frame), palette)) in members.iter().zip(&palettes).enumerate() {
        if position > 0 {
            text.push(',');
        }
        text.push_str("{\"filename\":");
        json::push_string(&mut text, &member_name(&stem, position));
        let duration = times[position];
        let _ = write!(text, ",\"frame\":{frame},\"duration\":{duration}");
        if let Some(palette) = palette {
            text.push(',');
            push_palette_fields(&mut text, palette);
        }
        text.push('}');
    }
    let format = match picture.indexed() {
        true => "INDEXED",
        false => "RGBA8888",
    };
    let (w, h, scale) = (picture.width(), picture.height(), picture.scale());
    let _ = write!(
        text,
        "],\"meta\":{{{OWNER_MARK},\"format\":\"{format}\",\"size\":{{\"w\":{w},\"h\":{h}}},\
         \"scale\":\"{scale}\",\"firstFrame\":{first},\"frameTags\":["
    );
    push_tags(&mut text, &tags_within(&doc.tags, &frames));
    text.push_str("]}}\n");
    Ok(SequencePlan {
        members,
        palettes,
        description: path.with_extension("json"),
        text,
    })
}

/// The number in a member's name, if `name` is `prefix`, at least four digits and `.png`. Case
/// is ignored as Windows ignores it, for ASCII letters.
fn member_number(name: &str, prefix: &str) -> Option<usize> {
    let rest = name
        .get(..prefix.len())
        .filter(|start| start.eq_ignore_ascii_case(prefix))
        .and_then(|_| name.get(prefix.len()..))?;
    let cut = rest.len().checked_sub(".png".len())?;
    let (digits, ext) = (rest.get(..cut)?, rest.get(cut..)?);
    let all_digits = digits.len() >= DIGITS && digits.bytes().all(|b| b.is_ascii_digit());
    (all_digits && ext.eq_ignore_ascii_case(".png"))
        .then(|| digits.parse().ok())
        .flatten()
}

/// Members an earlier, longer sequence of this name left past the `count` this one writes.
/// They would look like part of it, so an export must not go ahead while they are there.
#[inline(never)]
pub fn stale_members(path: &Path, count: usize) -> std::io::Result<Vec<PathBuf>> {
    let (stem, folder) = stem_and_folder(path);
    let folder = if folder.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        folder
    };
    let prefix = format!("{stem}_");
    let mut stale = Vec::new();
    for entry in std::fs::read_dir(&folder)? {
        let name = entry?.file_name();
        let Some(name) = name.to_str() else { continue };
        if member_number(name, &prefix).is_some_and(|n| n > count) {
            stale.push(name.to_string());
        }
    }
    // Every path is the folder and one name, so the paths sort as their names do.
    pikslar_core::sorting::sort_by_text(&mut stale, String::as_str);
    Ok(stale.iter().map(|name| folder.join(name)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::{ColorMode, Direction, Document, Tag};

    fn doc(frames: usize) -> Document {
        let mut d = Document::new(3, 2, ColorMode::Rgba).expect("valid");
        for _ in 1..frames {
            d.frames.push(d.frames[0]);
        }
        for (i, f) in d.frames.iter_mut().enumerate() {
            f.duration_ms = 10 * (i as u32 + 1);
        }
        d
    }

    #[test]
    fn members_are_numbered_from_one_in_four_digits() {
        assert_eq!(member_name("walk", 0), "walk_0001.png");
        assert_eq!(member_name("walk", 4095), "walk_4096.png");
    }

    #[test]
    fn the_description_lists_each_file_its_frame_and_duration() {
        let mut d = doc(5);
        d.tags.push(Tag {
            name: "run".into(),
            from: 1,
            to: 4,
            direction: Direction::PingPong,
            repeat: 0,
            id: pikslar_core::TagId::fresh(),
        });
        let plan = sequence_plan(&Picture::visible(&d), 2..=3, Path::new("out/walk.png"))
            .expect("planned");
        let names: Vec<_> = plan.members.iter().map(|(p, f)| (p.clone(), *f)).collect();
        assert_eq!(
            names,
            [
                (Path::new("out").join("walk_0001.png"), 2),
                (Path::new("out").join("walk_0002.png"), 3)
            ]
        );
        assert_eq!(plan.description, Path::new("out/walk.json"));
        assert_eq!(
            plan.text,
            "{\"frames\":[{\"filename\":\"walk_0001.png\",\"frame\":2,\"duration\":30},\
             {\"filename\":\"walk_0002.png\",\"frame\":3,\"duration\":40}],\
             \"meta\":{\"app\":\"Pikslar\",\"format\":\"RGBA8888\",\"size\":{\"w\":3,\"h\":2},\
             \"scale\":\"1\",\"firstFrame\":2,\"frameTags\":[{\"name\":\"run\",\"from\":0,\"to\":1,\
             \"direction\":\"pingpong\"}]}}\n"
        );
    }

    #[test]
    fn only_higher_numbered_members_of_this_name_are_stale() {
        let d = std::env::temp_dir().join(format!("pikslar-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dir");
        for name in [
            "walk_0001.png",
            "walk_0003.png",
            "walk_0004.PNG",
            "walk_00010.png",
            "walk_0004.json",
            "walk_x004.png",
            "walk_004.png",
            "walker_0009.png",
            "walk.png",
        ] {
            std::fs::write(d.join(name), b"").expect("write");
        }
        let stale = stale_members(&d.join("walk.png"), 3).expect("listed");
        let names: Vec<_> = stale
            .iter()
            .map(|p| p.file_name().expect("name").to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["walk_00010.png", "walk_0004.PNG"]);
        assert!(stale_members(&d.join("walk.png"), 10)
            .expect("listed")
            .is_empty());
        std::fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn members_left_under_another_case_of_the_name_are_found() {
        let d = std::env::temp_dir().join(format!("pikslar-seq-case-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dir");
        for name in [
            "Walk_0006.png",
            "WALK_0007.PNG",
            "walk_0002.png",
            "Walker_0009.png",
        ] {
            std::fs::write(d.join(name), "old").expect("write");
        }
        let stale = stale_members(&d.join("walk.png"), 5).expect("listed");
        assert_eq!(stale, [d.join("WALK_0007.PNG"), d.join("Walk_0006.png")]);
        std::fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_sequence_of_frames_the_document_does_not_have_is_refused() {
        let d = doc(3);
        assert!(matches!(
            sequence_plan(&Picture::visible(&d), 1..=3, Path::new("walk.png")),
            Err(ExportError::NoSuchFrame {
                frame: 3,
                frames: 3
            })
        ));
    }

    #[test]
    fn a_scaled_sequence_describes_its_files_at_their_size() {
        let d = doc(2);
        let layers = crate::Layers::Visible;
        let picture = Picture::new(&d, &layers, 3).expect("drawn");
        let plan = sequence_plan(&picture, 0..=1, Path::new("walk.png")).expect("planned");
        assert!(
            plan.text
                .contains("\"size\":{\"w\":9,\"h\":6},\"scale\":\"3\""),
            "{}",
            plan.text
        );
    }
}
