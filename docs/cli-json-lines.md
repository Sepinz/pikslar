# Command line batches and JSON lines

Phase 12.3. `pikslar-cli` gains two commands that edit projects through the scripting API (`docs/scripting-api.md`). They are for tools the person launches, such as build scripts, editors and asset pipelines. Neither has any editing path of its own: each operation goes to `pikslar-api`, which turns it into the editor's commands.

| Command | Use |
|---|---|
| `pikslar-cli batch <project> <ops file or -> [--out <file>] [--overwrite]` | One list of operations, applied once, then saved |
| `pikslar-cli session --dir <folder>` | A conversation: open, run several times, save, close |

## Transactions

Every list of operations is **one transaction**. For a batch that is the whole file. For a session it is one `run` request.

- **Success.** All of its operations are done, as one undo step.
- **Failure.** If any operation is refused, the document is exactly as it was before the list, and the reply names the operation that failed, counted from 0. Visibility changes in the same list are taken back too, because the run works on a copy.
- **Order.** All operations are read and checked for shape before any of them runs, references to earlier results included. A misspelt field in operation 900 means operations 0 to 899 never run either.
- **No partial writes.** Nothing reaches a file except through an explicit save: the end of a batch, or a `save` request. A save writes a temporary file beside the target and renames it into place. Either the old file or the new one is there, never a mix.

There are no `begin`, `commit` or `rollback` requests. A transaction never spans lines, so a tool that stops, or a stream that breaks, can never leave one open.

## The JSON

Both commands use one strict reader (`crates/pikslar-cli/src/json`). It is written for this purpose, and no third-party crate is used.

- **Text.** UTF-8 only. Invalid UTF-8 and a byte order mark are refused.
- **Numbers.** Whole numbers that fit in an `i64`. Fractions, exponents (`1e2`, `1e400`), `-0`, leading zeros, `NaN` and `Infinity` are refused. Nothing is rounded or clamped. The API then checks every number against its own range.
- **Strings.** Raw control characters are refused. A `\u` surrogate escape must be a correct high and low pair. A lone surrogate is refused.
- **Objects.** A duplicate key is refused. So is any field an operation or request does not define, so a misspelt optional field is never silently ignored. A `null` value counts the same as a field that is left out.
- **Trailing text.** Anything after the value, other than whitespace, is refused.
- **Limits.** All limits are checked while reading, so no input can use deep recursion or much memory:
  - at most 8 levels of nested arrays and objects;
  - at most 4,096 characters in a string;
  - at most 10,000 items in an array;
  - at most 32 members in an object;
  - at most 10,000 operations in a list;
  - at most 1 MiB in a request line or a batch file.

Replies are compact JSON on one line. Control characters, DEL, U+2028 and U+2029 are escaped, so a reply never contains any kind of line break.

## Operations

An operation is an object with `"op"` and that operation's fields. The names match `pikslar_api::Op`.

