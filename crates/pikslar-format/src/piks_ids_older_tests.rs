//! What a build that does not know `Nids` does to a file this build wrote, and what this build
//! makes of it again. The older build is acted out with its own rules: it carries the section
//! unchanged (`piks_sections::keep_foreign`), writes no ids of its own, and numbers a new layer
//! one past the highest live id, copied below from `studio-opt1`.

use super::{
    add_top_layer, contains, dot, edited, frame_ids, layer_ids, plain_three, remove_top_layer,
    section_of,
};
use crate::piks::{load, load_noted, save, LoadNotes};
use crate::piks_ids::IDS;
use crate::piks_v3::encode_with;
use pikslar_core::{
    ColorMode, Document, Editor, ForeignSection, FrameCmd, LayerCmd, PersistedLayerId,
};
use std::sync::Arc;

/// `studio-opt1`'s `Document::unused_persisted_id` (`crates/pikslar-core/src/document.rs`),
/// copied: "One past the largest number in use, or the smallest free one if that would not
/// fit." `studio-m4` to `studio-m11` have the same rule; none keeps a floor.
fn unused_persisted_id(live: &[u32]) -> u32 {
    let taken = |id: &u32| live.contains(id);
    let last = live.iter().copied().max();
    let next = Some(last.unwrap_or(0).saturating_add(1)).filter(|&id| id <= PersistedLayerId::MAX);
    // At most 256 layers, so one of the first 257 numbers is free.
    let free = || (1..).find(|id| !taken(id)).unwrap_or(1);
    next.unwrap_or_else(free)
}

/// A build that does not know `Nids`, with a file of this build's open.
struct Older {
    doc: Document,
    carried: Vec<u8>,
}

impl Older {
    /// Opens `bytes` as it does: the art and the layer records' ids, no frame ids, no floors.
    /// To this build's reader the section is made one it does not know, then left out.
    fn open(bytes: &[u8]) -> Older {
        let carried = section_of(bytes);
        let mut unknown = bytes.to_vec();
        let at = unknown.windows(4).position(|w| w == IDS).expect("written");
        unknown[at + 3] = b'z';
        let end = unknown.len() - 4;
        let crc = crate::crc32(&[&unknown[..end]]);
        unknown[end..].copy_from_slice(&crc.to_le_bytes());
        let mut doc = load(&unknown).expect("opens");
        doc.foreign.retain(|f| f.tag != *b"Nidz");
        Older { doc, carried }
    }

    /// Any edit its commands make the same way this build's do.
    fn edit(&mut self, change: impl FnOnce(&mut Editor)) {
        let mut e = Editor::new(self.doc.clone());
        change(&mut e);
        self.doc = e.doc().clone();
    }

    /// A new layer at the top, numbered by its rule.
    fn add_layer(&mut self) -> u32 {
        let live = layer_ids(&self.doc);
        let id = unused_persisted_id(&live);
        let top = self.doc.layers().len();
        self.doc.add_layer(top, ColorMode::Rgba).expect("room");
        let numbered: Vec<u32> = live.into_iter().chain([id]).collect();
        self.doc.keep_persisted_ids(&numbered).expect("distinct");
        id
    }

    /// What it saves: its own sections, then the carried one unchanged.
    fn save(mut self) -> Vec<u8> {
        self.doc.foreign.push(ForeignSection {
            tag: *IDS,
            data: Arc::from(&self.carried[..]),
        });
        encode_with(&self.doc, &[], false)
    }
}

/// `bytes` through one session of an older build making `change`.
fn through_older_build(bytes: &[u8], change: impl FnOnce(&mut Older)) -> Vec<u8> {
    let mut older = Older::open(bytes);
    change(&mut older);
    older.save()
}

fn reopened(bytes: &[u8]) -> (Document, LoadNotes) {
    load_noted(bytes).expect("opens")
}

