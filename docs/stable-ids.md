# Stable ids

Phase 12, after 12.2. The owner's rule is that a reference must not silently point at a different object after edits. If stable frame ids need a file format change, the migration is designed first and the owner is asked before it is built.

This note covers how ids are kept today, what was changed, and the guarantee that now holds. It ends with persisted ids: frame ids and the id floors saved in an optional `Nids` section, approved by the owner on 2026-11-17 and built (see "Persisted ids" at the end).

The changes up to "Persisted ids" do not change the file format. Those in "Persisted ids" add one optional section that older builds carry along, written only for documents whose ids opening a plain file would not work out. What a save holds can differ in two ways, both in values the format already had. Layer ids: after a layer is removed, or an added one is undone, the next new layer gets a higher id than before, also when only the interface is used (add a layer, undo, add again: it was id N twice, it is now N, then N + 1). Colors: see the swatch section below. The interface replay record (`perf/replay-app.txt`) does not hash persisted layer ids, so it does not show the first.

## What was there

| Object | Id | Where it is allocated | Saved | Undo and redo | Could it name another object? |
|---|---|---|---|---|---|
| Layer, session | `Layer::id`, a `u32` from a process-wide counter | `Layer::new`, `Layer::duplicate` | No | Kept: the step log brings back the layer object itself | No |
| Layer, persisted | `PersistedLayerId`, 1 to 2^24 | `Document::unused_persisted_id`: one more than the highest *live* id | Yes, in the v3 layer record | The snapshot `Settings` swaps it back | **Yes.** Removing the top layer and adding one gave the same id, in one run, across runs, and after undoing an add |
| Frame | None. A place, from 0 | n/a | n/a | n/a | **Yes.** Every insert, removal or move shifts places |
| Cel | Layer and frame | n/a | n/a | n/a | As those do |
| Tag | `TagId`, a process-wide `u64` | `Tag::new` | No | The snapshot keeps tags whole | No. The API names tags by none of these: it only adds them |
| Swatch | Color id, the index in `ColorTable` | `ColorTable::id_for`: the table's length | Yes, in the Colors part | The snapshot swaps the whole table back | **After undo, yes.** Fixed: see below |
| Tileset | `TilesetId`, from the table's `next_id` | `Tilesets` | Yes, `next_id` is in `TSET` | The snapshot swaps the whole table back, counter included | After undo, in principle. Not reachable from the API |
| Tile | Tile id, from the tileset's `next_id` | `Tileset` | Yes, in `TSET` | As tilesets | As tilesets. Not reachable from the API |
| Slice | Its place in `Document::slices` | n/a | Yes, in order | Kept whole | Not reachable from the API |

How the parts work:

- **History.** A structural step (`history/snapshot.rs`) saves the document's settings, frames, tags, colors, slices and tilesets from before the change. Cels and whole layers come back through the step log. Undo and redo swap these fields. They do not replay commands, so redo gives back the very ids the first run gave.
- **Older files.** Versions 1 and 2, and `.ase` import, build the document with `Document::from_parts`. That numbers layers 1, 2, 3 and so on from the bottom. A v3 file calls `keep_persisted_ids` with the ids it stored, refusing duplicates, 0, and anything above 2^24.
- **Transactions.** `Editor::transaction` runs on a copy of the document. On success the document takes on the copy, through `take_on_document`. On failure the copy is dropped.

## What changed

### Layers: a floor that never goes down (no format change)

`Document` has a new field, `layer_floor` (`ids::IdFloor`). It holds the highest persisted id given or opened with while the document has been open.