| `op` | Fields (optional fields in brackets) |
|---|---|
| `version`, `document`, `swatches` | none |
| `layer` | `layer` |
| `pixel` | `layer`, `frame`, `x`, `y` |
| `add_layer` | [`above`], [`mode`: `"rgba"` (default) or `"palette"`], [`name`] |
| `remove_layer`, `duplicate_layer` | `layer` |
| `rename_layer` | `layer`, `name` |
| `move_layer` | `layer`, `to` |
| `set_opacity` | `layer`, `opacity` |
| `set_visible` | `layer`, `visible` (true or false) |
| `set_blend` | `layer`, `blend` (the panel's name, such as `"Multiply"`) |
| `add_frame` | `at`, [`copy`: false] |
| `remove_frame` | `frame` |
| `set_duration` | `frame`, `ms` |
| `add_tag` | `name`, `from`, `to` |
| `set_pixel`, `fill` | `layer`, `frame`, `x`, `y`, `color` |
| `erase_pixel` | `layer`, `frame`, `x`, `y` |
| `draw_line` | `layer`, `frame`, `from`: `[x, y]`, `to`: `[x, y]`, `color` |
| `add_swatch` | `color` |
| `recolor` | `swatch`, `color` |
| `remove_swatch` | `swatch` |

How things are named:

- **Layers.** By persisted id. Never given to another layer while the project is open.
- **Swatches.** By color id.
- **Frames.** By handle (`docs/stable-ids.md`). `document` lists them in order, and `add_frame` gives back the new one's. A handle follows its frame through inserts and moves, and is refused with `no_such_frame` once the frame is removed. Handles are not saved: each open gives new ones. `add_frame`'s `at` and `move_layer`'s `to` are places, not references.
- **Colors.** Straight-alpha `0xAABBGGRR`, written as a decimal number. For example, opaque red is `4278190335`.

What each operation gives back:

- `null` when it is simply done;
- `{"version": n}`, `{"color": c}`, or `{"id": n}` for the layer or swatch it made;
- `{"frame": handle}` for `add_frame`;
- `{"swatches": [{"id", "color"}, ...]}`;
- `{"width", "height", "durations", "frames", "layers"}` for `document`, with `frames` the handles in order;
- `{"id", "index", "name", "kind", "mode", "visible", "locked", "opacity", "blend"}` for `layer`.

### Referring to an earlier result

A list is fixed before it runs, so it cannot know the handle of a frame that it makes, or of a frame already in the project. Instead, a field that wants a layer id or a frame handle may hold a reference:

| Reference | Means | Allowed when operation `n` is |
|---|---|---|
| `{"result": n}` | The id or handle operation `n` made | `add_layer` or `duplicate_layer`, for a layer field; `add_frame`, for a frame field |
| `{"result": n, "place": k}` | The frame or layer at place `k` (from 0, layers from the bottom) of the document operation `n` gave back, as it was then | `document` |

The fields that take references are `layer` and `above` (layers), and `frame`, and `add_tag`'s `from` and `to` (frames). No other field does: places (`at`, `to` of `move_layer`), coordinates, colors and swatch ids are numbers only.

```json
{"api":1,"ops":[
  {"op":"document"},
  {"op":"add_frame","at":1,"copy":true},
  {"op":"set_pixel","layer":1,"frame":{"result":1},"x":3,"y":2,"color":4278190335},
  {"op":"add_tag","name":"walk","from":{"result":0,"place":0},"to":{"result":1}}
]}
```

The rules are strict, and every one is checked before any operation runs:

- `n` must name an **earlier** operation in the same list. A reference to itself, to a later operation or past the end is refused.
- What operation `n` gives must be what the field wants. A layer for a frame field, a frame for a layer field, a swatch, or an operation that gives nothing is refused. A `document` result needs `place`, and any other result takes none.
- `n` and `k` are whole numbers from 0. The object has only those members.

Shape errors (`{"result": -1}`, `{"result": "0"}`, a missing `result`, another member, a reference in a field that takes none) are `bad_request`. A reference that cannot fit is `bad_result`. A `place` past the end of the document's list can only be found as the list runs: it is `bad_result` too, and the whole list is taken back like any other refusal. Either way nothing is written, and the error names the operation.

A session's `run` cannot refer to another request's results. A client that keeps a handle between requests can use it directly, as long as the project stays open.

`crates/pikslar-cli/src/api_json.rs` is the only file that names `Op` variants. When the API's operations change, that file and its tests are the only ones to update.

### How much one run may do

A run, in a batch or in one session request, has two budgets, charged before each operation:

- **Reply cells**, at most 1,048,576 (2^20): the pixel colors it gives back. `pixels` costs its area.
- **Pixel work**, at most 268,435,456 (2^28): the pixels its operations touch. `pixels` and `fill_rect` cost their area, `put_pixels` its color count, `fill` the whole canvas, `draw_line` its longer side plus one, and any other operation 1.

An operation that would go over either budget is refused with `too_large`, with `at` naming it, and the whole run is taken back like any other refusal. `hello` gives both as `limits.reply_cells` and `limits.pixel_work`.

## batch

The file is one object of at most 1 MiB: `{"api": 1, "ops": [...]}`. A file of `-` means standard input.

Exit codes:

- **0** done;
- **1** bad arguments;
- **2** the project or the operations could not be read, including any malformed JSON or operation;
- **3** an operation was refused, or the API version is not 1;
- **4** the output could not be written.

On any failure nothing is written, and standard error names the problem: `operation 2 (out_of_range): opacity must be from 0 to 255, not 300`.

On success, one line is printed:

```json
{"results":[null,{"id":3}],"changed":true,"written":"walk.piks"}
```

Where the result goes:

- **Saved in place.** Without `--out`, the project is saved over itself, and only when something changed. Aseprite files, projects saved by an older version, and projects that opened with something left out are refused in place. Give `--out` for those.
- **A new file.** With `--out`, a new `.piks` file is written, checked like any other output. `--overwrite` is needed to replace an existing file. The project itself, links, folders and read-only files are refused.

## session

The session reads one request per line on standard input and writes one reply per line on standard output, in order. Line ends may be `\n` or `\r\n`. Warnings from opening a project go to standard error. The session ends with exit code 0 when standard input ends. If standard output cannot be written, it ends with exit code 4.

Every request has an `id`, either a whole number or a string of up to 256 characters. The `id` is given back in the reply.

```json
{"id":1,"ok":true,"result":{...}}
{"id":2,"ok":false,"error":{"code":"no_such_layer","message":"no layer has id 9","at":0}}
```

If a line cannot be read at all, the reply has `"id": null`: bad JSON, not an object, a missing or bad `id`, or a line that is too long. Every line gets exactly one reply, and an error never ends the session.

| Request | Fields | Result |
|---|---|---|
| `hello` | `api` | `{"api", "protocol", "program", "limits"}`. Must come before any other request. Any `api` but 1 is refused with `version` |
| `open` | `path` | `{"document": {...}, "can_save_over"}`. One project at a time |
| `run` | `ops` | `{"results": [...], "changed"}`. One transaction |
| `save` | [`path`], [`overwrite`] | `{"saved": true}`. With no path, saves over the open file, unless that is refused for the reasons a batch refuses it. With a path, writes a new `.piks` file, which becomes the open file. An existing file needs `"overwrite": true` |
| `close` | [`discard`] | `null`. Refused with `unsaved` if there are changes, unless `"discard": true` |

Error codes:

- **Reading the line:** `bad_json`, `bad_request`, `line_too_long`, `unended`.
- **Requests:** `hello_first`, `version`, `unknown_type`, `already_open`, `not_open`, `unsaved`.
- **Files:** `bad_path`, `outside`, `not_found`, `unreadable`, `exists`, `bad_target`, `cannot_save_over`, `write_failed`.
- **Operations:** `unknown_op`, `too_many_ops`, `too_large`, `bad_result`, and the API's own codes: `no_such_layer`, `no_such_frame`, `no_such_swatch`, `out_of_range`, `outside`, `transparent`, `bad_name`, `unknown_blend`, `refused`, `cancelled`.

A line that ends without a line break, because the input ended inside it, is **not carried out**. It may be a request that was cut short. It gets an `unended` reply, and the session ends. A document still open at the end is dropped unsaved.

### What a session may touch

Opening and saving files is a capability, so a session gets only the folder the person names on the command line with `--dir`. A request's `path` must be relative and made only of plain names.

These are refused before any file is touched:

- drives, roots, shares and `\\?\` paths;
- `.` and `..`;
- empty paths;
- names that Windows refuses or changes: devices (`con`, `prn`, `aux`, `nul`, `conin$`, `conout$`, and `com` or `lpt` with 0 to 9 or a superscript one to three, whatever extension follows), `:` streams, trailing dots and spaces, and control characters;
- names holding a character that would hide or reorder them (direction overrides, zero-width marks and the rest of the plugin crate's list, `pikslar_plugin::hidden`), refused with `bad_path`.

Then the path is resolved with every link and junction followed. The file to open, or the folder to save into, must still be inside the folder. A save target that is a link is refused. A path through a link or junction whose target is gone is refused with `outside`, the same as one whose target exists outside, so a reply never tells where a link leads.

Error messages name a path only as the request gave it. They never hold the session folder, an absolute or `\\?\` path, or the user's name.

The session has no other file access, and it has no network access.

Known limits:

- **Timing.** The check and the write are separate steps. Something else that changes links inside the folder between them could redirect a save. The person controls the folder, and saves rename a temporary file into place, so a hard link is replaced, not written through.
- **Hard links.** A hard link inside the folder to a file outside it can be opened, because hard links cannot be told apart from ordinary files.

## Evidence

Tests are in `crates/pikslar-cli`.

- `src/json/tests.rs` (8 tests): the reader's grammar, the refused numbers, escapes and surrogates, duplicate keys, the BOM and bad UTF-8, every limit at and past its edge (including a million levels of nesting, without overflowing the stack), and writing then reading back.
- `src/api_json_tests.rs` (5 tests): each of the 24 operations read by its name (the test fails to build if an operation is added without a row), defaults, 17 malformed operations, the shape of every reply, and every API error code.
- `src/results_tests.rs` (4 tests): references to `add_frame`, `add_layer`, `duplicate_layer` and `document` results giving the same saved bytes as the API calls with the ids written out; 9 references refused before anything runs (forward, self, out of range, four wrong types, a document without a place, a place on one id); 10 malformed references and references in fields that take none; a place past a document's end failing the run with the document and its revision unchanged.
- `src/session.rs` and `src/session_tests.rs` (11 tests):
  - line splitting with bounded memory;
  - the handshake;
  - a session's saved bytes equal to the same `pikslar_api::run_ops` calls;
  - each of the 24 operations through a session giving the API's own result or error code, with frames named through a `document` result;
  - rollback on a refused run;
  - the save, close and open rules;
  - 18 paths that try to leave the folder;
  - 49 malformed lines (truncated, overlong, deeply nested, wrong types, control characters, NaN, Infinity, -0, 1e400, lone surrogates, a BOM, bad UTF-8, duplicate keys, trailing text, unknown fields, 10,001 operations), each with one error reply, the session going on and nothing changed;
  - over 1,000 sessions of one-byte mutations of valid requests, none crashing or writing;
  - input ending mid-line;
  - a closed output.
- `tests/batch.rs` (9 tests):
  - a batch saved byte for byte equal to the same API calls;
  - standard input and saving in place;
  - six late refusals that write nothing, with and without `--out`;
  - six references that cannot be resolved (forward, self, out of range, two wrong types, a place past the end) writing nothing, with and without `--out`;
  - 20 malformed operation files, including over 1 MiB and 100,000 levels of nesting;
  - another API version;
  - a batch that changes nothing and writes nothing;
  - target checks;
  - Aseprite files, which are only read.
- `tests/session.rs` (3 tests): a session over the real standard streams, input ending mid-line, and `--dir` checks with help.

## Open questions

- **Undo.** A session has no `undo` request. Each run is one undo step in memory, but nothing can reach it yet. Add one if tools ask for it.
- **Colors.** Colors are decimal numbers only. A `"#RRGGBBAA"` string form would be friendlier, but it would add a second spelling.
- **Performance.** Program size and the cost of very large runs were not measured in this phase.
