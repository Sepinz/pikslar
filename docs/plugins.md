# Plugins

Roadmap phase 12.6. The plugin model is in `crates/pikslar-plugin`: the manifest, the package, install, upgrade, uninstall, integrity checks (detecting accidental or unexpected changes to installed files) and file handles. The crate is plain Rust with no interface, and it never runs a script. The app's side (the Scripts menu, the install review, running commands) is in `crates/pikslar/src/plugin_ui.rs`, described under [In the app](#in-the-app). Running a command means handing its script source and the granted capabilities to `pikslar-bind`, which runs it in the script host (`crates/pikslar-script`) against the API (`crates/pikslar-api`).

## Format

A plugin is **one flat folder**:

```
outline/
  plugin.json
  outline.pks
  thin.pks
```

**Why a folder and not an archive.** A `.zip` would need an archive parser in the shipped program. Pikslar ships no third-party code, and a hand-written unzipper adds decompression, central directories, name encodings and zip bombs to the attack surface. Windows already unpacks zips, so a plugin is published as a zip and the user extracts it. Pikslar then reads a folder of at most 16 small text files. It reads that folder into memory once, and the review, the hash and the install all work from those bytes.

Rules for the folder (all enforced, `src/package.rs`):

- It contains only `plugin.json` and the `.pks` scripts that the commands run. A script that no command runs is refused, and so is any other file, any folder, and any link or junction. The folder itself must not be a link or junction either.
- Script names are `[a-z][a-z0-9_-]{0,31}.pks`, lower case only. This is a whitelist, so the following can never appear: path separators, `..`, drive letters, UNC prefixes, `:` (alternate data streams), trailing dots or spaces, and two names that differ only by case. Windows device names are refused as stems: `con`, `prn`, `aux`, `nul`, `conin$`, `conout$`, and `com` or `lpt` followed by 0 to 9 or by the superscripts one to three (`com0`, `com¹`, `lpt³`). The export name check (`pikslar-export` `targets.rs`) refuses the same list.
- At most 16 files. Each script is at most 256 KiB, `plugin.json` at most 16 KiB, and all files together at most 1 MiB. Scripts must be UTF-8.
- Files are opened without following reparse points (`FILE_FLAG_OPEN_REPARSE_POINT`). The type and size checks are made on the opened handle, and reads stop one byte past the limit. A file whose reparse point redirects (see "Links" below) is refused; a cloud placeholder is read as the file it is.

### `plugin.json`

This uses the strict JSON reader in `crates/pikslar-json` (moved out of `pikslar-cli` so both can use it). It refuses duplicate keys, a byte order mark, trailing data, fractions and oversized or deep values. On top of that, the manifest reader refuses unknown keys, missing keys and wrong kinds.

```json
{
  "manifest": 1,
  "id": "com.example.outline",
  "name": "Outline",
  "version": "1.2.0",
  "api": 1,
  "capabilities": ["read-document", "edit-document"],
  "commands": [
    {"id": "add-outline", "title": "Add outline", "script": "outline.pks"}
  ],
  "description": "Draws a one-pixel outline around the selection.",
  "author": "Example Studio"
}
```

| Key | Rule |
|---|---|
| `manifest` | `1`. Any other value is refused. |
| `id` | 2 to 6 dot-separated words of `[a-z][a-z0-9-]*` (no trailing hyphen), each at most 32 characters, 64 in all. The first word may not be a device name. It names the install folder. |
| `name` | 1 to 48 characters of display text (see below). |
| `version` | `major.minor.patch`, each part 0 to 99999 with no leading zeros. There are no pre-release or build suffixes. Parts are compared as numbers. |
| `api` | The scripting API version (`pikslar_api::API_VERSION`, now 1). Any other value is refused when the plugin is read, so an install never shows a plugin that could not run. |
| `capabilities` | A list, possibly empty, of known capability words, each at most once. An unknown word refuses the install. |
| `commands` | 1 to 16 entries of `{id, title, script}`. `id` is one word as in `id`. `title` is display text of at most 48 characters. `script` is a script name that exists in the folder. Ids and titles are unique. |
| `description` | Optional display text of at most 400 characters. |
| `author` | Optional display text of at most 64 characters. It is shown as stated and never checked. |

