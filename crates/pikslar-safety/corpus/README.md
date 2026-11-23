# Frozen project files

These files were written by the real program at old commits and are never regenerated. If one
of them stops opening, or stops looking the same, the code is wrong, not the file.

| Files | Format | Written by |
|---|---|---|
| `v1-*.piks` | 1, no sections | commit `aaebbbc` |
| `v2-*.piks` | 2, tagged sections | commit `d29dde4` (RC2) |
| `v2-recovery-*.piks` | 2, with floating pixels | commit `d29dde4` (RC2) |
| `v3-*.piks` | 3, bounded images and cel records | the first build to save version 3 (phase 2.6), from the `v2-*` files, each checked to look the same |
| `v3-numbered.piks` | 3, saved layer numbers 7, 3 and 12 | the same build, from `v2-animated.piks` |
| `v3-offsets.piks` | 3, cels moved partly off the canvas on every side, pixels kept past the edge, a hold moved together and a hold whose frames sit apart | the first build to save offsets (phase 2.4b), drawn and moved through its own commands |
| `v3-layers.piks` | 3, a resized canvas, every blend mode, opacities, a lock, a renamed, a duplicated and a merged layer | commit `87313b9` (M6), through its own commands, by `tests/corpus_writer.rs` |
| `v3-reference.piks` | 3, a reference layer held on every frame, pixels past the canvas, required feature bit 0 | the same |
| `v3-groups.piks` | 3, groups two deep, a locked, a hidden and a folded group, a reference in a group, required feature bits 0 and 1 | the same |
| `v3-rate-slices.piks` | 3, timed at 24 frames a second (a Rate section, frames of two, one and six ticks), and two slices, one with a pivot and a centre | the first build to save a rate and slices (phase 7), through its own commands, by `tests/corpus_writer.rs` |
| `v3-tilemap.piks` | 3, required feature bit 2: three tilesets made and one removed (counter 4), a removed tile (ids 1 and 3 of 8 by 8 tiles, next id 4), two hidden tilemap layers (map cels of every turn, a map of 8 by 4 tiles with flips) over a painted layer | the first build to save tilesets (phase 10.4), through core operations, by `tests/corpus_writer.rs`. An M7 build (`studio-m7`) refuses it as made by a newer version, unchanged |
| `v3-aseprite-blending.piks` | 3, required feature bit 3: a document set to Aseprite-compatible blending, four layers (Normal, Hue, Color Burn at opacity 170, Soft Light) over two frames | the first build with the blending setting, through core operations, by `tests/corpus_writer.rs`. A build before it refuses it as made by a newer version |

`manifest.txt` has one line per file:

```
name version width height layers frames tags floating frame-hashes
```

Each hash is FNV-1a over the little-endian bytes of the composited frame, with any floating
pixels applied to the preview, taken from the program that wrote the file. A new format version adds new files here. Old files are never removed.
