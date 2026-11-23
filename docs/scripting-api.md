# Scripting API, version 1

Phase 12.2. The crate `pikslar-api` is the surface that scripts reach. It is plain Rust, with no interpreter in it, so the engine chosen in 12.1 and the JSON lines of 12.3 both call the same thing. Since 12.4 b the program runs scripts through it (Script binding, below). The file format does not change.

## Shape

- `API_VERSION` is 1. A run names the version it was written for. Any other version is refused with `ApiError::Version` before anything runs.
- `run(editor, version, body)` runs one script. `body` gets a `Script`, and `Script::call(Op) -> Result<Reply, ApiError>` carries out one operation. `run_ops` runs a list of `Op`s. `run_cancellable` also takes an `AtomicBool`, checked before every operation. `run_with` takes the flag as an option and lets the caller choose the body's error type, as the binding needs.
- `Op` and `Reply` are plain data. Numbers arrive as `i64`, as an engine or a JSON reader holds them. Each one is checked at this boundary, and anything out of range is refused, not clamped.

## One transaction per run

A run is `Editor::transaction`, added to `pikslar-core`. The body works on an editor of its own, which holds a copy of the document that shares every drawing. Every operation becomes ordinary commands through `Editor::apply`, so the API has no editing path of its own.

- **Success.** The document takes on the copy as one structural undo step, through the same snapshot and log machinery as any other command. Undo and redo restore it exactly, and history memory is counted as usual. A run that changes nothing records nothing.
- **Error or cancel.** The copy is dropped. The document, undo, redo, the revision and the saved bytes are exactly as they were.
- **Refused while busy.** A run is refused with `LimitError::Busy` while a stroke of pixels or of map cells (tiles or terrain) is in progress, or pixels are floating. The panel must anchor them first. A value being dragged (a joined step) does not refuse a run: its step is closed first, as before any command.
- **Frames a run renewed are kept.** A run whose frames play as before but carry new keys (copy a frame, remove the original) is still a step, so the handles it gave out go on naming their frames, and undo takes the run back.
- **What the body starts with.** The body sees no selection, the active layer, frame and color, and a fresh editor's brush and options. Pixel operations always use a one-pixel round brush, with pixel-perfect and rhythm off, so a run does not depend on what the interface has chosen.
- **Visibility.** Visibility, locking and folding are never undone, as with the commands. They are applied outside the step, and they still count as a change to the file.
- **Canvas size.** A run that changes the canvas size clears the selection, and undo brings the selection back, as for `CanvasCmd::Resize`.

## Ids

| Object | Named by | While the project is open | Across save and reopen |
|---|---|---|---|
| Layer | Its persisted id (format v3 layer record) | Never given to another layer, even after removal, undo or a failed run | Kept |
| Swatch | Its color id (Colors part) | An id a run has seen is never given to another color, even after undo or a failed run (`docs/stable-ids.md`) | Kept |
| Frame | Its handle: `Document` lists them, `AddFrame` gives the new one | Follows its frame through inserts and moves; refused once removed, valid again if undo brings the frame back | Not kept: frames get new handles, and old ones are refused |
| Cel | Layer id and frame handle | As those do | As those do |

Session `LayerId`s never leave the API. Ids are resolved again for every operation, so moving or removing other layers or frames never makes a reference point somewhere else: it names its own object or is refused. Frames are named by handle, not by place, so a script loops over `Document().frames`. `AddFrame { at }` and `MoveLayer { to }` take places, because they say where something goes, not which object is meant.

**Changed in v1 before release** (`docs/stable-ids.md`): frames were named by place and are now named by handle; `DocumentInfo` gained `frames`; `AddFrame` gives `Reply::Frame(handle)`; `AddTag` takes handles; layer and swatch ids are never given twice in a session. `API_VERSION` stays 1.

## Operations

Reading: `Version`, `Document` (size, durations, layer ids from the bottom), `Layer` (id, place, name, kind, mode, visible, locked, opacity, blend), `Pixel` (shown color of one cel, `0xAABBGGRR`), `Swatches`.

Editing, each mapped to the commands named:

