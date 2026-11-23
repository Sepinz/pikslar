# M3: export and command line

The plan for milestone 3. Export leaves the app, gains the formats animators hand to engines and
the web, and gains a program that exports without a window. The phase list is the roadmap's,
"M3 Export and command line". This document settles what the roadmap leaves open. An
independent review of the first draft found two Critical and five High problems; each is
answered below, and section 7 lists them.

## 1. What exists today

- `pikslar-render::flatten` turns one frame into straight RGBA, using the same composition as the
  canvas. Since 3.1, `flatten_document` does the same from a document alone.
- Since 3.1, `pikslar-export` writes a frame as PNG, and a sheet: every frame in rows, a row per
  tag, a PNG and a JSON description in the de facto atlas shape.
- `pikslar-format` has its own PNG encoder and decoder, deflate, inflate and atomic replacement.
- The workspace uses no third-party crate. That is a hard limit, checked by the suite.
- Limits that shape export: images up to 8192 a side, 4,096 frames, 256 layers, frame durations
  of 10 ms to 60 s.

## 2. Architecture

```text
pikslar-core  <-  pikslar-render  <-  pikslar-export  <-  pikslar (app)
      ^                                ^       ^
      +---------- pikslar-format ------+       +----  pikslar-cli
```

- **`pikslar-format` keeps bytes.** Each encoder takes finished images and returns bytes: PNG,
  indexed PNG, APNG, GIF, ICO. Nothing in it knows about layers, tags or files chosen by a person.
- **`pikslar-export` decides what is exported**: which frames, layers or tag, in what order,
  scale, sheet layout, the JSON, file names, and writing them. It takes a `&Document`, so it
  cannot change one. It depends on core, render and format only.
- **`pikslar-cli` is a separate small program**, `pikslar-cli.exe`, over core, format and export.
  It never links `pikslar-platform` or `pikslar-ui`, so it creates no window, device or GPU
  resource. The suite checks the dependency tree and the program's import table, which must not
  name the window or graphics libraries the app uses.
- **Composition is reused, not copied.** Every exported pixel comes from `flatten_document`,
  which runs the canvas's own tile composition with nothing in progress.
- **The app** exports through the same `plan()` and target checks as the command line (since
  3.9). It reads `Editor::shown_document()`: the document itself, or, while pixels are lifted,
  a copy with them written where they are shown, by the same code anchoring uses. Nothing in
  the editor changes (decision D1).

### Memory

- **No image an export makes is larger than 8192 a side**, the limit for documents. Frames, scaled
  frames and sheets are checked against it before anything is allocated, and refused with the
  limit named. Scale is bounded by this. Since 3.3b `check_size` does it for every format, in
  128-bit arithmetic so no scale overflows, when the export's `Picture` is made: before any file
  or lock exists.
- **Scaling holds no enlarged image (since 3.3b).** Each document row is widened once and sent to
  the encoder as many times as the scale. What is held is one band of document rows per frame
  and one band of the image's width, so memory is bounded by the output's width, never its area.
- **An indexed export finds its colours first (since 3.4)**, drawing each frame once at the
  document's own size while the export is planned, then again to encode. Enlarging adds no
  colour, so the first pass is never scaled. What it holds is one band and at most 257 colours,
  and it stops at the 257th. The palettes found are kept for writing, at most 1 KiB each.
- **Frames are flattened one at a time** and encoded as they come. A GIF reads every frame up to
  three times: once for the colors, once to compare it with the frame before, and once to encode
  what changed. Nothing keeps all frames.
- **No export holds a whole image (since 3.3).** A frame is flattened a band of 16 rows at a time
  (`BandSource`), each band goes straight into the PNG encoder, and the encoder writes 1 MiB
  `IDAT` chunks to the file as they fill. A sheet holds one band of its own width. The DEFLATE
  window and the band are the only working buffers. Measured above the loaded document: about
  5 MiB at 8192 a side, the same for one frame, a sheet or a sequence of any length. Only a
  sequence's description grows with its frames, by under 1 KiB each.
- **Memory limits.** 1 GiB peak private memory is the temporary ceiling for the largest
  workloads; a single 8192 by 8192 PNG is under 512 MiB (49 MiB measured, most of it the
  document). Peak is measured at 8192 a side and on the bench's stress document (128 by 128, 12
  layers, 400 frames).