**Display text** must be non-empty and have no leading or trailing space. It may hold no control characters, and none of the invisible formatting characters that can hide or reorder what the user reads. The one list is `pikslar_plugin::hidden`: every Default_Ignorable code point (direction marks, overrides and isolates, zero-width marks, the byte order mark, Hangul and Braille blank fillers, variation selectors including the Mongolian ones, tag characters), every other format character (the Arabic, Kaithi, Egyptian and shorthand format controls among them), U+FFFC, private-use characters, noncharacters and controls. Spaces other than U+0020 and more than two combining marks in a row are refused too. The app runs the same list over everything a plugin or script puts on screen (its printed output, its errors, the disabled reason, stray folder names in the menu) and drops what it catches, and a CLI session refuses file names that hold any of it.

## Capabilities

The set is small, closed and defined in `src/capability.rs`. These words are part of the format.

| Word | Shown at install | Lets the plugin |
|---|---|---|
| `read-document` | See the open document: its frames, layers, pixels and palette. | Call the API's reading operations |
| `edit-document` | Change the open document. Each run can be undone in one step. | Call the API's editing operations, in one transaction per run |
| `pick-file-to-read` | Ask you to choose a file, then read that one file. It never sees where the file is. | Ask the host for an open dialog, then read through a handle |
| `pick-file-to-write` | Ask you to choose where to save, then create or replace the one file you pick, if it is plain data or a picture (like .txt or .png). It never sees where. | Ask the host for a save dialog, then write through a handle |

A plugin with no capabilities can compute, but it cannot see or change anything.

**Mapping to the run-time capability set.** The binding gives each run a capability set that decides which host functions are put in the script's `HostTable`. `impl From<CapabilitySet> for pikslar_bind::Capabilities` (`crates/pikslar-bind/src/caps.rs`) turns `LoadedPlugin::granted()` into that run's set, one for one, by word:

| Manifest word | Binding | Wired |
|---|---|---|
| `read-document` | `Capability::Read` | Yes |
| `edit-document` | `Capability::Edit` | Yes |
| `pick-file-to-read` | `Capability::PickFileToRead` | Yes |
| `pick-file-to-write` | `Capability::PickFileToWrite` | Yes |

A script run outside a plugin (the Scripts panel, `--run-script`) gets read and edit, and never the pickers: `Capabilities::ALL` holds no picker, and only a plugin's run is given a `FileAccess`.

## Install flow

`PluginStore` (`src/store.rs`) is the only place plugins are installed.

1. **Where.** `PluginStore::for_settings_dir(settings_dir)` puts plugins in `<settings>/plugins`. The app wires it from the same folder as `SettingsFile`: `--settings-dir DIR` gives `DIR/plugins`, the default gives `%LOCALAPPDATA%\Pikslar\plugins`, and `--no-settings` gives `None`, which turns plugins off. When plugins are off, every review returns `PluginError::NoStore` and the list is empty. The app makes its store this way when it loads the settings (`settings_ui.rs`).
2. **Review.** `store.review(folder)` reads and checks the package and returns a `PendingInstall`. It writes nothing and runs nothing. `pending.review()` is the `InstallReview` the dialog shows: name, version, id, API, author (marked as unverified), the plugin's own description (labelled as such), each capability in plain words, the commands it adds, the file count and size, and the SHA-256 of the files. `lines()` gives it as plain text in the order the dialog shows it: name and version, the id (ASCII only) and API, every capability, the commands, the fixed statement that the plugin cannot open files or folders by itself or use the network, what the install replaces, "Author: *author* (unverified: as the plugin states it)" or "Author: not stated", "Fingerprint (SHA-256 of its files): *eight groups of eight hex digits*", the fixed statement of what the fingerprint means (`FINGERPRINT_MEANS`: it detects accidental or unexpected changes to these files; it does not prove who made the plugin, or stop a program that can change your files), and the plugin's own description last, so a long description can never push a capability out of sight.
3. **Decision.** `store.approve(pending)` installs. `pending.deny()`, or simply dropping it, leaves everything as it was. Outside the crate a `PendingInstall` can only come from `review`, so nothing can be installed without one. Approval writes the bytes that were reviewed, even if the source folder changed afterwards. It is refused (`ChangedSinceReview`) if the installed copy of that id changed since the review, or if the pending install was reviewed for another store. The snapshot of the installed record is read with `open_no_follow` and checked on the opened handle, so a record swapped for a link, junction, folder or device after the review refuses the approval and is never read through.
4. **Writing.** The package is written to `plugins/.new-<id>/`, with the record `installed.json` last. Any installed copy is renamed to `.old-<id>`, the new folder is renamed to `<id>`, and the old copy is removed. Folders whose names start with a dot are never listed or loaded, and the next install or uninstall of that id clears them. If Pikslar crashes during an install, the result is the old plugin, the new one, or no plugin. It is never a mix, because a folder without its record, or one with a record that does not match its files, is disabled.

