//! Which frames an export takes. Every format resolves the same choice the same way.

use crate::ExportError;
use pikslar_core::{cumulative, Direction, Document, Playhead, Span, Tag};
use std::ops::RangeInclusive;

/// The frames asked for, numbered from 0. People count from 1; the command line converts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frames {
    All,
    One(usize),
    /// From and to, both included.
    Range(usize, usize),
    /// The first tag with this name.
    Tag(String),
    /// The tag at this place in the document's list, counted from 0. Names may repeat; a place
    /// does not.
    TagAt(usize),
}

/// The tag `which` names, if it names one, or why the document has none.
fn tag<'a>(doc: &'a Document, which: &Frames) -> Result<Option<&'a Tag>, ExportError> {
    match which {
        Frames::Tag(name) => doc
            .tags
            .iter()
            .find(|t| t.name == *name)
            .map(Some)
            .ok_or_else(|| ExportError::NoSuchTag(name.clone())),
        Frames::TagAt(i) => doc
            .tags
            .get(*i)
            .map(Some)
            .ok_or_else(|| ExportError::NoSuchTag(format!("number {}", i + 1))),
        _ => Ok(None),
    }
}

/// The frames, in document order, or why the document does not have them.
#[inline(never)]
pub fn resolve(doc: &Document, which: &Frames) -> Result<RangeInclusive<usize>, ExportError> {
    let (from, to) = match which {
        Frames::All => (0, doc.frames.len().saturating_sub(1)),
        Frames::One(frame) => (*frame, *frame),
        Frames::Range(from, to) => (*from, *to),
        // Always a tag here: tag() answers None only for the other choices.
        Frames::Tag(_) | Frames::TagAt(_) => tag(doc, which)?.map_or((0, 0), |t| (t.from, t.to)),
    };
    within(doc, &(from..=to))
}

/// The frames chosen, in the order an animation plays them: a tag's own direction, so reverse
/// plays it backwards and ping-pong forward then back without repeating either end (0 1 2 3 2
/// 1); any other choice forward.
#[inline(never)]
pub fn play_order(doc: &Document, which: &Frames) -> Result<Vec<usize>, ExportError> {
    let forward: Vec<usize> = resolve(doc, which)?.collect();
    let direction = tag(doc, which)?.map(|t| t.direction);
    Ok(match direction {
        Some(Direction::Reverse) => forward.into_iter().rev().collect(),
        Some(Direction::PingPong) if forward.len() > 2 => {
            let back = forward[1..forward.len() - 1].iter().rev().copied();
            forward.iter().copied().chain(back).collect()
        }
        _ => forward,
    })
}

/// The frames an animation of `which` shows, and how many times it plays them, 0 for forever.
/// `asked` wins; without it a tag plays as many passes as it is set to and anything else plays
/// forever. A file can only loop the whole of its frames, so a ping-pong played a set number of
/// times is written out pass by pass and played once.
#[inline(never)]
pub fn animation_order(
    doc: &Document,
    which: &Frames,
    asked: Option<u32>,
) -> Result<(Vec<usize>, u32), ExportError> {
    let order = play_order(doc, which)?;
    let tag = tag(doc, which)?;
    let repeat = tag.map_or(0, |t| t.repeat);
    Ok(match (asked, tag) {
        (Some(plays), _) => (order, plays),
        (None, Some(t)) if repeat != 0 && t.direction == Direction::PingPong => {
            (passes(t, repeat), 1)
        }
        (None, _) => (order, u32::from(repeat)),
    })
}

/// Every frame a tag shows over `repeat` passes, as the editor plays it.
fn passes(tag: &Tag, repeat: u16) -> Vec<usize> {
    let span = Span {
        from: tag.from,
        to: tag.to,
        direction: tag.direction,
    };
    let (mut head, mut frame) = (Playhead::default(), tag.from);
    let mut order = vec![frame];
    while let Some(next) = span.step_counted(frame, &mut head, repeat) {
        frame = next;
        order.push(frame);
    }
    order
}

