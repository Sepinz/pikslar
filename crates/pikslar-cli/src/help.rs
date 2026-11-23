//! The help text. Kept with the code so it cannot drift from what the options do.

pub const MAIN: &str = "\
pikslar-cli: exports Pikslar projects without opening the program.

Usage:
  pikslar-cli info <project> [--json]
  pikslar-cli export <project> [options]
  pikslar-cli palette import <project> <palette file> [options]
  pikslar-cli palette export <project> [options]
  pikslar-cli batch <project> <operations file | -> [--out <file>] [--overwrite]
  pikslar-cli session --dir <folder>
  pikslar-cli help [info|export|palette|batch|session]
  pikslar-cli --version

Exit codes:
  0  done
  1  the arguments do not make sense
  2  the project, a palette file or a batch's operations could not be read
  3  the project cannot be exported, a palette loaded or an operation done, as asked
  4  the output could not be written, or exists and --overwrite was not given

Aseprite files (.ase, .aseprite) can be given wherever a project is: they are read
through the importer, and pikslar-cli never writes one. What the importer leaves
out or changes is printed to standard error, a line each starting \"warning:\",
and the exit code stays 0.
";

pub const INFO: &str = "\
pikslar-cli info <project> [--json]

Prints the size, layers, frame durations, frame rate (if timed by one), tags and
slices. Frames are counted from 1. --json prints one line of JSON instead: width,
height, fps (null when timed in milliseconds), durations in milliseconds, layers
from the bottom up, tags with their first and last frame, and slices with their
name, color and bounds, and pivot and center when set.

Aseprite files (.ase, .aseprite) can be given wherever a project is: they are read
through the importer, and pikslar-cli never writes one. What the importer leaves
out or changes is printed to standard error, a line each starting \"warning:\",
and the exit code stays 0.
";

pub const PALETTE: &str = "pikslar-cli palette import <project> <palette file> [options]
pikslar-cli palette export <project> [options]

Moves colours between a project (.piks) and a palette file: GIMP .gpl, JASC .pal
or .hex (six hex digits a line). The files have no alpha: colours read are opaque,
and a partly clear swatch is written as its colour alone, with a note. GIMP colour
names are not kept.

export writes the swatches in the order shown.
  --format gpl|pal|hex  The file format. Default: from --out's ending, else gpl.
  --out, -o <file>      Where to write. Default: the project's name with the
                        format's ending.
  --overwrite           Replace the file if it exists. Nothing is replaced without it.

import changes the project, or writes a new one with --out. No pixel ever moves to
another colour number. --how says what the file's colours do:
  append    (default) Join the end of the swatches. A colour the project already
            has keeps its number; one that is already a swatch stays where it is.
  replace   The swatches become exactly the file's colours, in its order. Colours
            the pictures still use stay in the project, so no pixel changes; a note
            says how many are no longer swatches. Groups lose members that are no
            longer swatches, and a group left empty goes.
  colors    A palette swap: swatch 1 takes the file's colour 1, and so on, every
            number kept, so the picture's colours change. The file must have as
            many colours as the project has swatches, or nothing is changed.
  --format gpl|pal|hex  The file's format. Default: from its ending, else its
                        first line.
  --out, -o <file>      Write this new .piks instead of changing the project.
  --overwrite           Replace --out if it exists.
Without --out the project is replaced, safely: it is written beside itself and
renamed over it. If opening it left something out, nothing is replaced: use --out.
If the project already matches, nothing is written. The path written is printed.
A file that cannot be read, or that has a line that is not a colour, is refused
with the line number and nothing is changed. Palette files are at most 4 MiB and
65,535 colours.
";

pub const EXPORT: &str = "\
pikslar-cli export <project> [options]

  --format png|sheet|sequence|gif|apng|tmj
                      png (the default) writes one frame. sheet writes every chosen
                      frame in one image, a row per tag, and a .json beside it
                      describing frames, durations and tags. sequence writes each
                      chosen frame to its own PNG, name_0001.png upward in the
                      order exported, and name.json listing each file with its
                      frame (counted from 0) and duration, written last. gif
                      writes the chosen frames as one GIF, each for its duration.
                      apng writes them as one animated PNG, every colour and
                      alpha as they are, each for its duration exactly.
                      tmj writes one frame's tilemap layers as a Tiled map (see
                      below) and an atlas PNG for each tileset beside it.
  --out, -o <file>    Where to write. Must end in .png, .gif for a GIF or .tmj
                      for a Tiled map. An APNG ends in .png.
                      Default: the project's name with that ending. A sheet's
                      .json takes the same name; a sequence's files are named
                      from it, and it is not written.
  --frame <n>         One frame, counted from 1. The default for png is frame 1.
  --frames <a>-<b>    Frames a to b, both included.
  --tag <name>        The frames of a tag. A GIF or APNG plays them in the
                      tag's direction: reverse plays backwards, ping-pong
                      forward then back without repeating the ends.
  --layer <name>      Draw only this layer, shown or hidden, with its own opacity
                      and blend mode. Give it again for more layers; they are
                      stacked as in the project. Default: the visible layers.
                      A name two layers share is refused.
  --scale <n>         Make each pixel n by n, nearest neighbor, after the
                      layers are blended. A whole number from 1, the default.
                      A frame may be at most 8192 a side once scaled.
  --indexed           Write indexed PNGs: each image gets a palette of exactly
                      its own colours, at 1, 2, 4 or 8 bits a pixel, and reads
                      back as the same pixels as without it. An image of more
                      than 256 colours is refused; no colour is ever changed.
                      The palette is in the project's palette order (the clear
                      entry first, then the colours the image uses, in swatch
                      order) when every colour used is a swatch; otherwise colours
                      not opaque come first, then the opaque, each by value.
                      Not for GIFs, which are always indexed, or APNGs.
  --loop <n>          GIF and APNG: how many times it plays. 0 is forever; up to
                      65536. Without it a tag plays as many times as it is set
                      to, and other frames forever. One frame is a still image.
  --columns <n>       Sheet: n frames a row, in frame order, instead of a row
                      per tag.
  --padding <n>       Sheet: n clear pixels between frames, as exported, and
                      none round the edge. 0, the default, is none.
  --json hash|array   Sheet: how the .json lists frames. array, the default, is
                      a list with each frame's filename; hash is keyed by name,
                      as Pixi reads it. Phaser reads both.
  --overwrite         Replace files that exist. Without it nothing is replaced.
                      A .json is replaced only if Pikslar wrote it. A sequence
                      is refused while files numbered past it are left from a
                      longer one.