### Writing

- **Each file is all or nothing**: written beside its target, read back against a hash taken as
  it was written, then renamed over it.
- **A set of files is not**, so a set is ordered so that a description is never beside files it
  does not describe. A sheet's PNG and JSON, and a PNG sequence, are all written to temporary
  files and checked before anything is replaced. Then the old description is removed, the files
  are renamed one by one, and the new description comes last. A rename can still fail part way:
  the program then says which files were replaced, and no description is left. The app's sheet
  export follows the same order (since 3.3).
- **Last checks before anything is replaced.** Once every file is written, each description is
  checked again for its owner, and on Windows each target for another program holding it open.
  Either stops the export with every target as it was.
- **One export at a time per set.** A lock in the user's temporary folder, named by a hash of the
  set's folder (resolved) and name (in any case), is held while the set is written. On Windows it
  is the open file itself, so a killed export's lock is taken over by the next. Elsewhere only a
  new lock file excludes another export. It is not beside the set, because a file made and
  removed there slowed the export.
- **Sequence names have a fixed width**: `name_0001.png` upward, numbered by position in the
  export, four digits, since there are at most 4,096 frames. The description gives each file's
  project frame. Files with higher numbers left from an earlier, longer export, in any case of
  ASCII letters, stop the export (exit 4) and are named, since they would look like part of it.
- **Links are refused** as targets on the command line.
- **The CLI replaces no file unless asked.** Without `--overwrite`, any existing target stops the
  export before anything is written. It never writes over the project it read. A file that
  appears between that check and the rename is replaced; this is stated, not hidden.