#[test]
fn a_file_an_older_build_only_opened_and_saved_keeps_its_ids() {
    let e = edited();
    let (back, notes) = reopened(&through_older_build(&save(e.doc()), |_| {}));
    assert_eq!(notes, LoadNotes::default());
    assert_eq!(frame_ids(&back), frame_ids(e.doc()));
    assert_eq!(layer_ids(&back), layer_ids(e.doc()));
    assert_eq!(back.layer_floor(), e.doc().layer_floor());
}

#[test]
fn layer_settings_an_older_build_changed_keep_every_id() {
    let e = edited();
    let carried = through_older_build(&save(e.doc()), |older| {
        older.edit(|o| {
            let layer = o.layer_id(0).expect("a layer");
            o.apply(LayerCmd::SetOpacity { layer, opacity: 90 })
                .expect("opacity");
            o.apply(LayerCmd::Rename {
                layer,
                name: "sky".into(),
            })
            .expect("renames");
        })
    });
    let (back, notes) = reopened(&carried);
    assert_eq!(notes, LoadNotes::default());
    assert_eq!(frame_ids(&back), frame_ids(e.doc()));
    assert_eq!(layer_ids(&back), layer_ids(e.doc()));
}

/// Three layers, each drawn on, and a fourth removed, so the file carries a section with the
/// layers 1, 2 and 3 and the layer floor 4.
fn three_drawn_layers() -> Editor {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("valid"));
    for x in 0..3 {
        if x > 0 {
            add_top_layer(&mut e);
        }
        e.layer = x as usize;
        dot(&mut e, 0, x);
    }
    add_top_layer(&mut e);
    remove_top_layer(&mut e);
    assert_eq!(
        (layer_ids(e.doc()), e.doc().layer_floor()),
        (vec![1, 2, 3], 4)
    );
    e
}

/// The review's case: in one session an older build removes the top layer and adds one, which
/// its rule numbers with the removed layer's id. That id is in the saved list, so only the check
/// beside it shows that it is now another layer's.
#[test]
fn a_listed_layer_s_id_an_older_build_gave_again_in_one_session_is_renumbered() {
    let saved = save(three_drawn_layers().doc());
    for draw_on_it in [false, true] {
        let mut given = 0;
        let carried = through_older_build(&saved, |older| {
            older.edit(remove_top_layer);
            given = older.add_layer();
            if draw_on_it {
                older.edit(|o| {
                    o.layer = 2;
                    dot(o, 0, 5);
                });
            }
        });
        assert_eq!(
            given, 3,
            "the older build gave the removed layer's id again"
        );
        let (back, notes) = reopened(&carried);
        assert!(
            notes.layer_ids_renumbered && notes.can_save_over(),
            "{draw_on_it}"
        );
        assert_eq!(
            layer_ids(&back)[..2],
            [1, 2],
            "{draw_on_it}: the others keep theirs"
        );
        assert!(
            layer_ids(&back)[2] > 4,
            "{draw_on_it}: {:?}",
            layer_ids(&back)
        );
        assert!(notes.message().expect("told").contains("new layer ids"));
        let again = load(&save(&back)).expect("opens");
        assert_eq!(layer_ids(&again), layer_ids(&back), "kept from now on");
    }
}

/// `studio-m12` keeps a floor while a file is open but saves none, so it does the same across
/// two sessions: remove and save, then open again and add.
#[test]
fn a_listed_layer_s_id_given_again_in_a_second_session_is_renumbered() {
    let saved = save(three_drawn_layers().doc());
    let first = through_older_build(&saved, |older| older.edit(remove_top_layer));
    let second = through_older_build(&first, |older| older.edit(add_top_layer));
    let older = Older::open(&second);
    assert_eq!(
        layer_ids(&older.doc),
        [1, 2, 3],
        "the removed layer's id again"
    );
    let (back, notes) = reopened(&second);
    assert!(notes.layer_ids_renumbered);
    assert_eq!(layer_ids(&back)[..2], [1, 2]);
    assert!(layer_ids(&back)[2] > 4);
}

