//! What to export, chosen in one small panel over the canvas; where to, in the system's save
//! dialog after it. The panel shows only what the chosen kind of file has.

use pikslar_core::{Tag, TagId};
use pikslar_export::{Frames, Job, JsonShape, Kind, Layout, SheetOptions};

/// Which frames: the one shown, every frame, a range, or a tag's, the tag known by who it is
/// rather than where it is in the list, so removing or renaming others never changes which one
/// plays. A range is the first and last frame, counted from 0, both included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameChoice {
    This,
    All,
    Range(usize, usize),
    Tag(TagId),
}

/// The export panel's choices. Kept after an export, so the next one starts from them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportChoice {
    pub kind: Kind,
    pub frames: FrameChoice,
    /// Only the layer being edited, shown or hidden, rather than the visible layers.
    pub this_layer: bool,
    pub scale: usize,
    pub indexed: bool,
    /// An animation plays once rather than forever.
    pub once: bool,
    /// A sheet's frames a row; None, a row for each tag.
    pub columns: Option<usize>,
    pub padding: usize,
    pub hash: bool,
    /// A Tiled map of the frame shown, in place of the kind: its tilemap layers and an atlas
    /// for each tileset.
    pub map: bool,
    /// The kind is Frames only because "All frames" was asked of a still image; "This frame"
    /// takes it back to a PNG, and it is not kept for the next export.
    pub auto_sequence: bool,
}

/// The largest scale and padding the panel steps to. The size check refuses what is too large.
pub const MOST_SCALE: usize = 16;
pub const MOST_PADDING: usize = 64;
pub const MOST_COLUMNS: usize = 64;

impl Default for ExportChoice {
    fn default() -> Self {
        ExportChoice {
            kind: Kind::Png,
            frames: FrameChoice::This,
            this_layer: false,
            scale: 1,
            indexed: false,
            once: false,
            columns: None,
            padding: 0,
            hash: false,
            map: false,
            auto_sequence: false,
        }
    }
}

impl ExportChoice {
    /// The choice for `kind`: a still image is the frame shown, anything else every frame,
    /// unless frames were already chosen for something other than a still image.
    pub fn with_kind(self, kind: Kind) -> Self {
        let frames = match (kind, self.kind, self.frames) {
            (Kind::Png, _, _) => FrameChoice::This,
            (_, Kind::Png, _) => FrameChoice::All,
            (_, _, frames) => frames,
        };
        ExportChoice {
            kind,
            frames,
            map: false,
            auto_sequence: false,
            ..self
        }
    }

    /// What one choice changed in another on its own, said in the status line so that nothing
    /// changes unseen (R1, A16): the format when "All frames" or "This frame" needs another,
    /// and the frames when a format needs other frames. None when only what was chosen changed.
    /// `by_kind` says a format was what was chosen, so the change is named by its frames.
    pub fn changed_by_itself(before: Self, after: Self, by_kind: bool) -> Option<String> {
        if before.map || after.map || before.kind == after.kind {
            return None;
        }
        let shown = |c: Self| match c.kind {
            Kind::Png => FrameChoice::This,
            _ => c.frames,
        };
        let (was, now) = (kind_label(before.kind), kind_label(after.kind));
        if after.auto_sequence && before.kind == Kind::Png {
            return Some(format!(
                "All frames: the format is now {now}, as a {was} holds one frame"
            ));
        }
        if !by_kind && before.auto_sequence && after.kind == Kind::Png {
            return Some(format!("This frame: the format is {now} again"));
        }
        match (shown(before), shown(after)) {
            (FrameChoice::All, FrameChoice::This) => Some(format!(
                "{now}: Frames is now This frame, as a {now} holds one frame"
            )),
            (FrameChoice::This, FrameChoice::All) => {
                Some(format!("{now}: Frames is now All frames"))
            }
            _ => None,
        }
    }

    /// "All frames" of a still image: a picture each, as the Frames kind, until "This frame".
    pub fn all_frames_of_still(self) -> Self {
        ExportChoice {
            auto_sequence: true,
            ..self.with_kind(Kind::Sequence)
        }
    }

    /// "This frame": a PNG again if the Frames kind was only a step towards all frames.
    pub fn this_frame(self) -> Self {
        let back = if self.auto_sequence && self.kind == Kind::Sequence {
            self.with_kind(Kind::Png)
        } else {
            self
        };
        ExportChoice {
            frames: FrameChoice::This,
            ..back
        }
    }