- **The app asks instead (since 3.9).** The save dialog asks about the file it shows. Every
  target is then checked, and only then are other existing files (a sequence's members) asked
  about, by name and count; No writes nothing. The two differ in one case, on purpose: a `.json`
  someone else wrote makes the command line refuse the whole export, even with `--overwrite`,
  since a script should not get half of what it asked for; the app writes the image, keeps that
  `.json` and says so. Either way the `.json` is never replaced, and the app still takes the
  lock named for it.
- **Deterministic.** No time, randomness or machine detail goes into a file. Chunk, key and palette
  order are fixed. The same project and options give the same bytes from the CLI and the app.

### The command line

```text
pikslar-cli export <project> --format png|sheet|sequence|gif|apng|ico
    [--out <path>] [--frame N | --frames 1-12 | --tag <name>] [--layer <name>]... [--scale N]
    [--loop N] [--indexed] [--columns N] [--padding N] [--json hash|array] [--overwrite]
pikslar-cli info <project> [--json]   # size, frames, layers, tags, durations
```

Frames are counted from 1, as in `info` and the app's timeline.

Exit codes, documented in `--help`: 0 done, 1 bad arguments, 2 project could not be read,
3 project cannot be exported as asked (limits, too many colors for the format), 4 output could not
be written. Messages go to standard error, one line, no panic text. Projects are read with the
existing limited reader and loaders, which the safety suite already mutates.

## 3. Formats and rules

| Format | Rule |
|---|---|
| PNG | As today, RGBA |
| Indexed PNG | Since 3.4. Only with `--indexed`, for every format, scale and choice of layers. Colour type 3 at 1, 2, 4 or 8 bits, the fewest that number the colours, with a `tRNS` only if an entry is not opaque. Each image's palette is exactly its own colours: an image of more than 256 is refused (exit 3) before any file is made, and nothing is ever quantized or dithered. The file holds exactly the pixels of the RGBA export, colours stored behind clear pixels included (see below). Palette order: **in palette order (since 9.7) when every colour the image shows, apart from fully clear pixels, is one of the document's swatches**: the clear colour first if the image has it, then the colours the image uses in the order of the swatches (swatches it does not use are left out, since a palette is exactly its image's colours; a colour two swatches share goes at the first). Otherwise, and for every document with no swatches, as before: colors not opaque first so `tRNS` stays short, then the opaque, each group by its 32-bit value (alpha, then blue, green, red, highest first in significance). Either way the same image always has the same palette wherever its frames are placed. No existing golden changed: no fixture has every used colour as a swatch. A sheet has one palette, the space between its frames counting as a colour; each member of a sequence has its own. Descriptions say so (since 3.5, see below) |
| PNG sequence | Fixed-width names, as above |
| Sheet | Since 3.8. By default a row for each tag, as before, bytes unchanged. `--columns N` puts N frames a row in frame order, tags then starting no row. `--padding N` puts N clear pixels between frames, none round the edge; a side is `count * size + (count - 1) * padding`. A sheet over 8192 a side is refused before any file, naming columns and padding (and scale, above 1). `--json array` (the default, as before) lists frames with a `filename`; `--json hash` keys them by name, as Pixi reads it. Phaser reads both. The rectangles were checked against each frame's own export by Pillow, and the fields loaders read are present; no loader library was run |
| GIF | See below |
| APNG | Since 3.7, see below |
| ICO, CUR | Last, and only if time allows. Sizes up to 256, PNG-compressed entries |

**What an indexed description says (decided by the owner, 2026-11-12).** An RGBA export's
JSON is unchanged, byte for byte: `meta.format` is `RGBA8888`. An indexed sheet's `meta.format`
is `INDEXED`, followed by `bitDepth` (1, 2, 4 or 8, the PNG's own) and `paletteSize` (its
entries). An indexed sequence's `meta.format` is `INDEXED`, and each entry in `frames` has its
own `bitDepth` and `paletteSize`, since each member has its own palette.
- Compatibility: loaders that read rectangles, durations and tags (the fields Phaser, PixiJS
  and most engine importers use) are unaffected, and every PNG decoder gives the same RGBA
  pixels. A tool that checks `meta.format` for a value it knows, such as `RGBA8888` or
  Aseprite's `I8`, will not know `INDEXED`. It is not `I8` because the file may be 1, 2 or 4
  bits a pixel, which `I8` would misstate. The added keys are ones such tools ignore.

**Clear pixels in indexed exports stay as they are.** Hidden colours behind clear pixels are
kept by default and never erased without being asked (decided by the owner, 2026-11-12). An
option to clear them before the palette is found, such as `--clear-transparent-rgb`, is deferred:
it is a separate choice, not part of 3.5.

**GIF (since 3.5).** `--format gif`, for every choice of frames, layers and scale.
- **One global palette, exact**: the colours of every frame shown, found while the export is
  planned (each frame drawn once at the document's size, holding one band and at most 257
  colours), before any file or lock exists. Clear entry first if any pixel is clear, then the
  opaque colours in palette order when every colour is a swatch (since 9.7), else by value; the table is the fewest bits that hold them, and LZW starts at the
  fewest bits it allows (2). More than 256 entries (255 colours with clear) is refused, exit 3:
  nothing is quantized or dithered.
- **Alpha is opaque or clear, and nothing else is changed (decided by the owner, 2026-11-12).**
  A pixel of alpha 1 to 254 is refused, exit 3, naming the frame and where the first one is.
  It is reported before too many colours, wherever each is, so fixing what one refusal names
  never uncovers the other; finding it may take a look at every frame chosen. No threshold is applied without being asked; an explicit conversion policy, if wanted, is a
  later option. Blending translucent layers over an opaque one gives opaque pixels and exports.
- **Clear pixels.** Every clear pixel is the one clear entry: GIF keeps no colour behind it, so
  colours hidden behind clear pixels (kept by RGBA and indexed PNG) are not in a GIF and do not
  count towards its colours. The pixels shown are the same.
- **Each frame draws only what changed (since 3.6).** The frames are compared in play order
  while the export is planned, at the document's size, holding two bands. A frame draws, in one
  rectangle, the opaque pixels that differ from what the canvas shows when it starts, and keeps
  the rest (disposal 1). A drawn pixel cannot make the canvas clear again, so when the next frame
  clears pixels this one shows, this frame's rectangle grows to cover them and is restored to the
  background, the clear entry, after it is shown (disposal 2); the next frame then draws again
  what it keeps there. A frame that changes nothing draws one pixel that is already shown. The
  first frame draws its opaque pixels, so a still image is cut to them.
- **Loops start the same however a player restarts.** Some players clear the canvas between
  plays and some do not. The first frame draws every pixel it shows, so the last frame of a loop
  clears only the pixels it shows that the first does not, and the first frame is the same
  either way. A GIF that plays a fixed number of times ends on that last frame; a player that
  applies its disposal after the final play, rather than holding the frame, shows those pixels
  cleared. Browsers hold it. 3.5 cleared every frame, so this is no worse.
- **Considered and not done.** Unchanged pixels inside a rectangle could be written as the clear
  entry: measured, it saved under 2% except on one small idle sprite, needs the frame before
  drawn again for every frame, and needs a clear entry an opaque image does not have. Local
  palettes cost a table per frame for at most a bit per pixel. Merging identical frames would
  change the number of frames players and tools count; a frame that changes nothing costs
  about 30 bytes.
- **Order.** Every frame by default, or the frames chosen, in order; `--tag` plays in the tag's
  direction (below). One frame is a still image: no loop extension and a delay of 0.
- **Timing.** A delay is in hundredths of a second. Each duration is rounded to the nearest
  hundredth, halves up, and the rounding error is carried to the next frame, so the total stays
  within 5 ms of the project's. Most players show a delay of 0 or 1 as 100 ms, so a delay that
  would be under 2 becomes 2 (20 ms); that time is not taken back from other frames, so it
  lengthens the animation by what it adds. The CLI notes, once, how many frames were
  lengthened. Durations of 10 ms to 60 s (the project's limits) become 2 to 6000.
- **Memory.** The comparison holds two bands and, for each frame played, four rectangles (a few
  dozen bytes). Writing draws again only the bands a frame's rectangle crosses; what is held is
  one band, one enlarged row, its indices, the 4096-code LZW table and 64 KiB of coded bytes.
  No frame is held whole.

**APNG (since 3.7).** `--format apng`, for every choice of frames, layers and scale. The file
ends in `.png`, and is named after the project if `--out` is not given.
- **Pixels.** 8-bit RGBA, exactly the cells the PNG export of each frame has: partial alpha,
  layer opacity and blending as the canvas shows them, and the colour stored behind a clear
  pixel. Nothing is refused for colours or alpha, and `--indexed` is a usage error.
- **Chunks.** The signature, `IHDR`, `acTL` (frames, plays), then for each frame an `fcTL` and
  its data, `IEND` last. The first frame's data is the `IDAT` a viewer without APNG shows; the
  others' are `fdAT`. `fcTL` and `fdAT` share one sequence count from 0, in file order. A
  frame's data is compressed as a PNG's, split into chunks of at most 1 MiB.
- **Each frame draws only what changed.** The first frame is the whole image. Every later frame
  is the rectangle of cells that differ, in any channel, from the frame shown before it in play
  order; a frame that changes nothing is one cell already shown. Every frame replaces what it
  covers (`blend_op` SOURCE) and leaves it (`dispose_op` NONE), so a translucent or clear cell
  never shows the frame before, and a loop starts the same whether a player clears the canvas
  between plays or not.
- **Timing and loops.** A delay is the duration in milliseconds over 1000, exactly. `--loop N`
  is the number of plays, 0 (the default) forever, up to 65,536 as for GIF. One frame plays
  once. A frame of 10 ms is kept, and a note says how many there are, since some browsers show
  frames that short as 100 ms.
- **Memory.** Planning draws each frame shown once at the document's own size, a band at a
  time, holding two bands and a rectangle per frame. It runs once the destination has been
  checked, so a refused file is refused at once, and not at all for a single frame. Writing draws again only the bands a
  frame's rectangle crosses; what is held is one band, one enlarged row, the compression window
  and at most 1 MiB of compressed bytes. No frame is held whole.
- **Considered and not done.** Writing the unchanged cells of a rectangle as clear and drawing
  it OVER: measured on the 3.6 and 3.7 scenes it saved about 1% more over all, needs every
  changed cell to be opaque and the frame before drawn again while writing. PNG row filters:
  every PNG this program writes uses filter 0, and adding filters would change the bytes of
  every PNG export, so they are a separate decision. Merging identical frames would change the
  frame count players and tools see.

**Order, looping and scale.**
- `--loop N` is the number of plays, and 0, the default, is forever. GIF: forever is a loop count
  of 0; N plays is N - 1, and one play leaves the loop extension out, so a GIF plays at most
  65,536 times or forever. `--loop` with another format is a usage error. APNG: `num_plays` is N,
  with the same limit.
- Exporting the whole document plays frames in order. Exporting one tag with `--tag` follows its
  direction: reverse plays it backwards, ping-pong plays forward then back without repeating the
  ends, so 0 1 2 3 2 1. A tag of one frame is that frame; of two, 0 1.
- Sheets and sequences keep frame order and give the direction in the JSON, as today.
- Scale is whole numbers, nearest neighbor, after composition. A sheet's places and its JSON's
  rectangles are in exported pixels, and its `meta.scale` is the scale; a sequence's `meta.size`
  is the exported size.
- Layers (since 3.3b): by default the visible layers, as the canvas shows them. `--layer <name>`,
  given once or more, draws only those layers, shown or hidden, each with its own opacity and
  blend mode, stacked in the project's order. A name two layers share is refused rather than
  guessed. The pixels are those of the document with exactly those layers shown.
- Clear pixels (since 3.4, decided for indexed PNG): an export keeps the RGBA values the picture
  has, exactly. Where layers are blended a clear pixel is (0, 0, 0, 0). Where one layer at full
  opacity is drawn alone, its cells are written as stored, so a clear cell keeps any colour
  stored behind it; that has been true of RGBA exports since 3.1, and indexed exports keep it
  too, so they read back as the same values. Such colours count towards the 256, and a refusal
  says at least how many of the colours found are clear pixels with a colour behind them.

## 4. Dependencies

- **None at run time.** The third-party crate limit stays at zero. What makes image formats risky
  is reading hostile files, and these phases only write. A GIF's LZW encoder and APNG's extra
  chunks are a few hundred lines over the PNG and deflate code we already have and test. Every
  format is checked by a decoder we did not write. If an encoder proves harder than this, the
  fallback is a small, widely used crate such as `gif`, with the owner's approval and the limit
  raised for that crate only.
- **Test only: ffmpeg**, from a Windows build linked from ffmpeg.org, unpacked into a tools folder
  outside the repository, with no installer, path change or system setting. It decodes GIF and
  APNG frame by frame and reports timing. Windows' own decoder, already used by the workflows,
  is the second reader for PNG, GIF and ICO. If ffmpeg cannot report a loop count, ImageMagick's
  portable build from imagemagick.org is added for that alone. Neither ships, and the suite says
  when it skips a check because a tool is missing.
- **The app's size.** It was at 911.5 of 1,024 KB. The encoders join the app only in 3.9, and its
  size is reported in every phase. If 3.9 would pass the limit, that comes to the owner first.
  **It did:** 1,090.5 KB at `88c3694`. Nothing was changed to hide it; where the growth comes
  from is in `docs/studio-m3-close.md`. The owner accepted it and raised the soft budget to
  1.25 MiB (2026-11-13).

## 5. Acceptance

Each phase ends with format, lint, tests, mutation checks on its new logic, the suite, the gate and
an independent review. M3 is done when:

- **Pixels.** Every corpus file exports byte-identically before and after 3.1, as each frame and as
  a sheet with its JSON. Done in 3.1.
- **Each format, read by a third-party decoder,** has the right dimensions, colors, transparency,
  frame count and order, frame timing and loop count. The CLI's and the app's files are the same
  bytes.
- **Headless.** Output is the same twice in a row. Each of these gives the documented exit code
  and leaves no file under the chosen name: a malformed project, a missing one, a directory as
  output, a missing folder, a read-only file, the project itself as output, and reserved names
  (`CON.png`, `NUL`, a name ending in a dot or a space). Peak memory is measured as in section 2
  and held under the limit set then.
- **No cost to drawing.** The gate shows nothing new: only `history_undo_redo_all_2000`, the
  accepted exception. The baseline and tolerance are unchanged. The app's launch, memory and size
  stay within the hard limits. Real-mouse latency is measured once at the end of M3, with the
  owner's approval, as for earlier phases.
- **Review.** No open Critical or High finding.

## 6. Order

| # | Work |
|---|---|
| 3.1 | `pikslar-export`; frame and sheet export moved out of the app; `flatten_document`. Golden hashes of every corpus file's exports recorded first, then compared. **Done** |
| 3.2 | `pikslar-cli`: `info`, PNG and sheet, exit codes, the overwrite rule, the no-GUI check, failure tests. **Done** |
| 3.3 | PNG sequence; every export streamed a band at a time. **Done.** Frame and tag choice were done in 3.2. Scale, layer choice and the check of a scaled image's size moved to 3.3b |
| 3.3b | Export controls: scale, layer choice, the size check before any file. **Done** |
| 3.4 | Indexed PNG. **Done**: refused above 256 colours rather than written as RGBA with a note, as first planned, at the owner's request |
| 3.5 | GIF with an exact palette and full frames, then the ffmpeg checks. **Done** |
| 3.6 | GIF size: each frame draws only what changed, with disposal chosen per frame. **Done.** Local palettes and merged frames were considered and not done (section 3, GIF) |
| 3.7 | APNG: exact RGBA, each frame the rectangle that changed, SOURCE and NONE, exact delays. **Done** |
| 3.8 | Sheet columns, padding, array or hash JSON. **Done.** Loader fields checked; no loader library run |
| 3.9 | App export of every kind: a panel for what, the system's save dialog for where. **Done**, with D1 |
| 3.10 | ICO and CUR, if time allows |
| 5.6 | The panel, extended in M5: a frame range beside this frame, all frames and a tag, its first and last frame counted from 1 as the timeline shows them and kept in order and inside the frames there are, as `--frames` does. A range starts from the chosen tag's frames, or from the frame shown to the last. The panel stays on screen and above what it covers in the smallest window. **Done**, checked by `spikes/e2e-export-range.ps1` |

The shared path and the CLI come first, so every later format is tested headless from the start.
GIF comes before APNG because GIF has the most rules.

### Decisions

- **D1. Export never changes the project. Approved by the owner, 2026-11-12; built in 3.9.**
  Every format, in the app and the CLI, follows it:
  - Export shows the document exactly as it appears, floating pixels included, in their place,
    with their transparency, clipped at the canvas edge.
  - It does not anchor or move floating pixels, and leaves the document, the selection, undo and
    redo, and the modified state as they were. It never records a step or clears redo.
  - A cancelled or failed export leaves everything as it was.
  - It reads one unchanging snapshot of the document, so saving or editing at the same time
    cannot mix two states in one file.
  - The CLI exports only what the file holds. A project has no floating pixels unless the file
    records them.

  Built in 3.9. Tests in `crates/pikslar/src/export_tests.rs` and
  `crates/pikslar-core/src/shown_tests.rs` pin it for every kind, and `spikes/e2e-export.ps1`
  checks it in the real window.

## 7. Review of the first draft

| Severity | Finding | Answer |
|---|---|---|
| Critical | GIF full frames without disposal show the previous frame through transparency; the transparent index takes a palette entry | Disposal 2 from 3.5; from 3.6, disposal per frame, a frame clearing what the next does not show; 255 colors with transparency |
| Critical | APNG full frames blended OVER draw onto the frame before; chunk order and sequence numbers unstated | SOURCE and NONE; order and numbering stated |
| High | The app anchors floating pixels, so "export changes nothing" is false for the app | The exporter takes `&Document`; decision D1, approved |
| High | An exporter built on `&mut Editor` locks the GUI's state into every export | Done in 3.1: `flatten_document` and a `&Document` API |
| High | Memory was not bounded: the stress document is 400 frames, frame size dominates, scale was unbounded, sheets are whole images | Every image capped at 8192 a side before allocation; measured at that size |
| High | Several renames are not atomic; variable-width names mix; the sheet's two files can disagree | Stated per file; temporary files first; fixed-width names; stale files counted |
| High | Delays of 1 hundredth play at 100 ms; carrying error makes them | A floor of 2 hundredths, stated by the CLI |
| Medium | Loop counts differ by one between GIF and APNG | `--loop` is total plays, converted per format |
| Medium | One palette means flattening every frame twice | Stated |
| Medium | Automatic indexed PNG would change today's bytes | Opt-in; fixed palette order |
| Medium | Tag direction for the whole document; short ping-pong tags; encoders in the app | Rules stated; size reported per phase |
| Low | Existence is checked before writing, not at the rename; the temporary name is overwritten | Stated |
| Low | Reserved names need their own cases | Listed under Headless |