/// The check is of what a layer is and shows, so drawing on a listed layer in an older build
/// also renews its id: a reference to it is then refused, never taken for another layer.
#[test]
fn a_listed_layer_an_older_build_drew_on_gets_a_new_id() {
    let saved = save(three_drawn_layers().doc());
    let carried = through_older_build(&saved, |older| {
        older.edit(|o| {
            o.layer = 1;
            dot(o, 0, 6);
        })
    });
    let (back, notes) = reopened(&carried);
    assert!(notes.layer_ids_renumbered);
    let ids = layer_ids(&back);
    assert_eq!((ids[0], ids[2]), (1, 3));
    assert!(ids[1] > 4, "{ids:?}");
}

/// An older build gives a new layer one more than the highest live id, which can be the id of a
/// layer removed before it opened the file: not in the saved list, and not above its floor.
#[test]
fn an_unlisted_layer_at_or_below_the_floor_is_renumbered() {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("valid"));
    add_top_layer(&mut e);
    let removed = *layer_ids(e.doc()).last().expect("a layer");
    remove_top_layer(&mut e);
    let carried = through_older_build(&save(e.doc()), |older| {
        assert_eq!(older.add_layer(), removed);
    });
    let (back, notes) = reopened(&carried);
    assert!(notes.layer_ids_renumbered && notes.can_save_over());
    assert_eq!(layer_ids(&back)[0], 1);
    assert!(layer_ids(&back)[1] > removed);
    assert!(!notes.lines().is_empty());
}

/// A middle layer removed leaves the floor at the top layer's id, so the ids an older build
/// gives are above it, never given before, and kept. (A removed frame makes the file carry a
/// section; with none, the layer floor is the top id and there is nothing to check.)
#[test]
fn layers_an_older_build_added_above_the_floor_keep_their_ids() {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("valid"));
    add_top_layer(&mut e);
    add_top_layer(&mut e);
    let layer = e.layer_id(1).expect("a layer");
    e.apply(LayerCmd::Remove { layer }).expect("removes");
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    e.apply(FrameCmd::Remove { from: 0, to: 0 })
        .expect("removes");
    let saved = save(e.doc());
    assert!(contains(&saved, IDS), "frame 1 was removed");
    let (back, notes) = reopened(&through_older_build(&saved, |older| {
        older.add_layer();
        older.add_layer();
    }));
    assert!(!notes.layer_ids_renumbered);
    assert_eq!(layer_ids(&back), [1, 3, 4, 5]);
}

/// Frames as an older build left them, and as this build numbers them again.
fn frames_after(change: impl FnOnce(&mut Editor)) -> (Vec<u32>, Document, LoadNotes) {
    let e = edited();
    let carried = through_older_build(&save(e.doc()), |older| older.edit(change));
    let (back, notes) = reopened(&carried);
    assert!(notes.damaged.is_empty() && notes.can_save_over());
    assert!(back.layer_floor() >= e.doc().layer_floor());
    let again = load(&save(&back)).expect("opens");
    assert_eq!(frame_ids(&again), frame_ids(&back), "kept from now on");
    (frame_ids(e.doc()), back, notes)
}

#[test]
fn drawing_on_one_frame_renews_only_its_id() {
    let (before, back, notes) = frames_after(|o| dot(o, 2, 5));
    let after = frame_ids(&back);
    assert_eq!(after[..2], before[..2]);
    assert!(
        after[2] > *before.iter().max().expect("frames"),
        "{after:?}"
    );
    assert!(notes.frame_ids_renumbered);
    assert!(notes.message().expect("told").contains("new frame ids"));
}

#[test]
fn retiming_one_frame_renews_only_its_id() {
    let (before, back, notes) = frames_after(|o| {
        o.apply(FrameCmd::Retime {
            from: 1,
            durations: vec![90],
        })
        .expect("retimes");
    });
    let after = frame_ids(&back);
    assert_eq!((after[0], after[2]), (before[0], before[2]));
    assert!(after[1] > before[0].max(before[2]) && notes.frame_ids_renumbered);
}