- `new_persisted_id` replaces `unused_persisted_id`. It gives one more than the higher of the floor and the live ids, then raises the floor. Every place that makes a layer uses it: blank, duplicate, group and reference layers. A merge keeps the lower layer's id.
- The floor is left out of history snapshots, so undo and redo never lower it. Undo of a removal brings the layer back with its own id, as before. Redo of an add brings back the same id.
- `transaction` takes on the scratch copy's floor *before* it looks at the result. Ids given in a failed run are used up as well. A script that kept one, for example in a variable that outlives the run, can never find it naming another layer.
- Opening a file (`from_parts`, `keep_persisted_ids`) starts the floor at the highest id in the file.
- `IdFloor` is always equal to another `IdFloor`. The floor is not part of what the document is, so document equality, history's "nothing changed" check and every existing test that compares documents are unaffected. The same rule already applies to `Layer`'s session id.
- Once 2^24 ids are used up in one session, the smallest free id is given, as before, and from then on the guarantee below no longer holds for that session: a removed layer's id can be given again. Failed runs use ids up too, so a hostile script that adds and removes layers until it is stopped can use up about 95,000 ids a run, and could reach the cap in about 180 runs of the same session. Since the floor is saved, saving and reopening no longer resets it: a document at the cap stays there, and gives the smallest free id from then on. Refusing new layers at the cap instead was rejected: a file that already holds id 2^24 (allowed by the format, pinned by `a_new_layer_takes_a_free_number_when_none_is_left_above`) could then never take another layer.

**The guarantee.** While a document is open, a persisted layer id names one layer at most, ever. That holds across runs, undo and redo, failed runs, and commands from the interface. A reference to a removed layer is refused with `NoSuchLayer`. It is accepted again only if undo brings that same layer back.

**Across save and reopen.** Every id a live layer holds is saved and kept exactly. Since the `Nids` section (below), the floor is saved too whenever it is above the highest live id, and reopening takes the higher of the saved floor and the live ids. So an id that only a removed layer had, or that an undone add or a failed run used up, is not given again after reopening either. A script file that names layer 7 written by hand keeps being refused with `NoSuchLayer` once layer 7 is removed, saved and reopened (`a_removed_layer_s_id_is_never_given_again_after_reopening` in `contract_ids.rs`). Before this, the floor started again at the highest live id.

### Frames: keys that follow their frames (no format change)

`FrameInfo` (moved to `ids.rs`, still exported from `document`) has a private `key: FrameKey`. It is a `u64` from a process-wide counter, like `Layer::id`.

- `FrameInfo::new(ms)` gives a fresh key. Struct literals outside the core no longer compile, and the format readers and tests now use `new`.
- Keys move with their frames through `move_frame`, `move_frames`, removal and cel drags, since they live in the frame list.
- New frames get fresh keys: `add_frame` (with or without copy), `duplicate_frames`, and the inserted frames of a cel drag.
- Retiming changes only the duration. It used to replace the whole `FrameInfo`.
- `from_parts` gives every frame a fresh key, so a reader passing `vec![info; n]` still gets distinct keys. Each open of a file gives new keys.
- `FrameInfo`'s `==` compares timing only, as `Layer`'s does. A reopened document is still equal to the one saved.
- **History.** `Frames::finish`, which trims a step to the frames it changed, compares keys as well (`FrameInfo::same`). Without that, a step that moved a frame past one that looks the same lost a key on undo. For example, with frames A, B, C where A and B look alike, a move gives B C A, and undo gave B B C. `Frames::matches`, history's "nothing to see" test, still compares timing only for commands. A change with nothing to see is put back whole, keys and all, so the frames stay where they were. That keeps the existing rule pinned by `history/audit_tests.rs`. A script run is the exception: its step is kept when it renewed any frame's key, even if the frames play as before (copy frame 1, then remove frame 0). Putting such a run back would leave the handles it gave out naming nothing while the run reported success, and undo would not see the run at all (`Snapshot::matches_keyed`, pinned by `editor/transaction_tests.rs`).

**The guarantee.** A frame key names one frame at most, for as long as the program runs, in any document. Inserts and moves keep it. Removal makes it name nothing. Undo of the removal brings the same frame back, with its cels, so the key names it again. This is by design: it is the same object, not a new one. A failed run's frames are dropped, and their keys are never given again. After reopening, every frame has a new key, so an old key is refused. It is never taken for another frame.