Aseprite files (.ase, .aseprite) can be given wherever a project is: they are read
through the importer, and pikslar-cli never writes one. What the importer leaves
out or changes is printed to standard error, a line each starting \"warning:\",
and the exit code stays 0.

Paths written are printed one per line. Sizes, layers and frames are checked
before any file is made, and an export that cannot be made makes none. Every
file is written and checked before any is replaced. The old .json is removed
first and the new one is put in place last, so while it is there it describes
a whole export. If a replacement fails, nothing more is replaced and the
message names any file already replaced. The same project and options always
give the same bytes.

A GIF has one palette for all its frames, exactly their colours, in the project's
palette order when every colour is a swatch (the clear entry first) and in order of
value otherwise: more than 256 (255 if any pixel is clear) is refused, as is a pixel neither opaque nor clear,
which a GIF cannot show. Nothing is quantized, dithered or rounded to clear.
Each frame stores only what changed from the one before, and every frame shows
exactly its own pixels: a clear pixel shows nothing of the frame before.
Durations become hundredths of a second, each running total rounded so the
total stays right, whether timed in milliseconds or by frame rate. A frame that
would round to under 20 ms plays for 20 ms, since most players show shorter as
100 ms, and a note says how many did.

An APNG keeps every pixel as the PNG export of its frame has it, partial alpha
and all, and each frame replaces the one before, so a clear pixel shows nothing
of it. Its first frame is the image a viewer without APNG shows. Durations are
kept exactly, in milliseconds or in ticks of the frame rate; some browsers show
a frame of 10 ms as 100 ms, and a note says how many there are.

A TMJ is a Tiled map (Tiled's JSON format) of one frame, --frame or frame 1:
orthogonal, drawn right-down, its tilesets inside it. Each tileset a tilemap
layer uses is written as name-tileset1.png upward, in the order of the tileset
list: its tiles in panel order, left to right, ceil(sqrt(tiles)) a row, with no
margin or spacing, every colour as RGBA. Every tilemap layer is a tile layer,
shown or hidden, with its name, opacity and, where Tiled has it, blend mode
(Addition and Subtract are written as normal, with a note); groups are group
layers. A cel offset of whole tiles moves the cells; the rest is a pixel
offset. Flips and turns are Tiled's flip bits. Paint and reference layers are
left out, with a note. All tilemap layers must have tiles of one size, or the
export is refused. --layer, --scale, --indexed, --frames and --tag are not
for tmj. Tiled shows whole tiles at the right and bottom where the canvas is
not a whole number of tiles; the PNG export stops at the canvas edge.
";

pub const BATCH: &str = "\
pikslar-cli batch <project> <operations file | -> [--out <file>] [--overwrite]

Runs scripting API version 1 operations on a project as one transaction, and saves
it, or writes a new project with --out. The operations file (- reads standard
input) is one JSON object of at most 1 MiB:

  {\"api\": 1, \"ops\": [{\"op\": \"add_frame\", \"at\": 1}, ...]}

Frames are named by handle, not place. A layer or frame field may instead refer to
an earlier operation's result: {\"result\": 2} is what operation 2 made, and
{\"result\": 0, \"place\": 1} is the frame at place 1 of operation 0's document.

Every operation is read before any runs. If the file is malformed (exit 2) or any
operation is refused (exit 3, naming it, counted from 0), nothing is written. A
batch that changes nothing writes nothing unless --out is given. Aseprite files,
projects an older version saved and projects that opened with something left out
are only written with --out. On success one line of JSON is printed:
{\"results\": [...], \"changed\": true|false, \"written\": \"<path>\"|null}.
See docs/cli-json-lines.md for the operations and the JSON rules.
";

pub const SESSION: &str = "\
pikslar-cli session --dir <folder>

A JSON lines session for a tool you launch: one request object a line on standard
input, one reply a line on standard output, in order, until standard input ends.
Requests are hello, open, run, save and close; hello must come first. Each run is
one transaction: all its operations are done, as one undo step, or none are.
Nothing is written until a save. open and save take paths relative to --dir and
can never reach outside it. A line that ends without a line break is not carried
out. See docs/cli-json-lines.md.
";

/// The help for a command, or the general help for anything else.
pub fn for_command(command: Option<&str>) -> &'static str {
    match command {
        Some("info") => INFO,
        Some("export") => EXPORT,
        Some("palette") => PALETTE,
        Some("batch") => BATCH,
        Some("session") => SESSION,
        _ => MAIN,
    }
}