#[test]
fn reordered_frames_keep_their_ids() {
    let (before, back, notes) = frames_after(|o| {
        o.apply(FrameCmd::Move { from: 0, to: 2 }).expect("moves");
    });
    assert_eq!(frame_ids(&back), [before[1], before[2], before[0]]);
    assert!(!notes.frame_ids_renumbered);
}

#[test]
fn an_inserted_frame_gets_a_new_id_and_the_others_keep_theirs() {
    let (before, back, notes) = frames_after(|o| {
        o.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    });
    let after = frame_ids(&back);
    assert_eq!([after[0], after[2], after[3]], before[..]);
    assert!(after[1] > *before.iter().max().expect("frames"));
    assert!(notes.frame_ids_renumbered, "the new frame was given an id");
}

#[test]
fn removing_a_frame_keeps_the_others_ids_with_nothing_to_tell() {
    let (before, back, notes) = frames_after(|o| {
        o.apply(FrameCmd::Remove { from: 1, to: 1 })
            .expect("removes");
    });
    assert_eq!(frame_ids(&back), [before[0], before[2]]);
    assert_eq!(notes, LoadNotes::default());
}

/// Frames that look and play alike cannot be told apart: they keep their ids while there are
/// as many as before, and lose them, all of them, once one is added or removed.
#[test]
fn look_alike_frames_keep_their_ids_only_while_as_many() {
    let mut e = Editor::new(plain_three());
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("room");
    e.apply(FrameCmd::Move { from: 3, to: 0 }).expect("moves");
    let saved = save(e.doc());
    let before = frame_ids(e.doc());
    // Frames 1 and 2 are alike.
    let (back, notes) = reopened(&through_older_build(&saved, |older| {
        older.edit(|o| {
            o.apply(FrameCmd::Move { from: 0, to: 3 }).expect("moves");
        })
    }));
    assert_eq!(
        frame_ids(&back),
        [before[1], before[2], before[3], before[0]]
    );
    assert!(!notes.frame_ids_renumbered);
    let (back, notes) = reopened(&through_older_build(&saved, |older| {
        older.edit(|o| {
            o.apply(FrameCmd::Remove { from: 1, to: 1 })
                .expect("removes");
        })
    }));
    let after = frame_ids(&back);
    assert_eq!((after[0], after[2]), (before[0], before[3]));
    assert!(after[1] > e.doc().frame_floor() && notes.frame_ids_renumbered);
    let (back, _) = reopened(&through_older_build(&saved, |older| {
        older.edit(|o| {
            o.apply(FrameCmd::Add { at: 2, copy: true }).expect("room");
        })
    }));
    let after = frame_ids(&back);
    assert_eq!((after[0], after[4]), (before[0], before[3]));
    assert!(after[1..4].iter().all(|&id| id > e.doc().frame_floor()));
}

#[test]
fn a_version_2_file_carrying_the_section_has_it_read() {
    let e = edited();
    let section = section_of(&save(e.doc()));
    let mut older = load(&encode_with(e.doc(), &[], false)).expect("opens");
    older.foreign.push(ForeignSection {
        tag: *IDS,
        data: Arc::from(&section[..]),
    });
    let v2 = crate::piks::v2_writer::encode_v2(&older, &[]).expect("version 2 holds it");
    let (back, notes) = load_noted(&v2).expect("opens");
    assert_eq!(notes, LoadNotes::default());
    assert_eq!(frame_ids(&back), frame_ids(e.doc()));
    assert_eq!(back.layer_floor(), e.doc().layer_floor());
}