    /// What the next export starts from: a kind chosen only by "All frames" is not kept.
    pub fn settled(self) -> Self {
        if self.auto_sequence {
            ExportChoice {
                frames: FrameChoice::This,
                ..self.with_kind(Kind::Png)
            }
        } else {
            self
        }
    }

    /// The choice of a Tiled map, keeping the rest for when another kind is chosen again.
    pub fn with_map(self) -> Self {
        ExportChoice { map: true, ..self }
    }

    /// A tag choice the document no longer has falls back to every frame, and a range is kept
    /// inside the `count` frames it has.
    pub fn fitted(self, tags: &[Tag], count: usize) -> Self {
        let last = count.saturating_sub(1);
        let frames = match self.frames {
            FrameChoice::Tag(id) if place(tags, id).is_none() => FrameChoice::All,
            FrameChoice::Range(from, to) => {
                let to = to.min(last);
                FrameChoice::Range(from.min(to), to)
            }
            frames => frames,
        };
        ExportChoice { frames, ..self }
    }

    /// The range a Range button starts from: the chosen tag's frames, or from the frame shown
    /// to the last.
    pub fn first_range(&self, frame: usize, tags: &[Tag], count: usize) -> FrameChoice {
        let range = match self.frames {
            FrameChoice::Range(..) => self.frames,
            FrameChoice::Tag(id) => match place(tags, id) {
                Some(i) => FrameChoice::Range(tags[i].from, tags[i].to),
                None => FrameChoice::Range(frame, count.saturating_sub(1)),
            },
            _ => FrameChoice::Range(frame, count.saturating_sub(1)),
        };
        ExportChoice {
            frames: range,
            ..*self
        }
        .fitted(tags, count)
        .frames
    }

    /// The range with its first frame moved to `from`; the last follows if it would be before it.
    pub fn range_from(self, from: usize) -> Self {
        let FrameChoice::Range(_, to) = self.frames else {
            return self;
        };
        ExportChoice {
            frames: FrameChoice::Range(from, to.max(from)),
            ..self
        }
    }

    /// The range with its last frame moved to `to`; the first follows if it would be after it.
    pub fn range_to(self, to: usize) -> Self {
        let FrameChoice::Range(from, _) = self.frames else {
            return self;
        };
        ExportChoice {
            frames: FrameChoice::Range(from.min(to), to),
            ..self
        }
    }

    /// The job this choice asks for, `frame` being the frame shown. None if the tag chosen is
    /// no longer in the project: nothing else is exported in its place.
    pub fn job(&self, frame: usize, tags: &[Tag]) -> Option<Job> {
        let frames = match (self.kind, self.frames) {
            (Kind::Png, _) | (_, FrameChoice::This) => Frames::One(frame),
            (_, FrameChoice::All) => Frames::All,
            (_, FrameChoice::Range(from, to)) => Frames::Range(from, to),
            (kind, FrameChoice::Tag(id)) => {
                let i = place(tags, id)?;
                match kind.animated() {
                    true => Frames::TagAt(i),
                    false => Frames::Range(tags[i].from, tags[i].to),
                }
            }
        };
        Some(Job {
            kind: self.kind,
            frames,
            plays: self.once.then_some(1),
            sheet: SheetOptions {
                layout: Layout {
                    columns: self.columns,
                    padding: self.padding,
                },
                json: if self.hash {
                    JsonShape::Hash
                } else {
                    JsonShape::Array
                },
            },
        })
    }

    /// Indexed only where the kind can be.
    pub fn indexed(&self) -> bool {
        self.indexed && self.kind.may_be_indexed()
    }
}

/// Where the tag `id` is in `tags` now.
pub fn place(tags: &[Tag], id: TagId) -> Option<usize> {
    tags.iter().position(|t| t.id == id)
}

/// The label a kind's button has.
pub fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Png => "PNG",
        Kind::Sheet => "Sprite sheet",
        Kind::Sequence => "PNG sequence",
        Kind::Gif => "GIF",
        Kind::Apng => "APNG",
    }
}