| Op | Commands |
|---|---|
| `AddLayer {above, mode, name}` | `LayerCmd::Add`, then `Rename`. Gives back the new id |
| `RemoveLayer`, `RenameLayer`, `MoveLayer`, `DuplicateLayer` | `LayerCmd::Remove`, `Rename`, `Move`, `Duplicate` (gives back the copy's id) |
| `SetOpacity`, `SetVisible`, `SetBlend` | `LayerCmd::SetOpacity` and `EndDrag`, `SetVisible`, `SetBlend` |
| `AddFrame`, `RemoveFrame`, `SetDuration` | `FrameCmd::Add` (gives back the handle), `Remove`, `Retime` and `EndDrag` |
| `AddTag {name, from, to}` | `TagCmd::Add` and `EndDrag` |
| `SetPixel`, `ErasePixel`, `DrawLine`, `Fill` | `DrawCmd::Stroke` with Pencil, Eraser or Line, and `DrawCmd::Begin` with Fill, on the chosen cel |
| `AddSwatch`, `Recolor`, `RemoveSwatch` | `PaletteCmd::AddSwatch` (gives back the id), `Recolor` and `EndDrag`, `RemoveSwatch` |

## Checks

These are refused with an `ApiError` that names the problem:

- Unknown layer, frame or swatch ids.
- Places outside the list.
- Pixels outside the canvas.
- Opacity outside 0 to 255.
- Durations outside 10 to 60,000 ms.
- Colors outside `u32`, or with alpha 0 when the color is not 0.
- Names that are blank, over 64 characters (32 for tags), or that hold control characters.
- Blend modes the document's blending does not offer, matched by the exact name the panel shows.
- Tags whose last frame comes before their first.
- `Pixels` blocks larger than `MOST_READ_PIXELS` (2^24 pixels, 64 MiB of colors), as `OutOfRange { what: "a block's area" }`, before anything is read. Scripts are held to their memory limit before that.
- Layer ids that two layers hold, as `NoSuchLayer`, as for frames. No loader or command makes them.

Script text in an error is cut short: a name or blend in `BadName` or `UnknownBlend` is at most its first 64 characters and an ellipsis (`input::quoted`).

Anything the command itself refuses is returned as `ApiError::Refused(LimitError)`, with the editor's own words.

## Evidence

- `crates/pikslar-core/src/editor/transaction_tests.rs` (14 tests):
  - one step, with undo and redo exact;
  - the same state as the commands run one by one;
  - failure keeps undo, redo and the revision;
  - nothing changed means no step;
  - layers moved and removed;
  - holds through repeated undo and redo;
  - busy refusals (a map stroke too, in `tilemap/stroke_tests.rs`);
  - a run that only renews frame keys kept as a step, while a command with nothing to see still records nothing;
  - canvas resize and the selection;
  - the active layer kept.
- `crates/pikslar-api/tests/` (34 tests, 12 of them on ids in `contract_ids.rs`):
  - the version reported, and others refused;
  - each operation's behaviour;
  - a run as one undo step;
  - rollback on a bad operation, an error from the script, a refused command or a cancel, each leaving the saved file byte-identical but for the `Nids` record of ids used up;
  - ids across save and reopen;
  - 33 malformed inputs refused, each after a good edit, with no change;
  - an API-driven edit saved byte for byte equal to the same commands given directly.

- `crates/pikslar-core/src/editor/transaction_tests.rs` also checks that a long run keeps no history of its own.
- `crates/pikslar-api/tests/contract_bulk.rs` (10 tests): the bulk operations, their refusals, locked and palette layers, and byte equality with one-pixel operations, in strips too.
- `crates/pikslar-bind` (68 tests):
  - 15 unit tests: costs, blocks, values, limits, capabilities (each one alone grants exactly its own functions), when growth is measured, a file read stopping at the first piece it cannot pay for;
  - `tests/scripts.rs` (16): an ordinary script as one step, `set_pixel` saved as the API's ops, bulk functions, capabilities and no ambient names, stale references with their lines, refusals of waiting pixels, argument types, a busy editor, a huge read refused, pixel work paid for before it runs, `set_pixel` refused at the call on a layer that cannot be drawn on, waiting pixels named as such inside another call, an API cancel classed as a cancel, lookups priced by what they look through, and long text and too many colors refused before they are copied;
  - `tests/files.rs` (9): reading files through a granted picker, a file larger than the script may hold and a long list of kinds refused before they are copied, a second read of a handle costing nothing more, a cancel stopping a large read;
  - `tests/write.rs` (9): writing a file chosen in the save dialog only when the run succeeds, with the document and every file unchanged by a failed, cancelled or end-of-run failed write; one write per handle; read and write handles never swapped; for each grant, the functions it lacks do not compile and show no dialog;
  - `tests/redteam_*.rs` (10): cancels, refusals quoting a bounded part of script data, shared strings not copied per item, and a grammar-aware fuzzer (250,000 cases in release: 165,969 compiled, 43,638 succeeded, 4,509 stops, 16,980 refusals, none breaking a rule; slowest 29 ms);
  - `tests/growth.rs` (3): the edits that add the most cels at once (a duplicated group of 128 layers, a layer on 4,096 frames, a frame on 256 layers) add at most 64 bytes per fuel unit they pay;
  - `tests/transaction.rs` (6): a failure of each of 7 kinds at every one of 21 operation places, a fuel limit at 80 points, 40 random cancels, a cancel during bulk fills on 1024 x 1024 (stopped within 500 ms), exact undo and redo, and a run that grows the document past its budget stopped. Each leaves the saved bytes, revision, undo and redo unchanged.
- `crates/pikslar`: three tests of `App::run_script` (one undo step in the app, a failed file changes nothing and says why, limits from the settings), and three of the settings lines.

## Bulk pixel operations

Phase 12.4 b added three operations, so that pixel work need not be one command per pixel:

| Op | Commands |
|---|---|
| `PutPixels {layer, frame, x, y, width, colors}` | `FloatingCmd::Paste` of the colors as a clip, then `Anchor` and `SelectionCmd::None`. A color of 0 leaves its pixel as it was. `colors` must make whole rows of `width` |
| `FillRect {layer, frame, x, y, width, height, color}` | The same paste of one color. For color 0 it is `SelectionCmd::Select` of the rectangle, `DrawCmd::DeleteSelected` and `SelectionCmd::None` |
| `Pixels {layer, frame, x, y, width, height}` | Reads the shown colors, row by row: `Reply::Pixels` |

These are the interface's own paste and delete, so the same guard refuses them (a locked layer, for example).

- A block larger than 65,536 pixels is pasted in strips of whole rows, so no paste holds more than 256 KiB of colors.
- A block saves byte for byte as the same pixels set one by one, on RGBA and palette layers, semi-transparent colors included (`tests/contract_bulk.rs`).
- The command line accepts them as `put_pixels`, `fill_rect` and `pixels`.

## Text

Roadmap 15.1 added one drawing operation, so a script can label a sprite without a font of its own:

| Op | Commands |
|---|---|
| `DrawText {layer, frame, x, y, text, color, scale}` | `FloatingCmd::Paste` of the text drawn in the built-in 5x7 font (`pikslar_core::glyphs::rasterize`), then `Anchor` and `SelectionCmd::None`, as `PutPixels` does |

The host functions are `draw_text(layer, frame, x, y, text, color, scale)` (`EditDocument`) and `text_width(text, scale)` (`ReadDocument`).

- `(x, y)` is the top left of the text and must be a canvas pixel. Text that runs past the right or bottom edge is clipped, as pasted pixels are; nothing is refused for that.
- `color` is straight-alpha `0xAABBGGRR` as everywhere, and must not be 0. `scale` is a whole number from 1 to 8: every font pixel becomes a square of that many canvas pixels. Letters are hard-edged: no smoothing, so no color but `color` is made.
- `text` is at most 256 characters, line breaks included. A line break (`"\n"`) starts a new line, one row below the last, left aligned. A character the font lacks (it has ASCII, Latin-1, Latin Extended-A and common punctuation) is drawn as `?`.
- The drawing may be at most 8192 pixels wide and high, the largest canvas, and at most 2,097,152 pixels in all (8 MiB of colors, `MAX_TEXT_AREA`; its paste is charged at 14 bytes a pixel, below). The whole rectangle of the lines is made, so one long line over many short or empty ones is a large drawing for few characters; ordinary text, even 256 characters at scale 8, is well inside it. Past either limit it is refused before it is made.
- `text_width` gives the width in pixels of the widest line, exactly as `draw_text` draws it (letters 5 wide, 1 between them, times the scale), so a script can centre or right-align a label. It is pure: it reads nothing of the document, and needs `ReadDocument` only so that a run without it stays free of the API.
- A refusal (outside the canvas, a scale or length out of range, a clear color, a locked or non-paint layer) changes nothing. A drawing is one undo step with the rest of the run.
- The font is fixed at 5x7. The text tool's other font (3x5, capitals only) is not offered to scripts yet; a later `draw_text_font` can take it without changing this function, since the language has no optional arguments.
- The command line accepts it as `draw_text` with `"text"`, `"color"` and `"scale"` fields (`{"op":"draw_text","layer":1,"frame":0,"x":3,"y":4,"text":"Hi","color":4278190335,"scale":2}`), and counts the pixels of the drawing it makes against a run's work budget, as a script pays for them.
- Tests: `crates/pikslar-api/tests/contract_text.rs` (8), `crates/pikslar-bind/tests/text.rs` (9), and the command line's table of ops.

## Script binding

The crate `pikslar-bind` joins the interpreter (`docs/scripting-language.md`) to this API. The app calls it through `App::run_script` (`crates/pikslar/src/scripting.rs`). At launch it runs `pikslar --run-script <file>`, with read and edit. A launch script has no Stop button, so it is stopped after 60 seconds (`LAUNCH_SCRIPT_TIME`) and changes nothing; the status says "Script *name* was stopped after 60 s. Nothing changed". In the window, the Scripts panel runs scripts (see [The Scripts panel](#the-scripts-panel)).

`run_script(editor, source, capabilities, limits, cancel) -> Outcome` compiles and runs one script as **one** `run_with` of this API, so as one transaction:

- **Success** is one undo step, or none if nothing changed.
- **Any failure** leaves the document, undo, redo, the revision and the saved bytes as they were. A failure is any of these:
  - a compile error or a script error;
  - fuel, memory or call depth;
  - a refused operation;
  - a cancel.
- **The `Outcome`** holds:
  - the result: the value rendered, or a `Failure` (`Script`, `Refused { error, at, waiting }` or `Thread`). `Failure::kind` gives `Cancelled` for a cancel, whether the interpreter or the API saw it first, so `is_cancel` and `is_stop` hold for both;
  - what the script printed, at most 64 KiB;
  - the fuel used and the memory peak.
- **A refused operation** is reported with the line and column of the call that met it.

### Only what is registered

A script reaches nothing but the functions below. Each one maps to one or a few `Op`s, and so to the commands. The interpreter has no file, network, process, environment, clock or OS access, and no name for any of them.

The table is built for each run from a `Capabilities` set. A function the set does not grant is left out of the table, so naming it is a compile error, as for any unknown name. Plugins can later be given less. `Capabilities::NONE`, `READ` and `ALL` are defined.

| Capability | Functions |
|---|---|
| none | `print(value)` |
| `ReadDocument` | `text_width(text, scale)`, `api_version()`, `width()`, `height()`, `layers()` (ids from the bottom), `frames()` (handles), `layer_name`, `layer_kind`, `layer_index`, `layer_visible`, `layer_locked`, `layer_opacity`, `layer_blend` (each `(layer)`), `frame_duration(frame)`, `get_pixel(layer, frame, x, y)`, `read_rect(layer, frame, x, y, w, h)` (a list of colors, row by row), `swatches()`, `swatch_color(swatch)` |
| `PickFileToRead` (plugins only) | `pick_file_to_read(title, kinds)` (a handle, or nil if cancelled), `read_file(handle)` (the file's text), `file_name(handle)` (its name, without the folder) |
| `PickFileToWrite` (plugins only) | `pick_file_to_write(title, kinds)` (a handle, or nil if cancelled), `write_file(handle, text)` (once per handle; written when the run succeeds) |
| `EditDocument` | `add_layer(name)`, `add_layer_above(layer, name)`, `remove_layer`, `rename_layer`, `move_layer(layer, to)`, `duplicate_layer`, `set_opacity`, `set_visible`, `set_blend(layer, name)`, `add_frame(at)`, `copy_frame(at)`, `remove_frame`, `set_duration(frame, ms)`, `add_tag(name, from, to)`, `set_pixel(layer, frame, x, y, color)` (0 erases), `erase_pixel`, `draw_line(layer, frame, x0, y0, x1, y1, color)`, `fill(layer, frame, x, y, color)`, `fill_rect(layer, frame, x, y, w, h, color)`, `write_rect(layer, frame, x, y, w, colors)`, `draw_text(layer, frame, x, y, text, color, scale)`, `add_swatch(color)`, `recolor(swatch, color)`, `remove_swatch` |

Every id is a stable id or a frame handle (`docs/stable-ids.md`). A stale one is refused with the API's error and the call's place. It is never mapped to another object. An argument of the wrong type stops the run with a `Host` error that names the function and the argument.

### set_pixel waits in blocks

One command per pixel costs about 1.1 µs a pixel on a 256 x 256 canvas, and 10 µs on 1024 x 1024. So `set_pixel` writes into 64 x 64 blocks of its cel instead, and the blocks are written as `PutPixels` at three points:

- before any other operation, so every read and edit sees them;
- when the script moves to another cel;
- at the end of the run.

At most 256 blocks (4 MiB) wait; one more writes them all first. Colors are checked when `set_pixel` is called, and so is the layer: a locked layer (also through a locked group), one that cannot be drawn on (a group, a reference, a tilemap) or one whose drawing is shared with a locked layer is refused at that `set_pixel`, before anything waits. The saved bytes are the same as for one `SetPixel` per pixel (`tests/scripts.rs`).

A refusal of waiting pixels (now only a document limit) is reported at the call that writes them, named `set_pixel (waiting pixels)` and with `waiting: true`, so a refusal met inside, say, `add_frame` does not read as `add_frame`'s own. If the end of the run writes them, it is reported with no place.

### Fuel paid before the work

Every host function charges fuel through `HostCx::charge`, before it does anything, in proportion to the work it can do. A run out of fuel never starts a call it cannot pay for, so no host call bypasses the limit.

The prices are in `src/cost.rs`. They were set by measuring on the stress and large documents (Measured, below), so that the default 50,000,000 units end any mix of calls within about a second:

| Work | Units |
|---|---|
| A read of a few values | 4. `layers()` and `frames()` add the layers and frames |
| A read that finds a layer by id | 4 + layers / 16 |
| `swatches()`, `swatch_color` | 4 + 1 a swatch, paid before the list of swatches is made |
| `get_pixel`, which finds a layer and a frame | 4 + layers / 16 + frames / 64 |
| Any command | 256 + canvas area / 1,024 + layers + frames |
| `set_pixel` into a waiting block | 1. A new block costs a command plus 256 |
| `fill_rect`, and `fill` (priced for the whole canvas) | 1 a pixel |
| `draw_text` | a command, then 1 a pixel of the drawing it makes (the whole rectangle of its lines, at most 2,097,152), whether a letter lights it or not. Paid before anything is drawn, and the drawing is charged to the script's memory while it lives at the most its paste holds at once, 14 bytes a pixel (the clip 4, the cells made from it 4, 4 more when a palette layer's colors are converted a second time, and its mask at most 1.75; the paste is not made in strips, so as not to change its anchor and undo steps). So the largest drawing, 2,097,152 pixels, is charged 28 MiB and needs a run with at least that much memory; at the default 16 MiB a drawing of up to about 1.2 million pixels runs. A drawing too large to make pays only the command, then is refused |
| `text_width` | 4 + 1 per 16 characters |
| `draw_line` | 4 a pixel of its length, which is counted as at most width + height |
| `write_rect` | 2 a color |
| `read_rect` | 1 per 16 pixels. Refused before any work if its list would not fit the memory limit |
| `recolor` | canvas area / 16, times layers x frames (at most 64), plus colors / 4 |
| `duplicate_layer` | a command plus 1 for each cel it copies: its frames, for every row of the layer and, for a group, of everything in it |
| `add_swatch`, `remove_swatch` | a command plus colors / 4: each looks through the swatches and may copy the table |
| `add_frame`, `copy_frame`, `remove_frame` | a command plus layers x frames / 8: each moves every layer's cel entries. Priced as a plain command, a loop of them on 256 layers by 512 frames ran 6.3 s before fuel stopped it; now about 0.4 s |

Document size limits still apply to every operation, as in the interface.

### Copies the host makes

A host function gets its arguments as the interpreter's values and copies what it needs out of them. Those copies are outside the meter, so each is bounded before it is made:

- **Text** (names, blends, file kinds): refused when longer than 1,024 bytes (`MOST_TEXT`), before it is copied.
- **Lists of text**: `pick_file_to_read`'s and `pick_file_to_write`'s kinds are at most 8, each a text as above, checked before any is copied, and only after the run is known to hold the capability.
- **`write_file`'s text**: not copied. Its length is checked against the 64 MiB limit and paid for (one unit per 64 bytes) when it is called, and the script's own string is kept until the end of the run, so the run's memory meter goes on counting it. The file is written only once the run has succeeded (`docs/plugins.md`, "Writing a file").
- **`write_rect`'s colors**: refused when there are more than the canvas has pixels, before they are copied. The copy is 8 bytes a color and is charged to the script's memory while it lives (`HostCx::hold`), before it is made, so the list and its copy together stay within `script.memory_mb`.
- **`read_file`**: the file's length is checked first, from its metadata, against the memory left and the 64 MiB limit: a file larger than the memory left stops the run with `Memory` before any byte is read or the read is paid for. Then it is read no further than that length (a file that grew meanwhile is refused), a megabyte at a time, each piece paid for before it is read, and the bytes become a script string, charged to the meter. While it runs, the read holds the file outside the meter, at most what the meter had room for. The string is kept for the handle: reading it again costs one read and makes no copy.
- **Lists the host makes** (`read_rect`, `layers()`, `frames()`, `swatches()`) are made through `HostCx::new_int_list`, charged to the meter before they are allocated. `read_rect` also charges the API's read of the block (4 bytes a pixel) before the read, until the list is made from it.

### Growth of the document

**A run has two separate caps: the script's memory (default 16 MiB) and a document-growth budget (default 256 MiB) for what the run adds to the document.** They are separate settings (owner decision, 2026-11-17): changing `script.memory_mb` does not change the growth budget, which is `script.growth_mb`. A run that succeeds keeps what it added, in the document and its one undo step. A run that fails frees it.

The meter bounds the script's own memory, but not what it adds to the document: new layers, frames and tiles. A run may grow the document by at most `ScriptLimits::growth()`, the `growth` limit brought into its range. It is measured as the bytes the document holds (distinct drawings with their shared allocations, tiles, cel entries and colors, `Document::held_bytes`) before the first edit and again whenever the edits paid for since could have used up what is left of the budget (no edit adds more than 64 bytes per fuel unit it pays, `MOST_BYTES_PER_UNIT`; `tests/growth.rs` checks it for the edits that add the most cels at once: duplicating a group of 128 layers, a layer on 4,096 frames, a frame on 256 layers). A run that grows past it stops with `Refused(NoMemory)`, naming the call, and is taken back like any failure. On the large document, a loop of new layers each filled with one color (`hostile_growth`) ends by fuel in about half a second, not at the budget: a one-color fill makes solid tiles of 48 bytes each, so the document grows by little; a fill of new pixels is what reaches the budget (`a_run_may_grow_the_document_only_so_far` in `tests/transaction.rs`). The measuring costs a few percent at most.

### Limits are settings

`ScriptLimits { fuel, memory, call_depth, growth }` defaults to 50,000,000 units, 16 MiB, 128 and 256 MiB.

- In the settings file the keys are `script.fuel`, `script.memory_mb`, `script.call_depth` and `script.growth_mb`.
- A value is written only when it differs from the default.
- A value out of range is brought into range: fuel from 10,000 to 2,000,000,000, memory from 1 to 256 MiB, depth from 1 to 256, growth from 16 to 2,048 MiB.
- **Why that growth range.** 16 MiB is one 2048 x 2048 cel, the least that lets a script redraw a large canvas once. 2,048 MiB is eight cels of the largest canvas (8192 x 8192, 256 MiB each, `MAX_DIMENSION`). A run that succeeds briefly holds what it added twice (the scratch editor and the document it replaces, plus the undo step), so a higher budget would let one run ask for more than a typical machine gives one program.
- The Scripts panel shows all four: "Limits: *fuel* fuel, *M* MB memory, *G* MB document growth, depth *D*".

### Started on first use

Nothing about scripts exists until a script runs, and nothing is made at launch.

- Each run compiles its source and runs on a scoped thread with a 16 MiB stack. The thread ends with the run.
- No thread, interpreter state or allocation is kept between runs.
- Cancel is the interpreter's flag. It is read at least every 1,024 fuel units, and within each host step. No host step does more than its fuel pays for, so a step ends quickly once the flag is set.

### Memory

**Decision: no private heap.** The documented bound is the interpreter's live meter. Every string, list and call frame, and the program itself, is charged before it is allocated. The meter counts only what is live, conservatively, and stops the run at the limit (`docs/scripting-language.md`, Memory accounting).

A private heap destroyed after each run cannot be had safely. Host calls on the run's thread make the document's new drawings, and those outlive the run when it succeeds. Sending only the interpreter's values to another heap would mean rewriting its value storage as an arena. That question is left open (below).

**The two limits, and what is outside them.** A run is held to two separate limits: the script's memory (`script.memory_mb`, default 16 MiB, charged before each allocation) and the document growth (`script.growth_mb`, default 256 MiB, measured as the document's held bytes). The accounting is conservative: an allocation is charged at its rounded-up size before it is made, and growth is measured whenever the edits since the last measure could have used up what is left. Every allocation a run makes that neither limit strictly bounds is listed here, with its actual bound. *P* is the canvas's pixels (at most 8192 x 8192 = 67.1M), *C* the document's cels (at most 256 layers x 4096 frames = 1,048,576).

| Allocation | When | Bound | Freed |
|---|---|---|---|
| The transaction's scratch document (`Editor::transaction`, `scratch`) | the start of every run | a shallow copy: 16 bytes a cel entry (16 MiB at most) and the layer list; images, tiles and the color table are shared (`Arc`) until written | with the run, or it becomes the document |
| Copy on write of existing pixels | the first write to a tile, or to the color table, the document already holds | at most once per tile and once for the table, so at most what the document held before the run (`held_bytes` at the start); not growth, because the old tile is replaced, not added to | old copies when a successful run's result replaces the document; new ones when a failed run is dropped |
| A stroke's overlay (`draw_line`, `set_pixel` blocks, fills) | each drawing command | 1,056 bytes for each 16 x 16 tile it touches, about 4.1 bytes a canvas pixel at most (about 270 MiB on the largest canvas), plus 8 bytes a tile slot; touched tiles are pooled for reuse | when the run's scratch editor is dropped |
| Flood fill working memory (`fill`) | each `fill` | 1 byte a canvas pixel for the visited map, plus the overlay above and a stack of one entry per run of cells; measured 40.7 MB on the 2048 x 3040 large document (6.6 bytes a pixel). A fill pays one fuel unit per canvas pixel first, so at the default fuel no fill runs on a canvas of more than 50M pixels | at the end of the command |
| Waiting pixel blocks (`set_pixel`) | between calls | at most 256 blocks of 4,096 colors, 4 MiB | when written, at the next other operation |
| `draw_text`'s paste (clip, cells made from it, a second conversion in a palette layer, mask) | each `draw_text` | at most 14 bytes a pixel of the drawing, which is at most 2,097,152 pixels (28 MiB); charged to the script's memory with `HostCx::hold` before the drawing is made, so a drawing the memory limit cannot hold is refused as out of memory | when the paste is anchored |
| Paste strips (`write_rect`, `fill_rect`, waiting blocks) | each block write | at most 65,536 colors a strip (256 KiB) and its mask, one strip at a time | after each strip |
| `held_bytes` measure (growth) | before the first edit and at each measure | 8 bytes a cel (8 MiB at most) for the sort, plus 8 bytes a tile reference of the distinct images, no more than the tile tables the document already holds | at the end of the measure |
| `read_file` buffer | each `read_file` | checked from the file's metadata first: no more than the script's memory had room for, and at most 64 MiB; the bytes then become a script string charged to the meter | when the string is made |
| The run's thread stack | each run | 16 MiB reserved (`RECOMMENDED_STACK`), committed as the interpreter uses it | with the run |
| `PluginStore::list` (not a run) | opening the Scripts menu, on the UI thread | reads and hashes up to 256 plugin folders of at most 1 MiB each (`MOST_LISTED`, `MOST_PACKAGE_BYTES`): up to 256 MiB, kept in the app's plugin list (`plugins.listed`) | when the list is next read again |

`write_rect`'s copy of its colors and `read_rect`'s read of the block were outside the meter until 2026-11-17 (security review L1); both are now charged with `HostCx::hold` (`tests/host_copies.rs`).

The run's own editor keeps no undo history: each command's log is dropped as it goes.

- **Before this change**, a loop of edits held up to the 256 MiB history budget until the run ended.
- **Measured case:** duplicating and removing a layer of the stress document took 296 MB.
- **Speed:** the same loop took ten times as long.

### Measured

`pikslar-bench script` runs each case in a process of its own, using the release build of `pikslar-bind` that the app ships, with the default limits. Private bytes are sampled every millisecond and counted over those just before the run. Times for a completed script are the median of 5 runs.

| Case | Document | ms | Ended by | Peak private, KB | After, KB |
|---|---|---|---|---|---|
| First script in the process (`return 1;`) | 128 x 128 | 0.18 to 0.26 | done | | 8 to 12 |
| Later runs, median of 21 | 128 x 128 | 0.08 to 1.5 | done | | |
| Ordinary: rename 12 layers, add 10 frames, 8 tags | stress (12 layers, 400 frames) | 0.5 to 1.4 | done | | |
| `set_pixel` over 256 x 256 | large (2048 x 3040) | 7.6 | done | | |
| `set_pixel` over 1024 x 1024 | large | 153 | done, 17.4M fuel | 9,812 | |
| `fill_rect` of the whole canvas | large | 51 | done | | |
| 100 diagonal lines | large | 12 | done | | |
| `read_rect` 256 x 256 | large | 1.5 | done | | |
| Read 256 x 256, change each color, `write_rect` | large | 7.5 | done | | |
| `while true {}` | 128 x 128 | 150 | Fuel | 100 | 12 |
| Unbounded list | 128 x 128 | 23 | Memory | 9,368 | 592 |
| 50,000 small strings freed, then one 15 MB string | 128 x 128 | 27 | done | 14,968 | 200 |
| Unbounded recursion | 128 x 128 | 1.6 | CallDepth | 84 | 20 |
| Add and remove a frame, forever | stress | 333 (201 since frame edits are priced by the cels they move) | Fuel | 300 | 204 |
| Duplicate and remove a layer, forever | stress | 901 | Fuel | 752 | 656 |
| Recolor a swatch used on 64 cels, forever | stress | 20 | Fuel | 532 | 436 |
| Whole-canvas diagonal line, forever | large | 937 | Fuel | 25,476 | 992 |
| Whole-canvas flood fill, forever | large | 415 | Fuel | 40,736 | 1,064 |
| Whole-canvas `fill_rect`, forever | large | 432 | Fuel | 3,356 | 1,104 |
| Scattered `set_pixel`, forever | large | 168 | Fuel | 11,820 | 2,392 |
| `write_rect` 512 x 512, forever | large | 359 | Fuel | 9,036 | 700 |
| `read_rect` 512 x 512, forever | large | 302 | Fuel | 10,544 | 0 |

- **Script memory** stayed within the limit: 14,968 KB at most, against 16,384 KB.
- **Peaks above 16 MiB** are host work on the large document's 24 MB cel: the line's copy of the cel, and the flood fill's working memory.
- **After a run**, at most 2.4 MB is left. That is the allocator keeping freed blocks; no values are still live.

The app itself was measured with `spikes/limits.ps1`, without `-Latency`. The `pikslar.exe` sizes are for the release build.

| Line | dev/studio 2e9640e | After |
|---|---|---|
| `launch_median_ms` | 23 (21 on a second run) | 23 |
| `private_128_mb` | 7.17 (7.36 on a second run) | 7.37 |
| `idle_cpu_percent` | 0 | 0 |
| `program_kb` | 2,015.5 (2,063,872 bytes) | 2,219.0 (2,272,256 bytes) after the binding: +203.5 KB. 2,282.5 (2,337,280 bytes) with the Scripts panel and plugins (12.5, 12.6), after the size commits 1af9bd1, 4500df0 and 2c66bf4. Soft limit 2,304. |

`pikslar-bench check` and `creative-check` were run before and after. No line is over its limit that was not over before:

- `check`: only `history_undo_redo_all_2000` is over, both times.
- `creative-check`, after: only `tile_cache_bytes_worst` is over.
- `creative-check`, before: six more lines were over. They are noise on this machine.

### The script gate

`pikslar-bench script-check` holds the scripting lines to two tracked files of their own; the files of the other gates are not read or written. `spikes/run-all.ps1` runs it as its own step, after the creative gate.

- `perf/script-baseline.txt`: every `_ms` line. A line fails when it is over 1.5 times its value plus 1 ms (`script_gate::TIME_RATIO`, `TIME_SLACK_MS`).
- `perf/script-limits.txt`: absolute ceilings, `name hard ceiling unit` as in `perf/limits.txt`. Each `script_hostile_*_stop_ms` has a ceiling of 2,000 ms. Each `script_hostile_*_peak_private_kb` has a ceiling of the memory setting (16 MiB) plus twice the larger of the document's largest cel and what the document holds (`script_bench::peak_ceiling_kb`). The second term is there because recoloring a swatch on the widest stress document peaks at 28.7 MB, while its largest cel is small: the scratch document's copy on write is of what the document holds, not of one cel.
- `script-update` appends only the lines that are missing, each the median of 3 measurements, and never rewrites a line.
- Repeatable: each case runs in a process of its own, on fixed documents built in code, with the default limits. An ordinary case has one untimed warm-up run and then the median of its timed runs; a hostile case reports its peak from its first, cold run and its stop time as the median of 3.
- A failing check is measured again, up to 4 times in all, each in new processes, keeping the best reading of each line. A real slowdown is slow every time.

Spread over five whole `script-measure` runs on the recording machine, slowest over fastest: 30 of the 35 time lines within 1.10. The widest were `script_first_use_first_ms` 1.18 (0.18 to 0.22 ms, which the 1 ms slack covers), `script_hostile_canvas_stop_ms` 1.14, `script_pixels_1024_large_ms` 1.24, `script_timeline_ordinary_ms` 1.25, and `script_hostile_lines_stop_ms` 1.40 (one run of 1,372 ms against a median of 987). The first check after recording passed every line and ceiling.

### Large projects

Measured by the gate, release build, default limits.

| Case | Document | ms | Ended by | Peak private, KB | Ceiling, KB |
|---|---|---|---|---|---|
| Rename all layers, set all durations, read, recolor and write back 32 x 32 on 64 cels | 256 layers x 512 frames, 64 x 64, 16,384 drawings | 12 to 15 | done | | |
| Read, recolor and write back sixteen 256 x 256 regions, fill 512 x 512, add and recolor a swatch | 4096 x 4096 | 95 | done | | |
| Every layer's kind and name, rename, durations, read 64 x 64 | tilemap stress map | 1.5 | done | | |
| Add and remove a frame, forever | 256 x 512 | 374 | Fuel | 8,056 | 34,340 |
| A pixel of the last frame on every layer, forever | 256 x 512 | 412 | Fuel | 92 | 34,340 |
| Fill the canvas, then flood fill all of it, forever | 4096 x 4096 | 315 | Fuel | 94,584 | 147,456 |
| Every layer's kind and name, forever | tilemap stress map | 796 | Fuel | 92 | 49,152 |

Every hostile case on every document stopped within 1 s at its median; the slowest is the whole-canvas line, at 987 ms. The 4096 x 4096 flood fill's peak is its working memory, outside the script's limit (see Memory).

## The Scripts panel

Phase 12.5 (`crates/pikslar/src/script_panel.rs`, drawn by `script_draw.rs`).

**For the user.** Press Scripts in the top bar and choose "Script panel...". Then:

1. "Open script..." chooses a `.pks` file.
2. "Run script" runs it on the open document.
3. "Stop script", or Esc, stops a long run. The window keeps drawing while a script runs.
4. "Clear output" empties the output. "Close scripts", or Esc when nothing runs, closes the panel.

- **A run that succeeds** and changes the document is one undo step: the panel says "*name* ran: one step to undo", and that undo says "Undid script *name*". A run that recorded no step (it only read, or only changed what has no undo step, such as visibility) says "*name* ran: nothing to undo", and no later undo names it. The step is keyed on the history's push count and revision, and forgotten when another document is opened or made.
- **A run that fails or is stopped** changes nothing, writes no file, and adds no undo step. The panel says "*name* changed nothing. Error at line *L*, column *C*: …" when there is a place, "*name* changed nothing. The pixels it left to write were refused: …" for pixels refused at the end, "*name* did not run. Finish the stroke or place the floating pixels first" while the document is busy, "*name* could not start: …" when its thread could not start, "*name* changed nothing. The file it was saving was not written: …" when a plugin's file could not be written at the end, or "*name* was stopped. Nothing changed".
- **An open gesture is finished first, as the user's own step.** Before a run starts, the app finishes whatever the user had in progress (`App::finish_gesture`): a stroke is committed and a drag is ended, and playback stops, whatever the run then does. That commit is the user's own work and their own history step, not the script's, so it stays when the run fails or is stopped. "Changes nothing" above is about what the script did: a failed run adds no step of its own, but the history may hold the user's finished stroke where a moment before it was still open. This is so the run is not refused as busy; refusing to run while a gesture is open was the other choice.
- **Stop during a file read.** `read_file` reads the chosen file (at most 64 MiB, and no more than the script's memory has room for) a megabyte at a time, and pays for each piece before reading it, which is when the cancel flag is read. Stop therefore ends a read within one piece: on a slow network share or a cloud placeholder file, within the time one megabyte takes, not the whole file. The read is checked against the memory and fuel before it starts, and the run is then stopped and taken back as usual. **Known limitation: Stop cannot end the open.** The cancel flag is read between pieces, not while the file is being opened. `read_file` opens the file to find its length and again to read it, and each `File::open` is a blocking system call with no cancel. If the file is on an offline share, a disconnected drive or a cloud placeholder that must first be fetched, Stop is not seen until the call returns, which can be as long as the system waits (seconds to minutes) and during which the run, and the window, wait for it. Nothing is read or changed meanwhile, and the run is stopped and taken back as soon as the call returns. An asynchronous open was not built.
- **Stop while files are written.** A plugin's files are written at the end of a run that succeeded. A Stop seen before they are put in place writes none and takes the run back; once they are being put in place (renames, each quick) the run finishes.
- **Output.** What the script printed stays in the panel, the last 200 lines, even when it fails. Each printed line and the status are cut to 1,024 characters, with "..." after them, before they are kept, and the characters in `pikslar_plugin::hidden` (direction overrides, zero-width marks and the like) are taken out; a tab becomes a space. Fitting a line to the panel is one pass over it, so a megabyte line draws as fast as a short one.
- **Limits.** The panel shows the limits from the settings (`script.fuel`, `script.memory_mb`, `script.call_depth`, `script.growth_mb` in `settings.txt`, read by `crates/pikslar/src/settings_script.rs`).

**How it runs.** A run starts on the frame after Run is pressed, so the panel already says "running". The window thread then calls `run_beside`, which runs `run_script_with` on a scoped thread. Meanwhile the window thread waits in `pikslar_platform::wait_answering`, which:

- pumps messages and repaints from the last frame;
- answers a plugin's file question (see `docs/plugins.md`);
- passes key and button presses to the panel, so that Stop or Esc sets the cancel flag;
- drops all other input until the run ends.

None of the messages handled during the wait reach the app's own handler, so nothing lent to the script's thread is touched meanwhile: the window procedure reaches the app only through `process()` and `session_ending`, and both are held off while the wait runs. If the wait ends with the run still going (the window is closing, or the app quits), the run is cancelled, a file question it is waiting on gets no file at once, and its thread is joined.

**Run with files.** `run_script_with(editor, source, caps, limits, cancel, files)` is `run_script` with an optional `FileAccess`. Only a plugin's run is given one, made from its granted capabilities and a picker. The functions `pick_file_to_read`, `read_file` and `file_name` need `Capability::PickFileToRead`, and `pick_file_to_write` and `write_file` need `Capability::PickFileToWrite`; `Capabilities::ALL` holds neither. `FileAccess::with_disk` sets how files are put in place: the app gives the system's `ReplaceFileW`. A run whose files could not be written fails with `Failure::Files`, and the document is unchanged.

Real-window check: `spikes/e2e-scripts.ps1` (24 checks: load and run with the pixels read from the command line's PNG export, a failing script, a long run stopped).

## From the command line

Phase 12.3 reaches this API from `pikslar-cli`:

- `batch` applies a file of operations to a project as one run.
- `session` is a JSON lines conversation over standard input and output, where each `run` request is one run. It may open and save files only inside the folder given with `--dir`.

Both use a strict JSON reader of their own. See `docs/cli-json-lines.md`.

## Open questions

- **Frames have no persisted id**, and the layer floor is not saved. Inside Pikslar no reference outlives a reopen. A reference kept outside, such as a hand-written script naming a layer id, could still meet a reused id after a reopen. `docs/stable-ids.md` proposes an optional section, not built, for the owner to decide on.
- **Tileset and tile ids after undo** can be given again; scripts cannot reach them yet. See `docs/stable-ids.md`.
- **Not yet covered.** Tags beyond adding (by place, since tags have only session ids), slices, groups, tilemaps and tilesets (whose ids are persisted), and selection. To copy a region, use `read_rect` and then `write_rect`.
- **A hard bound on process memory** would need the interpreter's values in an arena of their own, released after each run. The meter bounds live script memory. The process can briefly keep freed blocks: 0.91 of the limit at worst, as measured.
- **Refusals of waiting pixels** are reported at the call that writes them, named as waiting pixels, not at the `set_pixel` that queued them. Locks and layer kinds are checked at `set_pixel` itself, so only a document limit is left to meet there.
- **The fuel prices fit this machine.** On a slower machine a run stops proportionally later. The limits are settings.