**Upgrade rules** (`Change`):

| Installed | Offered | Result |
|---|---|---|
| Nothing | Any version | `Fresh` |
| Older version | Newer version | `Upgrade`. Capabilities the old version lacked are marked "new in this version". If the name or author differs from the installed plugin, the review shows a warning that this replaces a different plugin that had the same id. Both can be copied exactly, and the author is not checked, so a plugin that copies them gets only the ordinary "This replaces version X" line and the "new in this version" marks. |
| Same version, same files | | `Reinstall`. This re-approves and changes nothing. |
| Same version, other files | | Refused (`SameVersionChanged`). A changed release needs a new version number. |
| Newer version | Older version | Refused (`Downgrade`). Uninstall first to go back. |
| A disabled copy | Any version | `Repair`, with the reason the copy is disabled |

**Loading.** `store.load(id)` returns a `LoadedPlugin` only if all of these hold:

- The folder holds exactly the recorded files and the record, with no extras and no links.
- The content hash matches the one recorded at install.
- The manifest still parses and its API is still supported.
- The id, version, capabilities and file list all agree with the record.

Anything else returns a `Disabled` reason in plain words, for example "its files changed after it was installed; review and install it again to use it". `store.list()` shows every plugin folder as enabled or disabled. A plugin is never loaded in part. `LoadedPlugin` gives the manifest, the granted capabilities and `script_for(command_id)`.

**Uninstall.** `store.uninstall(id)` renames the folder to `.del-<id>`, so the plugin stops loading at once, then removes it and any leftover staging folders. Nothing of the plugin remains in the plugins folder. For a valid id whose folder is gone, uninstall still clears the `.new-`, `.old-` and `.del-` folders an interrupted install or uninstall left, and says the plugin is not installed only when there were none. Removal never follows a junction inside the folder.

**Content hash.** It is an integrity check, not a signature: it detects accidental or unexpected changes to the installed files, and says nothing about who made them. It is SHA-256 (`src/sha256.rs`, written here and tested against the FIPS 180-4 vectors). The input is a domain tag followed by, for each file in name order, the length of its name, the name, the length of its contents and the contents. Because of this framing, moving bytes between files or renaming a file always changes the hash.

## Files only through the user's pickers

Plugins get **no path capability**. There is no capability, host function or manifest key that takes or returns a path. A plugin cannot choose the folder a dialog opens in, or a file name to save as.

The host functions (`src/files.rs` holds the grants; `crates/pikslar-bind/src/files.rs` binds them):

```
pick_file_to_read(title, ["png", "gpl"])  -> handle or nil      needs pick-file-to-read   wired
read_file(handle)                         -> text (UTF-8)       needs pick-file-to-read   wired
file_name(handle)                         -> "palette.gpl"      needs pick-file-to-read   wired
pick_file_to_write(title, ["txt"])        -> handle or nil      needs pick-file-to-write  wired
write_file(handle, text)                  -> nil                needs pick-file-to-write  wired
```

Each word grants only its own functions (`functions.rs`, `each_capability_alone_grants_exactly_its_own_functions`): a plugin with `pick-file-to-write` alone cannot read the document, change it, or read a file, and `file_name` needs `pick-file-to-read`.