### Swatches: retired color ids (no format change)

A swatch id is an index into the color table. `AddSwatch` with a new color, or painting with one, grows the table, and undo swaps the old, shorter table back. The next new color used to take the same index, so a swatch id kept across the undo, such as one a long-lived CLI session held between runs, could name a different color.

`ColorTable` has a new field, `floor` (`ids::SharedFloor`): the highest color id any script run has seen. Unlike the layer floor it lives in the table, because painting grows the table in many places. Every copy of a table (`Arc::make_mut`, `clone`) shares one floor, so the older copy undo puts back has the same floor as the newer one it replaces.

- **Raised** for every run, kept or failed, only as far as the ids the script could have been given or read:
  - when the run starts (`Editor::transaction`), to the highest id the table holds then, since the run can read any swatch in it;
  - when the run ends (`pikslar_api::run_with`), to the highest swatch id the run was handed (`AddSwatch`) or read (`Swatches`).

  The API names colors by value everywhere else, so the ids of colors a run only paints with are never handed to it, and neither are the retired entries below a new color. Those are not counted. Before this was narrowed, the floor rose to the highest id in the run's table at the end: a failed run that painted with 60,000 new colors left the floor there for the session, and the next color added in the interface padded the saved table with 60,000 retired entries (`colors_a_run_only_painted_with_are_not_retired` in `contract_ids.rs`).
- **Used** by `ColorTable::id_for`: a new color takes the first id above the floor. The ids it skips are filled with transparent "retired" entries. They are not in the lookup, are never swatches, and no pixel uses them.
- **Only runs raise it.** Undo, redo, previews and cancelled strokes leave it alone. A document edited only in the interface numbers its colors exactly as before, so its saves, and the recorded interface replay (`perf/replay-app.txt`), are unchanged. A first version raised the floor on every undo as well; that changed the replay record, since undoing a new color and painting another then pads the table, and nothing a script holds needs it.
- **Not shared** with a table that is not a copy: `from_parts` (opening a file), `ColorTable::new`, and the tables a palette load or remap makes, which number colors afresh on purpose. Nothing is taken on from those.
- **Not compared and not saved.** `SharedFloor` is always equal to another, so table and document equality and history's "nothing changed" check are unaffected. After reopening, the floor starts at 0.
- **At the limit.** If the first id above the floor would be past `MAX_COLORS`, the first free id is given, as before.

**The guarantee.** While a document is open, a color id that a script run has seen names one color at most. A reference to a swatch taken back by undo or a failed run is refused with `NoSuchSwatch`, also once new colors are added. Removing a swatch and adding the *same* color again still gives its old id, because the color never left the table: that is the same entry. An id no run has seen can still be given again after undo in the interface, which no reference can notice.

**Limitation: ids a run was handed are retired even when there are many.** A run that adds many swatches and then fails, or whose step is undone, was given every one of those ids, so the guarantee needs them all retired: at default fuel one run can add about 14,000 swatches (each `add_swatch` pays for the colors it looks through, so the count grows with the square root of the fuel). The next color added in the interface then pads the table with that many retired entries, which are saved, and repeated runs can bring `TableFull` sooner than the live colors suggest. Capping the raise at the live table length would undo exactly the ids the failed run handed out, which is what the floor exists to prevent, so the cap was not made. The padding is bounded by `MAX_COLORS` and by fuel, each run needs a user to start it, and the floor is session-only: saving and reopening starts it at 0.

**What is saved.** A document whose new colors are never taken back after a run saves byte for byte as before. One where a run's color is taken back and another color is then added saves the retired entries too, as transparent colors in the Colors part. Older versions read them as ordinary unused colors.

## API changes (v1, unreleased)