/// The review's case for groups: a group's check was only its kind and colour mode, so after an
/// older build ungrouped layer 3 and grouped layer 2 under the first group's id, the other group
/// silently kept it. A group's check covers what is in it now.
#[test]
fn a_group_an_older_build_gave_another_groups_id_is_renumbered() {
    let mut e = three_drawn_layers();
    let top = e.layer_id(2).expect("a layer");
    e.apply(LayerCmd::Group { layer: top }).expect("groups");
    // A frame added and removed raises the frame floor, so the file carries the section.
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    e.apply(FrameCmd::Remove { from: 1, to: 1 })
        .expect("removes");
    let ids = layer_ids(e.doc());
    let gid = ids[3];
    let saved = save(e.doc());
    assert!(contains(&saved, IDS));
    let carried = through_older_build(&saved, |older| {
        older.edit(|o| {
            let group = o.layer_id(3).expect("the group");
            o.apply(LayerCmd::Ungroup { layer: group })
                .expect("ungroups");
        });
        older.edit(|o| {
            let layer = o.layer_id(1).expect("a layer");
            o.apply(LayerCmd::Group { layer }).expect("groups");
        });
        let mut numbered = layer_ids(&older.doc);
        numbered[2] = gid;
        older.doc.keep_persisted_ids(&numbered).expect("distinct");
    });
    let (back, notes) = reopened(&carried);
    assert!(notes.layer_ids_renumbered && notes.can_save_over());
    let ids = layer_ids(&back);
    assert_eq!(
        [ids[0], ids[1], ids[3]],
        [1, 2, 3],
        "the others keep theirs"
    );
    assert!(ids[2] > gid, "{ids:?}: the group kept the other group's id");
    assert!(notes.message().expect("told").contains("new layer ids"));
}

/// A group nobody touched keeps its id through an older build that only opened and saved.
#[test]
fn an_untouched_group_keeps_its_id_through_an_older_build() {
    let mut e = three_drawn_layers();
    let top = e.layer_id(2).expect("a layer");
    e.apply(LayerCmd::Group { layer: top }).expect("groups");
    // A frame added and removed raises the frame floor, so the file carries the section.
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    e.apply(FrameCmd::Remove { from: 1, to: 1 })
        .expect("removes");
    let carried = through_older_build(&save(e.doc()), |_| {});
    let (back, notes) = reopened(&carried);
    assert_eq!(notes, LoadNotes::default());
    assert_eq!(layer_ids(&back), layer_ids(e.doc()));
}

/// A file written before a group's check covered its contents lists a group with the check
/// that was only its kind and colour mode. Its drawn layers keep their ids; its groups are
/// renewed with the usual note, never matched and never refused.
#[test]
fn a_file_of_the_earlier_group_scheme_has_its_group_ids_renewed() {
    use crate::piks_ids::{decode, Pair};
    use crate::piks_ids_hash::group_check_before_contents;
    let mut e = three_drawn_layers();
    let top = e.layer_id(2).expect("a layer");
    e.apply(LayerCmd::Group { layer: top }).expect("groups");
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    e.apply(FrameCmd::Remove { from: 1, to: 1 })
        .expect("removes");
    let saved = save(e.doc());
    let mut ids = decode(&section_of(&saved)).expect("reads").expect("v1");
    let group = ids.layers.len() - 1;
    let before = group_check_before_contents(ColorMode::Rgba);
    assert_ne!(ids.layers[group].1, before);
    ids.layers[group] = (ids.layers[group].0, before);
    let layers: Vec<Pair> = ids.layers.clone();
    let mut out = Vec::from(crate::piks_ids::VERSION.to_le_bytes());
    out.extend_from_slice(&ids.layer_floor.to_le_bytes());
    out.extend_from_slice(&ids.frame_floor.to_le_bytes());
    for list in [&layers, &ids.frames] {
        out.extend_from_slice(&(list.len() as u32).to_le_bytes());
        for (id, check) in list {
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&check.to_le_bytes());
        }
    }
    let old = encode_with(e.doc(), &[(IDS, out)], false);
    let (back, notes) = reopened(&old);
    assert!(notes.layer_ids_renumbered && notes.can_save_over());
    let now = layer_ids(&back);
    let was = layer_ids(e.doc());
    assert_eq!(now[..3], was[..3], "the drawn layers keep theirs");
    assert!(now[3] > ids.layer_floor, "{now:?}");
    let again = load_noted(&save(&back)).expect("opens");
    assert_eq!(again.1, LoadNotes::default(), "kept from now on");
    assert_eq!(layer_ids(&again.0), now);
}