`read_file` gives text only: a file that is not UTF-8 is refused, and the message does not name it. It costs one read plus one unit of fuel per 64 bytes. A handle is read once: reading it again gives the same text at the cost of one read, with no second copy. The read is made a megabyte at a time, each piece paid for before it is read, so Stop (the cancel flag, read as fuel is burnt) ends it between pieces. **Known limitation: Stop cannot end the open.** The cancel flag is read between pieces, not while the file is being opened. `read_file` opens the file to find its length and again to read it, and each `File::open` is a blocking system call with no cancel. If the file is on an offline share, a disconnected drive or a cloud placeholder that must first be fetched, Stop is not seen until the call returns, which can be as long as the system waits (seconds to minutes) and during which the run, and the window, wait for it. Nothing is read or changed meanwhile, and the run is stopped and taken back as soon as the call returns. An asynchronous open was not built. A pick costs as much as an edit. A run without a `FileAccess` (anything but a plugin) refuses all of them with "this run has no way to ask for files".

### Writing a file

`write_file(handle, text)` writes text only (a script string is always UTF-8). It costs an edit plus one unit of fuel per 64 bytes, paid when it is called.

- **One write per handle.** A second `write_file` on the same handle is refused ("written once"), so the overwrite question the dialog asked covers exactly one write. A read handle cannot write and a write handle cannot read. The 8 picks per run are shared by reads and writes.
- **Size.** At most 64 MiB, checked from the string's length before anything is copied. The text is not copied at all: the script's own string is kept until the end of the run, so it stays counted by the run's memory meter.
- **Only plain data and picture kinds.** `pick_file_to_write` offers only kinds on the allow-list `files::ALLOWED_EXTENSIONS` (`src/kinds.rs`): `txt md csv tsv json gpl hex pal act ase png gif bmp piks aseprite`. Anything else (programs, scripts of any language, shortcuts, settings and remote-desktop files, themes, installers, certificates, `svg`, `html`, `ini` and the rest) is refused, as an allow-list is the only kind of check that does not need every dangerous kind to be thought of; reading such a file is allowed. As the user can type any name, the final name is checked again at write time, by its last extension, case-folded, trailing dots and spaces dropped (so `a.pyw.` is `a.pyw`, refused); for an existing target its real name (`GetFinalPathNameByHandleW`, so not an 8.3 short name standing for it) is checked too. A name with no extension is allowed: Windows runs nothing by opening one. What this does not claim: a `.json`, `.txt` or `.md` file is still text that another program the user has installed may interpret in its own way, and a picture or palette file may be read by programs with their own flaws; the guarantee is that this PC does not run or load what is written as code when it is opened. A `.piks` file is allowed: it is a document, and a plugin that may write a file may write a picture.
- **Never in a Startup folder.** The user's and the common Startup folders (`FOLDERID_Startup`, `FOLDERID_CommonStartup`, asked of `SHGetKnownFolderPath` at write time, `src/startup.rs`) are refused like Pikslar's own, in and under, whatever the kind.
- **Never in Pikslar's own folders.** The app gives each plugin run the folders it keeps its own files in: the settings folder in use, the plugins folder and the per-user `%LOCALAPPDATA%\Pikslar` (which holds crash recovery). A write whose folder is in or under one of them is refused. Both sides are compared by where they really are: the held folder's final path from its handle, each protected folder through `fs::canonicalize`, case-folded. So a junction higher up the chosen path that leads into one is refused too.
- **Deferred to the end of the run.** Nothing is written when `write_file` is called. The writes are made when the script has returned, its last waiting pixels are written and the growth check has passed, inside the run's transaction, just before its undo step is kept. A run that fails, is stopped by a limit or is cancelled writes no file, and a write that fails takes the whole run back: the document and every file are as they were, and the panel says "*name* changed nothing. The file it was saving was not written: *why*". The writes are the last thing the run does, and keeping its step afterwards can no longer fail in any way we know of. It is not proved: `Editor::transaction` still has error branches after the run (a stroke left open, the step's last command, taking the result on) that no script is known to reach. If one ever were reached, the files would be on disk while the document is rolled back; the run's `Outcome::files_written` says so, and the panel adds "The file it saved was written all the same" rather than claim nothing changed. If a file already put in place cannot be taken back, the panel says "*name* did not change the document, but a file already written could not be taken back; its old contents are kept beside it, in a file whose name starts with ~pikslar- and ends with .bak".
- **All or none.** With several files, each is staged first (checked, written to a temporary file and synced), then each is put in place. If one cannot be put in place, those before it are put back from their backups, so a run that fails leaves every file as it was. A cancel seen during staging (between files, or between the megabyte pieces a file is written in), or before the first file is put in place, writes none. The bound on what a cancel cannot interrupt: one piece of at most 1 MiB, the `sync_all` of one temporary file (up to 64 MiB, which on a slow network share can take seconds), and, once files are being put in place, the replaces and moves themselves, which are not stopped half-way so that the set stays all or none.
- **The destination is checked again at write time**, not only when it was chosen (`src/save.rs`). The checks are made on opened handles; the replace and the move that follow go by name, so a same-user program could still swap a file in between (see Limitations):
  - a plain absolute path (a drive or UNC path, or either in the verbatim `\\?\C:\` and `\\?\UNC\` forms; no `\\.\` or other device paths, no `.` or `..`), whose name holds no `:` (no alternate data stream), is not a device name (`nul.txt`, `COM1`, `con .txt` and the rest of `names.rs`'s list), is not of a kind off the allow-list, and does not end in a dot or space;
  - the folder it goes in, and that folder alone, is opened on its own name without following it and checked on the handle: a real folder, not a link, junction or other redirecting reparse point. It is held open, without delete sharing, until the write is done, so it cannot be renamed or replaced meanwhile. The folders above it are not checked one by one: a junction higher up the path is accepted, as the folder it leads to is what the dialog showed the user, and refusing it would refuse user folders moved with a junction. Its final path is what is compared with Pikslar's own folders;
  - the target, opened without following links, is absent or a plain file: a link, junction or other redirecting reparse point, a folder or a device is refused, and so is a read-only file;
  - it is still what it was when chosen: a file that appeared at a place chosen as new is not overwritten (the user was never asked about it), and a file replaced by a folder is refused. The target is checked once more just before it is replaced.
- **How it is written.** The text goes to a new file `~pikslar-<process>-<n>.tmp` beside the target, made with `create_new` (never a file or link already there), written and synced. An existing target is then replaced with the system's `ReplaceFileW` (`pikslar-platform/src/replace.rs`), which keeps the original's permissions, attributes, named streams and creation time, and moves the original to a reserved `~pikslar-<process>-<n>.bak` beside it, removed once every file is in place. A target that did not exist is moved into place with `MoveFileExW` without replace, so a file that appeared at the last moment is never overwritten. A plain rename over the target was not used: it would give the new file the folder's inherited permissions and drop the original's streams. On any failure the temporary file is removed and nothing is assumed about what a failed replace did (`src/rollback.rs`): `ReplaceFileW` can fail having done nothing, having moved the original to the backup name and been unable to move it back (error 1177 with a failed move back), or report an error after replacing. So the target is looked at by file identity (volume and file index, which a rename keeps): the original still there is left alone; the new file or nothing there means the original is put back, but only from a backup that is the original; anything else is not touched. A backup is removed only when its place is seen to hold what it should (the original after a rollback, the new file after a success). Otherwise it is kept beside the target and the run reports that an original could not be put back. A new file is removed on a rollback only when it is still Pikslar's own. The steps that touch the disk are a `Disk` trait, so tests make them fail (`PlainDisk` is the standard-library version tests use).
- **Errors never name the path**, and nor does `Debug`.
- **Links.** A reparse point is refused only when it redirects: when its tag has the name-surrogate bit (bit 29, Windows' `IsReparseTagNameSurrogate`), as symbolic links (`IO_REPARSE_TAG_SYMLINK`), junctions (`IO_REPARSE_TAG_MOUNT_POINT`), WSL links and the other surrogate tags do. Reparse points that keep the file where it is are allowed: OneDrive's and other cloud providers' placeholders (`IO_REPARSE_TAG_CLOUD_*`), deduplicated and compressed files. So a OneDrive-backed Documents folder, and a file in it, can be saved into and over. The tag is read from the opened handle (`FILE_ATTRIBUTE_TAG_INFO` through `GetFileInformationByHandleEx`, `src/reparse.rs`), for the folder and the target alike, and the same test is used when a plugin's own files are read at install and load. Where only a name is looked at (the plugins folder itself, what was at a chosen place when it was chosen), the standard library's `is_symlink`, which tests the same bit, is used. The CLI session sandbox (`pikslar-cli/src/sandbox.rs`) already refused only `is_symlink`, so it needed no change.
- **Limitations.** A program running as the same user can still race the last check and the replace, as both go by name; that is outside the threat model below. If Pikslar is killed or the system fails while files are written, a `~pikslar-<process>-<n>.tmp` (the plugin's bytes) or `~pikslar-<process>-<n>.bak` (the original's bytes, with its original permissions) can stay beside the target; when an original could not be put back, its backup is kept rather than removed. These are data left in the user's folder, and they are not cleaned up later: the names hold the process id and a counter, but a later Pikslar cannot prove such a file is a leftover of its own rather than something the user keeps, so it never deletes one. Remove them by hand; a `.bak` is the original file.

- `FileGrants::for_run(plugin_name, granted)` is made at the start of a run and dropped at its end. It holds the chosen paths, and its `Debug` output never prints them.
- `grants.pick(picker, request)` checks the capability **before** any dialog is shown and allows at most 8 picks per run. It then calls the host's `FilePicker`, which is the open or save dialog the interface implements. A cancel gives no handle. A choice gives an opaque `FileHandle`, a run-local index.
- Reads and writes go through the handle and are each limited to 64 MiB. A read handle cannot write, and a write handle cannot read. An unknown or made-up number is refused. Writes are as above.
- Error messages never include a path. `file_name` gives only the file's name, and gives "the chosen file" for a name that would not show as itself.
- **What the host dialog does.** The request's title is the plugin's text, so a plugin could title a dialog "Choose a palette to open". The app uses the system's own open or save dialog, titled `Plugin *id* "*name*" asks: *the plugin's title*`, so the plugin's words always come after its ASCII id and its name. The id comes first, and a double quote in the name is shown as a single one, so a name such as `Palettes" (com.example.palettes) asks: ` cannot make the title read as another plugin's. A pick that nobody will answer (the window is closing, or the run ended) gives the script none at once. The filter offers only the extensions the request named (`*.*` when it named none). The save dialog asks before overwriting, refuses read-only files, takes a shortcut as the shortcut file itself (not what it points to), starts in no folder and with no name of the plugin's, and gives a typed name the first extension the plugin asked for.

The run is on a thread of its own and dialogs belong to the window's thread. `PickBridge` (`crates/pikslar/src/script_files.rs`) carries the question across: the run puts it in a slot and waits; the window thread, which is pumping messages while it waits for the run, shows the dialog and puts the answer back.

## In the app

- **Scripts menu.** The Scripts button in the top bar opens it: "Script panel...", "Install plugin...", then one entry per installed plugin, "Plugin: *name*", or "Plugin: *id* (disabled)".
- **Install.** "Install plugin..." asks for the plugin's `plugin.json` and reviews its folder. The review covers the window and shows `InstallReview::lines()`, one capability per line, with Install and Do not install. The description is cut to three lines, with "..." when it was longer. When the window is too small, or the interface too large, to show every capability and the fixed statement above the buttons, Install is withheld and the review says to enlarge the window or lower the interface size; Do not install still works. Esc does not install. A package that fails its checks installs nothing and says why in the status line.
- **A plugin's menu** says who it is in three lines that run nothing: "*name* *version* (*id*)", "Author: *author* (unverified: as the plugin states it)" or "Author: not stated", and "Fingerprint: *the review's fingerprint*", so the installed plugin can be compared with the review and a published value. Then it lists its commands, and ends with Uninstall. A disabled plugin's menu shows only "Disabled: *the reason*" and Uninstall.
- **Running a command** loads the plugin again first, so a plugin whose files changed since the list was read is refused ("*id* cannot run: *reason*") and runs nothing. The menu runs a command by its id, which is looked up again in the freshly loaded plugin, so a command that was removed or renamed meanwhile is refused rather than run in another's place. Run in the Scripts panel loads and checks the plugin again on every run, and approving or uninstalling a plugin forgets a command of it that the panel had loaded. A command runs like a script in the Scripts panel: one undo step named "*plugin*: *command*", nothing changed on failure or Stop, its output in the panel. Its run gets exactly `granted()`.
- **Not protected against: lookalike names.** Display text is checked for hidden and reordering characters, but not for letters from another script that look the same (a Cyrillic "а" in place of a Latin "a"). The review and the plugin's menu also show the id, which is ASCII only, and the dialog title uses the name as the review showed it.

## Threat model

**Protected against: a hostile or careless plugin author.** A crafted folder cannot:

- write outside `plugins/<id>`. Ids and names are whitelists, and links and junctions are refused when reading and never followed when removing.
- exhaust memory or disk. Every count and size is bounded before allocation.
- hide what it asks for. Capabilities form a closed list shown in plain words, an unknown word refuses the install, and an upgrade marks new capabilities.
- disguise its text with invisible or reordering characters.
- swap content between review and install, because the reviewed bytes are what gets installed.
- take over another plugin's id without the review saying "This replaces version X". The stronger warning needs a different name or author, and both can be copied.
- reach files or the network. There is no path capability, the language has no ambient access (`docs/scripting-language.md`), and files come only through pickers.

**Detected: a plugin folder that never went through the review.** A folder copied into `plugins/` by hand, or dropped there by another program, can bring an `installed.json` of its own, and that record's hash is one anyone can compute. So approval is also noted outside the plugins folder: `plugin-approvals.txt`, beside it, holds one line per plugin, its id and the SHA-256 of the install record Pikslar wrote after the person answered Yes. A plugin loads only if its record is the one approved; otherwise it is disabled ("it was not installed through Pikslar's install review"). Uninstall removes the line. Plugins cannot write the file: their file access never reaches the settings folder. A program running as the same user can still edit it (below). Added in RC4 (review finding S1).

**Detected: accidental or unexpected change.** This covers a partial copy, an editor saving into the plugin folder, another program dropping a file there, or a crash mid-install. The content hash and the install record show that the files are not what was approved; the plugin is disabled with a reason, and reviewing and installing it again repairs it. This is integrity checking, not protection from someone who means to change the files (below).

**Not protected against: a program running as the same user.** Such a program can rewrite a plugin's files and its `installed.json` together, and the record could be rebuilt to match. The record is not signed. Signing it with a per-user key (DPAPI) would raise the bar, but a program that can already write the user's profile can also change Pikslar's settings, or Pikslar itself. That is outside what Pikslar can defend, as for any desktop application.

**Not checked:** who the author is. The `author` field is the plugin's own claim, and the review and the menu mark it as unverified. The fingerprint identifies the files, not the person who made them: two plugins with the same fingerprint have the same files, and that is all it shows. There is no signing, store or update service. Cryptographic signing is deferred (owner decision, 2026-11-17).

## Deliberately impossible

- Running anything at install: no install scripts, hooks or first-run code.
- A plugin naming, reading or writing a path, listing a folder, or reaching the network, processes, environment variables or the clipboard.
- Native code: no DLLs and no WASM. WASM plugins would come only on demand, as an optional library (roadmap M12).
- Nested folders, archives, links, or any file a command does not run.
- Capabilities granted later without a review: the only way to change what a plugin gets is a new version, which is reviewed.
- Downgrades, and changed files under an unchanged version number.

## Tests

| File | Covers |
|---|---|
| `src/sha256.rs` | FIPS 180-4 vectors, one million `a`s, split updates |
| `src/names.rs` | Ids, script names (traversal, absolute, UNC, ADS, devices, case, trailing dot or space), display text |
| `src/capability.rs`, `src/record.rs` | Capability words and sets. Record round trip and damaged records. |
| `src/files.rs` | Handles: access, unknown handles, no dialog without the capability, cancel, pick limit, one write per handle, no path in errors or `Debug` |
| `src/save.rs` | Destination paths: relative, `.` and `..`, device paths and names, streams, trailing dots and spaces, program and script kinds |
| `src/kinds.rs`, `src/startup.rs`, `src/identity.rs` | The allow-list in any case; every other kind refused, also after dots and spaces; names without an extension allowed; the Startup folders refused. File identity kept across a rename; final paths through a junction; folders within folders. |
| `tests/redteam_place.rs` | A replace failing cleanly, half-way (original at the backup) and after replacing, each with the put-back working or failing: the original is at its place or kept in a `.bak`, never deleted. Several files with the last lost half-way. A new file placed by a move that reported an error is taken away. Program names refused whatever the dialog offered, and by an existing file's short name (where the volume makes them). Pikslar's own folders refused directly, below, and through a junction above them. A cancel seen between pieces of a large file. |
| `src/reparse.rs` | Tag classification with known values: symlink, junction, WSL link and other surrogates redirect; cloud (`CLOUD` to `CLOUD_F`), OneDrive, dedup, WOF, NFS, app execution alias, AF_UNIX, ProjFS, HSM and no tag do not. On handles: a plain file and folder do not redirect, a junction does. |
| `tests/redteam_write.rs` | Files replaced and made with nothing left beside them. A junction as the folder and as the target, a file symbolic link as the target (where allowed), a target replaced by a folder after the pick, a folder chosen, a file appearing after the pick, streams and device names, a read-only target, a full disk, all or none over three files, a cancel mid-way, one write per handle and the size limit. Every refused case leaves the files as they were and no temporary file. |
| `pikslar-platform/src/replace.rs` | `ReplaceFileW` keeps the original's named stream and leaves it at the backup; a new file never replaces one; a failed replace leaves the original |
| `tests/manifest.rs` | A corpus of malformed manifests: structure, unknown and duplicate keys, kinds, API, version forms, fields, capabilities, commands, size and nesting |
| `tests/package.rs` | Hostile folders: extra, unused and missing files, case, nested folders, too many files, oversized files and totals, binary scripts, junctions inside and as the folder, file symlinks (where allowed), missing folder |
| `tests/store.rs` | Review writes nothing and denial leaves nothing. Approval installs what was reviewed. Upgrade, downgrade, same-version and reinstall rules. Stale and cross-store approvals. Six kinds of tampering disable a plugin and reinstalling repairs it. Record edits. A planted junction. Uninstall leaves nothing. Plugins off. Stray folders. The id-takeover warning. |
| `pikslar-bind/tests/files.rs` | A chosen file read by handle and name. Cancel gives nil. No capability: no functions and no dialog. Made-up handles, no picker, the grant checked again, non-UTF-8 refused without its path. A second read costs nothing more; a cancel stops a large read. |
| `pikslar-bind/tests/write.rs` | A file written through the save dialog when the run succeeds; a failed, cancelled, or end-of-run write failure leaves the document and the file as they were; an edit and a file kept together; one write per handle; read and write handles never swapped; for every grant, the functions it lacks do not compile and no dialog is shown (no write without `pick-file-to-write`, no edits with `read-document` alone, no pixels read with `edit-document` alone). |
| `pikslar/src/plugin_ui_tests.rs`, `script_files.rs` | Review, menu entries, exactly the granted capabilities, tamper and uninstall, plugins off, a pick answered through the bridge with the plugin's name in the title, a save answered through the bridge and written with the system's replace, which keeps no backup after a success and refuses a protected folder; the panel's words for a file that could not be taken back |
| `spikes/e2e-plugins.ps1` | The real window: install after the review, a command's pixels, a file read through the titled dialog, a read-only plugin that cannot paint, a file saved through the titled save dialog (new, overwritten after the dialog's question, cancelled) with its bytes checked and nothing left beside it, a typed `.bat` name and a place in the settings folder refused, a tampered plugin shown disabled, uninstall (45 checks) |