/// How long each of `frames` shows in milliseconds, for a description. A document timed by frame
/// rate has ticks that are not whole milliseconds, so each is the running total rounded less the
/// one before: the times add up to the exact total, to the millisecond, however many there are.
pub(crate) fn describe_durations(
    doc: &Document,
    frames: impl IntoIterator<Item = usize>,
) -> Vec<u32> {
    cumulative(
        frames.into_iter().map(|f| doc.frame_units(f)),
        doc.units_per_second(),
        1000,
    )
}

/// `range`, if the document has every frame of it.
pub(crate) fn within(
    doc: &Document,
    range: &RangeInclusive<usize>,
) -> Result<RangeInclusive<usize>, ExportError> {
    let (from, to, frames) = (*range.start(), *range.end(), doc.frames.len());
    if from > to {
        return Err(ExportError::Backwards { from, to });
    }
    if to >= frames {
        return Err(ExportError::NoSuchFrame { frame: to, frames });
    }
    Ok(from..=to)
}

/// The tags that overlap `range`, cut to it and counted from its first frame.
pub(crate) fn tags_within(tags: &[Tag], range: &RangeInclusive<usize>) -> Vec<Tag> {
    let (first, last) = (*range.start(), *range.end());
    tags.iter()
        .filter(|t| t.from <= last && t.to >= first)
        .map(|t| Tag {
            name: t.name.clone(),
            from: t.from.max(first) - first,
            to: t.to.min(last) - first,
            direction: t.direction,
            repeat: t.repeat,
            id: pikslar_core::TagId::fresh(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::{ColorMode, Direction};

    fn doc(frames: usize) -> Document {
        let mut doc = Document::new(2, 2, ColorMode::Rgba).expect("valid");
        for at in 1..frames {
            doc.add_frame(at, false).expect("room");
        }
        doc.add_tag("walk", 1, 3).expect("tag");
        doc.add_tag("idle", 4, 5).expect("tag");
        doc
    }

    #[test]
    fn each_choice_resolves_to_its_frames() {
        let d = doc(6);
        assert_eq!(resolve(&d, &Frames::All).expect("all"), 0..=5);
        assert_eq!(resolve(&d, &Frames::One(5)).expect("one"), 5..=5);
        assert_eq!(resolve(&d, &Frames::Range(2, 4)).expect("range"), 2..=4);
        assert_eq!(
            resolve(&d, &Frames::Tag("walk".into())).expect("tag"),
            1..=3
        );
    }

    #[test]
    fn frames_the_document_lacks_are_errors() {
        let d = doc(6);
        assert!(matches!(
            resolve(&d, &Frames::One(6)),
            Err(ExportError::NoSuchFrame {
                frame: 6,
                frames: 6
            })
        ));
        assert!(matches!(
            resolve(&d, &Frames::Range(0, 6)),
            Err(ExportError::NoSuchFrame {
                frame: 6,
                frames: 6
            })
        ));
        assert!(matches!(
            resolve(&d, &Frames::Range(3, 2)),
            Err(ExportError::Backwards { from: 3, to: 2 })
        ));
        assert!(matches!(
            resolve(&d, &Frames::Tag("run".into())),
            Err(ExportError::NoSuchTag(name)) if name == "run"
        ));
    }

    #[test]
    fn tags_are_cut_to_the_range_and_counted_from_its_start() {
        let d = doc(6);
        let within = tags_within(&d.tags, &(2..=4));
        let spans: Vec<_> = within
            .iter()
            .map(|t| (t.name.as_str(), t.from, t.to))
            .collect();
        assert_eq!(spans, [("walk", 0, 1), ("idle", 2, 2)]);
        assert!(within.iter().all(|t| t.direction == Direction::Forward));
        assert!(tags_within(&d.tags, &(0..=0)).is_empty());
        let after: Vec<_> = tags_within(&d.tags, &(4..=5))
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(
            after,
            ["idle"],
            "a tag that ends before the range is left out"
        );
    }
}