| Before | Now |
|---|---|
| `FrameRef` is a place from 0 | `FrameRef` is a frame handle: the frame's key |
| `DocumentInfo { width, height, durations, layers }` | Also `frames: Vec<u64>`, the handles in order |
| `AddFrame` gives `Reply::Done` | It gives `Reply::Frame(handle)` |
| `AddFrame { at: FrameRef, .. }` | `at: i64`: still a place to insert at, not a reference |
| `AddTag { from, to }` places | Handles. If `to` comes before `from` in the timeline, it is refused with `OutOfRange { what: "a tag's last frame's place", .. }` |
| `NoSuchFrame(i64)`: "there is no frame N" | Same variant. The message says the handle was removed or is from before the project was opened |
| `Reply::Created(u32)` | Unchanged, for layers and swatches |

`API_VERSION` stays 1, because version 1 has not been released. A script written against the old v1 that passes small places is refused loudly with `NoSuchFrame`. Handles start at 1 and run across the whole program, and a small number names a frame only by chance. A place used as a handle is either refused or names a frame the script never saw.

**For the interpreter (`pikslar-script`).** A script gets handles from `Document().frames`, or from what `AddFrame` returns. Loops over frames should iterate over that list, not over `0..count`.

**For JSON lines (`pikslar-cli`).** A fixed list of operations cannot name a frame that an earlier operation in the same list made, nor know an existing frame's handle in advance. A layer or frame field may hold `{"result": n}` (what operation `n` made) or `{"result": n, "place": k}` (place `k` of the `document` operation `n` gave back), checked before anything runs. See `docs/cli-json-lines.md`. Frame fields are otherwise JSON integers, and handles fit in an `i64`.

## Still open

