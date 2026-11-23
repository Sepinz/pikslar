//! One export as asked for (its kind of file, frames, plays and sheet layout) and every file it
//! makes. The command line and the app both export through here, so the same choices give the
//! same bytes from either.

use crate::{
    animation_order, apng_output, frame_output, gif_delays_for, gif_output, resolve,
    sequence_outputs, sheet_outputs, ExportError, Frames, Output, Picture, SheetOptions,
};
use std::path::PathBuf;

/// The kinds of file an export makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Png,
    Sheet,
    Sequence,
    Gif,
    Apng,
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::Png,
        Kind::Sheet,
        Kind::Sequence,
        Kind::Gif,
        Kind::Apng,
    ];

    /// The ending its file has. An APNG is a PNG, so it ends as one.
    pub fn extension(self) -> &'static str {
        match self {
            Kind::Gif => "gif",
            Kind::Png | Kind::Sheet | Kind::Sequence | Kind::Apng => "png",
        }
    }

    /// With no choice made, a PNG is the first frame, and everything else every frame.
    pub fn default_frames(self) -> Frames {
        match self {
            Kind::Png => Frames::One(0),
            Kind::Sheet | Kind::Sequence | Kind::Gif | Kind::Apng => Frames::All,
        }
    }

    /// One file that plays its frames, rather than still images.
    pub fn animated(self) -> bool {
        matches!(self, Kind::Gif | Kind::Apng)
    }

    /// Whether it can be written as indexed PNGs.
    pub fn may_be_indexed(self) -> bool {
        matches!(self, Kind::Png | Kind::Sheet | Kind::Sequence)
    }
}

/// What to export, apart from the layers, scale and kind of PNG, which the picture holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub kind: Kind,
    pub frames: Frames,
    /// How many times an animation plays, 0 for forever; `None` for a tag's own repeat count,
    /// or forever when the frames are no tag.
    pub plays: Option<u32>,
    pub sheet: SheetOptions,
}

impl Job {
    /// A job of `kind` with every choice at its default.
    #[inline(never)]
    pub fn new(kind: Kind) -> Job {
        Job {
            kind,
            frames: kind.default_frames(),
            plays: None,
            sheet: SheetOptions::default(),
        }
    }
}

/// Every file an export makes, not yet written, and anything the person should know.
pub struct Planned<'a> {
    pub outputs: Vec<Output<'a>>,
    pub note: Option<String>,
}

/// The shortest frame a project holds, which some browsers show as 100 ms in an APNG.
pub const SHORTEST_MS: u32 = 10;

/// Plans `job` from `picture` into `path`. Nothing is written, and no file is touched.
#[inline(never)]
pub fn plan<'a>(
    job: &Job,
    picture: &'a Picture<'a>,
    path: PathBuf,
) -> Result<Planned<'a>, ExportError> {
    let doc = picture.doc();
    let outputs = match job.kind {
        Kind::Gif | Kind::Apng => return animation(job, picture, path),
        Kind::Png => {
            let frames = resolve(doc, &job.frames)?;
            let count = frames.clone().count();
            if count != 1 {
                return Err(ExportError::OneFrame { count });
            }
            vec![frame_output(picture, *frames.start(), path)?]
        }
        Kind::Sheet => sheet_outputs(picture, resolve(doc, &job.frames)?, path, &job.sheet)?,
        Kind::Sequence => sequence_outputs(picture, resolve(doc, &job.frames)?, &path)?,
    };
    Ok(Planned {
        outputs,
        note: None,
    })
}

/// A GIF or APNG of the chosen frames in the order they play, with a note if a player may show
/// some longer than asked: a GIF's raised to 20 ms, or an APNG's of 10 ms.
fn animation<'a>(
    job: &Job,
    picture: &'a Picture<'a>,
    path: PathBuf,
) -> Result<Planned<'a>, ExportError> {
    let doc = picture.doc();
    let (order, plays) = animation_order(doc, &job.frames, job.plays)?;
    let (count, gif) = (order.len(), job.kind == Kind::Gif);
    let short = match gif {
        true => gif_delays_for(doc, &order).1,
        false => order
            .iter()
            .map(|&f| doc.frames[f].duration_ms)
            .filter(|&d| d <= SHORTEST_MS)
            .count(),
    };
    let output = match gif {
        true => gif_output(picture, order, plays, path)?,
        false => apng_output(picture, order, plays, path)?,
    };
    let note = match (count, short, job.kind) {
        (1, _, _) | (_, 0, _) => None,
        (_, n, Kind::Gif) => Some(format!(
            "{n} frame(s) would round to less than 20 ms, which most players show as 100 ms, so \
             in the GIF they play for 20 ms"
        )),
        (_, n, _) => Some(format!(
            "{n} frame(s) last 10 ms. The APNG keeps 10 ms, but some browsers show frames that \
             short as 100 ms"
        )),
    };
    Ok(Planned {
        outputs: vec![output],
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picture_tests::corpus;
    use crate::Frames;

    #[test]
    fn a_png_is_one_frame_and_more_are_refused_with_how_many() {
        let doc = corpus()
            .into_iter()
            .find(|d| d.frames.len() >= 3)
            .expect("an animated project");
        let picture = Picture::visible(&doc);
        let job = |frames| Job {
            frames,
            ..Job::new(Kind::Png)
        };
        let path = PathBuf::from("a.png");
        assert!(plan(&job(Frames::One(1)), &picture, path.clone()).is_ok());
        for (frames, count) in [(Frames::Range(0, 1), 2), (Frames::All, doc.frames.len())] {
            match plan(&job(frames), &picture, path.clone()) {
                Err(ExportError::OneFrame { count: n }) => assert_eq!(n, count),
                other => panic!("{:?}", other.map(|_| ())),
            }
        }
    }
}