/// What a kind's button says when hovered.
pub fn kind_hint(kind: Kind) -> &'static str {
    match kind {
        Kind::Png => "One image of the frame shown  1",
        Kind::Sheet => "Every frame in one image, and a .json describing them  2",
        Kind::Sequence => "Each frame its own numbered PNG, and a .json listing them  3",
        Kind::Gif => "An animated GIF, each frame for its duration  4",
        Kind::Apng => "An animated PNG, every color and alpha kept  5",
    }
}

/// The system dialog's filter for a kind's file.
pub fn filter(kind: Kind) -> &'static str {
    match kind {
        Kind::Gif => "GIF image\0*.gif\0",
        _ => "PNG image\0*.png\0",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::Direction;

    fn tag(name: &str, from: usize, to: usize) -> Tag {
        Tag {
            name: name.into(),
            from,
            to,
            direction: Direction::PingPong,
            repeat: 0,
            id: TagId::fresh(),
        }
    }

    fn tags() -> Vec<Tag> {
        vec![tag("walk", 2, 4)]
    }

    fn job(choice: &ExportChoice, frame: usize, tags: &[Tag]) -> Job {
        choice.job(frame, tags).expect("the tag is there")
    }

    #[test]
    fn a_still_image_is_the_frame_shown_whatever_else_was_chosen() {
        let tags = tags();
        let choice = ExportChoice {
            frames: FrameChoice::Tag(tags[0].id),
            ..ExportChoice::default()
        };
        assert_eq!(job(&choice, 3, &tags).frames, Frames::One(3));
        let sheet = choice.with_kind(Kind::Sheet).with_kind(Kind::Png);
        assert_eq!(sheet.frames, FrameChoice::This);
    }

    #[test]
    fn leaving_a_still_image_chooses_every_frame_and_later_kinds_keep_the_choice() {
        let sheet = ExportChoice::default().with_kind(Kind::Sheet);
        assert_eq!(sheet.frames, FrameChoice::All);
        let id = tags()[0].id;
        let tagged = ExportChoice {
            frames: FrameChoice::Tag(id),
            ..sheet
        };
        assert_eq!(tagged.with_kind(Kind::Gif).frames, FrameChoice::Tag(id));
    }

    #[test]
    fn a_tag_is_its_frames_and_an_animation_plays_it_in_its_direction() {
        let tags = tags();
        let sheet = ExportChoice {
            frames: FrameChoice::Tag(tags[0].id),
            ..ExportChoice::default().with_kind(Kind::Sheet)
        };
        assert_eq!(job(&sheet, 0, &tags).frames, Frames::Range(2, 4));
        let gif = ExportChoice {
            kind: Kind::Gif,
            ..sheet
        };
        assert_eq!(job(&gif, 0, &tags).frames, Frames::TagAt(0));
        assert_eq!(sheet.fitted(&[], 8).frames, FrameChoice::All);
        assert_eq!(job(&sheet.fitted(&[], 8), 0, &[]).frames, Frames::All);
    }

    #[test]
    fn the_chosen_tag_stays_chosen_when_others_are_removed_or_renamed() {
        let mut tags = vec![tag("idle", 0, 1), tag("walk", 2, 4), tag("run", 5, 6)];
        let walk = tags[1].id;
        let gif = ExportChoice {
            kind: Kind::Gif,
            frames: FrameChoice::Tag(walk),
            ..ExportChoice::default()
        };
        tags.remove(0);
        tags[1].name = "walk".into();
        assert_eq!(gif.fitted(&tags, 8).frames, FrameChoice::Tag(walk));
        assert_eq!(
            job(&gif, 0, &tags).frames,
            Frames::TagAt(0),
            "walk, now first"
        );
        let sheet = ExportChoice {
            kind: Kind::Sheet,
            ..gif
        };
        assert_eq!(job(&sheet, 0, &tags).frames, Frames::Range(2, 4));
    }

    #[test]
    fn a_chosen_tag_that_is_gone_exports_nothing_in_its_place() {
        let mut tags = vec![tag("idle", 0, 1), tag("walk", 2, 4)];
        let walk = tags[1].id;
        let gif = ExportChoice {
            kind: Kind::Gif,
            frames: FrameChoice::Tag(walk),
            ..ExportChoice::default()
        };
        tags.remove(1);
        assert!(gif.job(0, &tags).is_none());
        assert_eq!(
            gif.fitted(&tags, 8).frames,
            FrameChoice::All,
            "shown as all frames"
        );
        let still = ExportChoice {
            kind: Kind::Png,
            ..gif
        };
        assert_eq!(job(&still, 1, &tags).frames, Frames::One(1));
    }

    #[test]
    fn a_range_is_its_frames_for_every_kind_but_a_still_image() {
        let range = ExportChoice {
            frames: FrameChoice::Range(1, 3),
            ..ExportChoice::default()
        };
        assert_eq!(job(&range, 2, &[]).frames, Frames::One(2), "PNG");
        for kind in [Kind::Sheet, Kind::Sequence, Kind::Gif, Kind::Apng] {
            let made = job(&ExportChoice { kind, ..range }, 0, &[]);
            assert_eq!(made.frames, Frames::Range(1, 3), "{kind:?}");
        }
        assert_eq!(
            range.with_kind(Kind::Gif).frames,
            FrameChoice::All,
            "leaving PNG"
        );
        let gif = ExportChoice {
            kind: Kind::Gif,
            ..range
        };
        assert_eq!(gif.with_kind(Kind::Sheet).frames, FrameChoice::Range(1, 3));
    }

    #[test]
    fn a_range_stays_inside_the_frames_and_in_order() {
        let range = ExportChoice {
            kind: Kind::Gif,
            frames: FrameChoice::Range(4, 9),
            ..ExportChoice::default()
        };
        assert_eq!(range.fitted(&[], 6).frames, FrameChoice::Range(4, 5));
        assert_eq!(range.fitted(&[], 3).frames, FrameChoice::Range(2, 2));
        assert_eq!(range.fitted(&[], 0).frames, FrameChoice::Range(0, 0));
        assert_eq!(range.range_from(7).frames, FrameChoice::Range(7, 9));
        assert_eq!(range.range_from(12).frames, FrameChoice::Range(12, 12));
        assert_eq!(range.range_to(2).frames, FrameChoice::Range(2, 2));
        assert_eq!(range.range_to(6).frames, FrameChoice::Range(4, 6));
        let all = ExportChoice {
            frames: FrameChoice::All,
            ..range
        };
        assert_eq!(all.range_from(3), all, "only a range has ends to move");
        assert_eq!(all.range_to(3), all);
    }

    #[test]
    fn a_new_range_starts_from_the_tag_chosen_or_the_frame_shown() {
        let tags = tags();
        let tagged = ExportChoice {
            kind: Kind::Sheet,
            frames: FrameChoice::Tag(tags[0].id),
            ..ExportChoice::default()
        };
        assert_eq!(tagged.first_range(0, &tags, 8), FrameChoice::Range(2, 4));
        let all = ExportChoice {
            frames: FrameChoice::All,
            ..tagged
        };
        assert_eq!(all.first_range(5, &tags, 8), FrameChoice::Range(5, 7));
        let gone = ExportChoice {
            frames: FrameChoice::Tag(TagId::fresh()),
            ..tagged
        };
        assert_eq!(gone.first_range(1, &tags, 3), FrameChoice::Range(1, 2));
        let kept = ExportChoice {
            frames: FrameChoice::Range(1, 2),
            ..tagged
        };
        assert_eq!(kept.first_range(6, &tags, 8), FrameChoice::Range(1, 2));
    }

    #[test]
    fn the_rest_of_the_choice_reaches_the_job_and_indexed_only_where_it_can_be() {
        let choice = ExportChoice {
            kind: Kind::Sheet,
            frames: FrameChoice::All,
            indexed: true,
            once: true,
            columns: Some(3),
            padding: 2,
            hash: true,
            ..ExportChoice::default()
        };
        let made = job(&choice, 0, &[]);
        assert_eq!(made.plays, Some(1));
        assert_eq!(
            made.sheet.layout,
            Layout {
                columns: Some(3),
                padding: 2
            }
        );
        assert_eq!(made.sheet.json, JsonShape::Hash);
        assert!(choice.indexed());
        for kind in [Kind::Gif, Kind::Apng] {
            assert!(!ExportChoice { kind, ..choice }.indexed());
        }
        let plain = job(&ExportChoice::default(), 0, &[]);
        assert_eq!((plain.plays, plain.sheet), (None, SheetOptions::default()));
    }
}