- **Tileset and tile ids after undo.** The `next_id` counters live in the tileset table, which undo swaps back, so an undone tileset's or tile's id can be given again. They are not reachable from the API. The swatch fix does not carry over as simply: both counters are saved in `TSET`, so a floor would change what is saved for any document that undoes a new tileset or tile, and tile ids are what tile-image undo steps (`Edit::TileImages`) are keyed by, so a change there needs its own audit of tile history. Left until scripts can reach tilesets (12.4).
- **Hand-made duplicates.** `Document::frames` is a public field. Code that pushes a copy of a `FrameInfo` (only `pikslar-export`'s tests do) makes two frames share a key. The API refuses a handle that more than one frame holds, so it fails loudly. Commands never do this. Saving such a document writes usable ids: the first frame with an id keeps it, the rest get new ones (`hand_made_duplicate_frames_save_ids_that_open_as_saved`), so the file opens to what it holds and the app's save check passes. The open document keeps its hand-made ids until it is opened again.

## Evidence

- `crates/pikslar-core/src/ids_tests.rs` (14 tests): no reuse after removal, after an undone add, or across undo and redo; redo keeps the id; failed and successful transactions use ids up; document equality ignores the floor; every frame has its own key across add, copy and duplicate; keys follow inserts, moves, run moves and retiming; undo restores a removed frame's key; undo restores the order of look-alike frames; a move with nothing to see leaves keys in place; `from_parts` gives copies their own keys; frames compare by timing.
- `crates/pikslar-api/tests/contract_ids.rs` (12 tests):
  - layer id reuse within a run, across runs, undo and redo, after a failed run, and through the interface's commands;
  - stale layer references are refused;
  - a frame handle survives inserts and an interface move;
  - a frame handle is refused after removal, even once frames are added;
  - after undo, a handle names the same frame and its drawing;
  - a failed run's frame handle is never given again;
  - save and reopen keep every layer id and refuse old frame handles;
  - saved bytes are unchanged by ids used up but not kept;
  - v1 and v2 corpus files open with layers numbered from 1 and distinct frame handles.
- With both the floor and key-aware trimming turned off, 7 of the 14 core tests fail: 5 for layers and the 2 look-alike frame tests. With the floor turned off, 5 of the 12 API tests fail.
- Swatches: `crates/pikslar-core/src/color_floor_tests.rs` (8 tests): the interface on its own numbers colors as before; a color a run saw stays retired after undo; a run retires what the table held when it started, and its caller what it handed out; undo and redo never lower the floor; copies share it and new tables start without one; equality ignores it; a remapped table takes nothing on; at the limit the first free id is given. `contract_ids.rs` (7 more tests, 19 in all): an undone `AddSwatch`'s id is never given again, nor a failed run's; colors a run only painted with are not retired; a swatch id a run read stays retired; a stale swatch reference is refused by `Recolor` and `RemoveSwatch`; save and reopen keep every live swatch id; documents that never undo a new color save the same bytes as before. Before the fix, the API's three reuse tests failed; with the raise at the end of a run turned off, those three and 3 of the core tests fail.
- Persisted ids:
  - `crates/pikslar-core/src/frame_ids_tests.rs` (16 tests): a new document's frame is 1; every way of making a frame gives a new id; ids follow moves; a removed frame's id is never given again, nor a failed run's; undo and redo keep ids and never lower the floor; `from_parts` numbers 1 to n; `keep_frame_ids` and `keep_some_frame_ids` refuse bad lists and give the frames left out ids above the floor; renumbering frames and a layer goes above the floor; at the cap the smallest free id is given; renumbering 4,096 frames and adding frames at the cap stay quick (the old way took minutes); equality ignores ids.
  - `crates/pikslar-format/src/piks_ids_tests.rs` and `piks_ids_older_tests.rs` (35 tests): the round trip, byte for byte, also for frames that look alike; no section for documents that never used the ids; recovery copies; matching by print; every malformed field, every truncation and a second section are damage that costs only the ids, with the frames renumbered above the floors when those can be read; floors past the caps; a newer version or trailing fields are noted; random byte flips never panic; hand-made duplicates save and reopen byte for byte; a file asking for the largest floor opens quickly; the app's sentence. Acting out an older build with `studio-opt1`'s own layer numbering rule, copied into the test: opening and saving, or changing layer settings, keeps every id; a listed layer's id given again in one session (opt1) or two (m12) is renumbered; a listed layer drawn on is renumbered; an unlisted layer at or below the floor is renumbered and one above it kept; drawing on or retiming one frame renews only its id; reordering keeps every id; an inserted frame gets a new one; removing a frame keeps the others' with nothing to tell; look-alike frames keep theirs only while as many.
  - `crates/pikslar-safety/tests/ids.rs` (4 tests): every frozen file opens numbered from 1 with no section; every kind of file in the corpus keeps its frame and layer ids through saving, opening and an older build, so the prints and layer checks hold on holds, links, offsets, references, groups, tilemaps and Aseprite blending; a damaged section on a frozen file costs only the ids. The random undo model checks after every step that ids come back with their states, are distinct, sit under the floors, are never given twice, and survive saving and reopening, after which a new frame and layer go above the floors; a numbered seed joins the malformed and mutation corpus.
  - `crates/pikslar/src/ids_open_tests.rs` (1 test): the app tells renewed frame ids when it opens such a file, and Save still writes over it.
  - With the listed-layer check turned off, 3 format tests fail; with one stale print dropping every frame id (the old rule), 6; without the layer floor raise on open, 15; with new frame ids ignoring the floor, the undo model fails ("frame id 2 was given again, under floor 2").

## Persisted ids (built, 2026-11-17)

The owner approved this on 2026-11-17: "Existing projects must continue opening correctly. Unknown compatible file sections must survive saving, and older builds must not silently destroy important identity information."

### In the document

- **Frame ids.** `FrameInfo` has a persisted `id` (`FrameInfo::id`), 1 to `MAX_FRAME_ID` (2^31), distinct within one document. It sits beside the session key, which is unchanged: keys are process-wide and never repeat, ids are per document and are saved.
- **Frame floor.** `Document::frame_floor`, the highest frame id given while the document has been open, works as the layer floor does: a new frame (`add_frame`, `duplicate_frames`, the inserted frames of a cel drag) takes one more than the floor, then raises it. The floor is at least every live id, so this takes constant time. Undo and redo bring frames back whole, ids and all, and leave the floor alone. `transaction` takes on the scratch copy's frame floor with its layer floor, so a failed run's frame ids are used up too. Past 2^31 the smallest free id is given, found in one pass over the frames, as for layers.
- **Numbering on open.** `from_parts` numbers frames 1 to n with the floor at n, as `Document::new` numbers its one frame 1. `FrameInfo::new` outside a document has id 0, which a document never keeps.
- **Not compared.** Frame ids and floors are not part of what a document is: `FrameInfo`'s `==` still compares timing only.

### In the file: the `Nids` section

Layout, little-endian (version 1; it replaced an unreleased layout, written by no tagged build, that had one fingerprint for all the frames and no layer checks):

| Field | Type | Notes |
|---|---|---|
| version | `u32` | 1 |
| layer floor | `u32` | at most 2^24, and at most 2^23 unless a listed layer id is as high |
| frame floor | `u32` | at most 2^31, and at most 2^30 unless a listed frame id is as high |
| layer count | `u32` | the layers in use when it was written, at most 256 |
| layers | (`u32` id, `u64` check) × count | from the bottom, ids distinct, 1 to the layer floor |
| frame count | `u32` | 0 for no list, otherwise the document's frame count, at most 4,096 |
| frames | (`u32` id, `u64` print) × count | timeline order, ids distinct, 1 to the frame floor |

Every later version keeps the version and both floors in the first 12 bytes, so a build that cannot read the rest can still keep the floors.

**When it is written.** Only when it says something opening the file would not work out: the layer floor is above the highest live layer id, the frame floor is above the frame count, or the frames are not numbered 1 to n in order. Any other document saves byte for byte as before: a new drawing, frames and layers only ever added, every frozen corpus file opened and saved (`frozen_files_open_numbered_from_1_with_no_section` in `pikslar-safety/tests/ids.rs`). Removing a frame or a layer, moving a frame, or undoing an add does write it, since that is exactly the information a reopen would otherwise lose. Writing it always was rejected: it would change the bytes of every save, and the corpus and pinned-bytes tests check that saves are stable. One pinned file changed for this reason, `a_hold_released_in_the_middle_keeps_its_bytes` in `v3_bytes.rs`, which removes a frame; it now ends with a 100-byte `Nids` (60 bytes in the replaced layout).

A document whose frame ids are not usable (two frames sharing an id, or a frame with none, which only hand-made `doc.frames.push` does) is saved as if the first frame with an id kept it and the others had new ones above the floor.

**Prints and checks.** A frame's print is its duration and, summed so layer order does not matter, a hash of each cel that is not blank: its position and the tiles of its image that draw something, with their place and stored form. A layer's check is its kind, colour mode and tileset, and the sorted distinct hashes of the cels it shows; a group has no cels, so its check is instead its kind, colour mode, a scheme mark, how many layers it holds, and the id and check of each of them from the bottom. A group's check therefore changes when what it holds changes (a layer added, removed, swapped, drawn on or renumbered). Files written before this scheme listed a group's check as only its kind and colour mode; such a check never matches, so on open every group of such a file gets a new id above the floor with the usual note (drawn layers keep theirs). Tiles that draw nothing and the reach of an image's table past its drawn tiles are left out of both, because opening a file drops and trims them; every other input is stored exactly and read back as stored by every studio build, so opening and saving changes neither. A frame's print changes when it is drawn on, retimed, or has a cel moved; it does not change when an empty layer is added or removed, layers are reordered, or a layer's visibility, opacity, blend, lock or name changes. A layer's check changes when it is drawn on, changes kind, colour mode or tileset, or loses the last frame that shows one of its drawings; it does not change when it is renamed, hidden, moved or retimed, or when frames are added, copied, reordered or removed without taking a drawing. Neither is a security measure: a crafted file can choose any ids it likes, as it can choose any pixels.

**On open** (version 3, and version 2 files that carry the section; both read it last, after tags, rate, groups, slices and terrain, so the prints and checks are of the frames and layers as they now stand):

- The layer floor becomes the higher of the saved floor and the highest live id, and so does the frame floor.
- A listed layer whose check no longer matches may be another layer that took a removed layer's id, so it gets a new id above the floor. So does a layer not in the list whose id is not above the saved layer floor: it was numbered by a build that did not keep the floor. `LoadNotes::layer_ids_renumbered` is set. Listed layers whose checks match, and layers above the floor, keep their ids.
- A frame keeps a listed id if its print is in the list exactly as many times as among the frames; alike frames take their ids in order. Any other frame (drawn on, retimed, added, or one of a set of alike frames that grew or shrank) gets a new id above the saved floor, and `LoadNotes::frame_ids_renumbered` is set. Reordered frames keep their ids, and removing a frame costs only its own.
- Both notes are told: the app's message after opening or restoring says "Some frames an older Pikslar changed or added were given new frame ids" or the layer sentence, and the command line says so too. Neither stops a save over the file, since nothing that saving would keep is lost, so the message does not end with "Save will ask for a new name" unless something else needs it.
- A section that does not parse is damage ("frame and layer ids"), costing only the ids: version 0, a floor past its maximum or past its cap with no listed id as high, a count over its limit or past the bytes, an id of 0, a repeated id, an id above its floor (in either list), a cut-short section, or a second `Nids`. If the first 12 bytes still give a version other than 0 and floors within the caps, the floors are kept and every frame gets a new id above them, so no id given before names a frame; otherwise the frames are numbered 1 to n with the floors at the live ids. Save asks for a new name, as for any damaged section. The file always opens.
- A version above 1 is a layout this build does not know: its floors are read as for damage, its lists are not, it is noted as newer fields, and Save asks for a new name. Bytes after a version 1 section are skipped and noted the same way; the fields this build reads are used.
- Version 1 and 2 files without the section, `.ase` import, and version 3 files without the section open exactly as before: frames 1 to n, floors at the live ids.

### What older builds do

Checked against the shipped code, read-only:

- **RC2 and `main`** (format version 2 only) refuse every version 3 file as "a newer project version". They never open, edit or save a file with `Nids`.
- **The studio builds before this one** (tags `studio-m4` to `studio-m12`, `studio-opt1`, and `dev/studio` before this change) read version 3 and, by `piks_sections::keep_foreign`, keep any section with a lowercase last letter that they do not know, room permitting (64 sections in all, 16 MiB of carried data). They write it back unchanged, after the sections they know, and open the file with no note, so Save writes over it. Their own editing never touches the section. `studio-m4` to `studio-opt1` number a new layer one past the highest live id with no floor at all (`unused_persisted_id`), so removing the top layer and adding one gives the removed layer's id again in one session; `studio-m12` keeps a floor while the file is open but saves none, so it does the same across two sessions.

So, for a file this build wrote and an older studio build then opened, changed and saved:

- **Floors: always kept.** The section is carried byte for byte and floors only rise, so this build takes the higher of the saved and live values. No layer id and no frame id given before can be given again by this build, whatever the older build did.
- **Layer ids: kept, except where the layer may not be the one the id was given for.** Layer ids are in the layer records, which older builds keep, so a layer keeps its id. A layer that took a removed layer's id is either not in the saved list and not above the floor, or in the list with a check that no longer matches (the removed layer showed other drawings, or none, or was of another kind or mode). Either way it gets a new id above the floor with a note, and a reference to the removed layer is refused rather than taken for it (`a_listed_layer_s_id_an_older_build_gave_again_in_one_session_is_renumbered`, `..._in_a_second_session_...`, `an_unlisted_layer_at_or_below_the_floor_is_renumbered`). The check is conservative: a listed layer the older build drew on also gets a new id, costing that id, never giving it to another layer (`a_listed_layer_an_older_build_drew_on_gets_a_new_id`). Layers it numbered above the floor keep their ids (`layers_an_older_build_added_above_the_floor_keep_their_ids`).
- **Frame ids: kept for frames that look and play as they did.** Opening and saving, adding an empty layer, changing layer settings, reordering or removing frames keeps every remaining frame id. A frame drawn on or retimed, and a frame added, gets a new id above the floor, with a note; the others keep theirs. A frame id is kept only for a frame with the same print: the same duration and the same drawn cels at the same places. Edits that change how a frame looks without changing its print keep the ids, which is right, since the frames are still the same frames: tile pixels of a tileset (map cells unchanged), palette colours in indexed mode, layer visibility, opacity or blend, moving a cel to another layer in the same frame, and frame-rate changes that leave the durations whole.
- **Not guaranteed.** Frames that look and play exactly alike cannot be told apart: if an older build swaps them, or removes one and adds an alike one, they keep or trade ids, since nothing in the file tells them apart. A blank layer that took the id of a removed blank layer of the same kind, colour mode and tileset keeps it, for the same reason. A group an older build emptied, filled or regrouped has a different check, so it is renumbered like any changed layer, not matched (`a_group_an_older_build_gave_another_groups_id_is_renumbered`); a group is kept only while it holds the same layers, by id and check. Ids from a file an older build changed are dropped, not repaired: references kept outside Pikslar must then be found again. A damaged section whose floors cannot be read leaves the frames numbered 1 to n, which may be ids removed frames had. At the caps (2^31 frame ids, 2^24 layer ids) the smallest free id is given again; a file cannot ask for a floor above half of that without an id as high, and a document would need 2^30 frame or 2^23 layer creations in one session to get there, after which its save would be refused as unverified rather than written. An older build with no room left for carried sections drops `Nids` and says the file has data from a newer Pikslar, so it saves under a new name; that copy opens numbered from 1 with its floors at the live ids. Adding `Nids` to this build's known sections leaves room for one carried section fewer (55 rather than 56): a file carrying exactly 56 lowercase unknown sections, which only a hand-made or future file could, opens with the 56th not kept and is protected from Save.

### Round trips

- New, save, open in the new build: every frame id, both floors and every layer id are kept, and saving again gives the same bytes (`ids_and_floors_survive_save_and_load` in `piks_ids_tests.rs`, and every corpus file in `every_kind_of_file_keeps_its_ids_through_saving_and_opening`). Recovery copies keep them too.
- New, older studio build, new: as above (`piks_ids_older_tests.rs`, acting out the older build with its own carry rule and its own layer numbering rule, copied from `studio-opt1`).
- `.ase` export carries no ids; re-import numbers from 1.

### API and scripts

Frame handles stay session keys, as before. Persisted frame ids are not exposed in API version 1 yet: `DocumentInfo` and the replies are unchanged, so the CLI's JSON and the interpreter's bindings did not change. Frame handles therefore still do not survive a reopen: an old handle is refused with `NoSuchFrame`, never taken for another frame (`save_and_reopen_keep_every_layer_id_and_refuse_old_frame_handles`). The file now holds what a later API needs to name frames across reopening (a reply field, or a lookup by persisted id) with no further format change. Layer references, which are persisted layer ids, now survive a reopen and are never given to another layer.

The byte-identity contracts for undo and failed runs (`contract_transaction.rs`, `contract_ids.rs`) now compare everything but the `Nids` section (`piks::save_without_ids`): a run that added a layer and was undone or failed has used up its id, and that is saved, as the guarantee needs.
