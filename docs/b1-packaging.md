# B1 batch 2P: portable Windows package and reproducible release build

Status: built, measured and reproduced on the development machine, 2026-11-21, at commit
0e408a1. Nothing here has been tried on a second machine; see "Verified and simulated".

## What is built

`spikes/make-release.ps1` makes `Pikslar-0.9.0-beta.1-windows-x64.zip` and, beside it,
`Pikslar-0.9.0-beta.1-windows-x64.zip.sha256.txt`. The zip holds one folder:

| File | What it is |
|---|---|
| pikslar.exe | the editor (GUI subsystem), icon, version block, the program's own manifest |
| pikslar-cli.exe | the command-line tool, the same version block, a console-neutral manifest |
| README, RELEASE-NOTES, KNOWN-ISSUES, TROUBLESHOOTING | placeholders to fill before B1 ships |
| LICENSE.txt | the PolyForm Noncommercial 1.0.0 terms with the Required Notice line |
| THIRD-PARTY-NOTICES.txt | what the linker actually took, with licence texts (one gap, below) |
| BUILD.txt | commit, tag, toolchain, linker and its hashes, flags, sizes and hashes |
| SHA256SUMS.txt | hashes of every other file in the folder |

All text files are CRLF in the package and LF in the repository (`docs/release/`).

Version: the workspace is `0.9.0-beta.1`. `pikslar.exe --version` prints
`Pikslar 0.9.0-beta.1 (<12-digit commit>)` and exits without a window and without touching
settings or recovery (test `crates/pikslar/tests/version.rs`). From a console it writes to the
redirected stdout if there is one, otherwise it attaches to the parent console
(`pikslar-platform/src/console.rs`); the prompt comes back first, because a GUI program does not
hold the console, so the line appears after it. Started from Explorer, a shortcut or Win+R there
is no console to attach to: it shows nothing and exits at once. The Keys dialog footer shows
"Pikslar 0.9.0-beta.1" in muted text, shortened to the bare version or left out when narrow.
The commit comes from `PIKSLAR_COMMIT`, which the release script sets; a developer build says
"unknown". The copyright string stays the neutral placeholder ("Copyright notice to be set").

Icon (D3, for the owner's approval): `spikes/make-icon.py` draws a dark Lightbox tile holding a
four-by-five cell grid; the amber cells spell a "P" and the last cell is the focus-blue keyboard
cursor. 16, 20, 24, 32, 40, 48 and 64 are each drawn at their own size; 256 is the 64 drawing
enlarged four times. Sheet: `docs/b1/icon-sheet.png` (1x and 4x, light and dark).

## The linker decision

Two ways to stop depending on whichever MinGW is first on PATH were compared.

(a) `-C link-self-contained=yes`, rustup's bundled MinGW (GCC 14.2.0, ld 2.44), which links
msvcrt.dll. (b) The WinLibs toolchain already used for development (GCC 16.2.0, MinGW-w64
14.0.0, UCRT, ld 2.47), pinned.

(a) needed one code change, kept because it is harmless either way: the bundled MinGW has no
libdwmapi.a, so `DwmSetWindowAttribute` is now looked up at run time from System32
(`pikslar-platform/src/look.rs`). Then `spikes/crt-check` hashed every C runtime function the
program links (floor, floorf, ceilf, round, roundf, trunc, fmod, fmodf) over all 2^32 f32
inputs and 2^26 f64 inputs, built both ways:

- `roundf` differs: msvcrt's static MinGW `roundf` returns 1 for 0.49999997 and -1 for
  -0.49999997; the Universal CRT returns 0 and -0, which is correct.
- `fmod` and `fmodf` differ only in the bits of NaN results (identical with NaNs counted as
  one value).
- memcpy is slower under (a): 4 KiB copies 0.067 s against 0.044 s per 8 GiB, 16 MiB copies
  0.284 s against 0.235 s. memmove, memset and memcmp are level.

The brief says to stop rather than force the switch on any behavioural or performance change,
so (a) is not used. The release uses (b): `link-self-contained=no`, the linker found by path,
and the build refused unless gcc's and ld's version lines match the constants at the top of
`make-release.ps1`. BUILD.txt records both lines and the SHA-256 of gcc.exe and ld.exe, so a
rebuild can compare them; the script does not refuse on a different hash.

`rust-toolchain.toml` was not added. Pinning 1.98.1 by name makes rustup download a toolchain
(the installed one is "stable", which is 1.98.1), and it would change every developer's
`cargo`. The script instead refuses any rustc whose release, commit hash or host differs from
1.98.1 / 48a229cea / x86_64-pc-windows-gnu. Developers' `cargo test` is unchanged: all release
flags are set by the script in the environment, none in `.cargo/config.toml`.

## Determinism

The script sets `-Wl,--no-insert-timestamp` (PE time stamp 0) and `--remap-path-prefix` for
the checkout, the target folder, CARGO_HOME and RUSTUP_HOME. The resource objects were already
deterministic (`pikslar-res` writes COFF with a zero time stamp). `pe-check`
(`crates/pikslar-res/src/bin/pe-check.rs`) reads the built file and fails on a non-zero time
stamp, an import from any DLL that is not part of Windows, or any of 15 forbidden texts
(user name, profile, checkout, target folder, cargo and rustup homes, the checkout's folder
name as a path part, `\Users\`, `/Users/`, each with both slash kinds), searched as ASCII and
as UTF-16. The package's text files get the same text scan. Both exes pass; their imports are
kernel32, user32, gdi32, ADVAPI32, comdlg32, SHELL32, ntdll, bcryptprimitives, the api-ms-win-core synch set and the
Universal CRT (api-ms-win-crt-*).

Reproduction: the script was run in this worktree and again in a second worktree at the same
commit in a different folder, each into its own empty target folder. Every file came out
byte-identical, the zip included:

| File | SHA-256 |
|---|---|
| pikslar.exe | 2a5ff92182df79d8084b62918b233eac7251ed64b72f5578579ef0dd98e83ecd |
| pikslar-cli.exe | 06b2678381affd97bcca347d7c125a1693ec10f2d0d8a8d8540363e72475f20e |
| the zip | 4222659a79f04b2d50886014db4e3000969f69f135f12b6241d131a49709bae0 |

The second worktree and both extra build folders were deleted afterwards.

## How to reproduce

From a clean checkout, with Rust 1.98.1 and WinLibs GCC 16.2.0 UCRT r1 on PATH:

    powershell -NoProfile -ExecutionPolicy Bypass -File spikes\make-release.ps1 -TargetDir C:\b\t -Out C:\b\d

The script refuses uncommitted changes and empties the target folder first. It builds both programs
with `--release --locked`, runs `pe-check` on each, checks `--version`, runs both crates'
`resources` tests against the built files (`PIKSLAR_EXE`, `PIKSLAR_CLI_EXE`), writes the
documents, hashes, sets every file time to the commit time and writes the zip with sorted
entries. It takes about 80 s here.

Path length matters in a deep checkout: `-TargetDir` and `-Out` must be short. Windows limits
the build-script path and gcc's `-B` lookup to 260 characters, and pikslar-cli's build script
path is the longest; the script stops with a clear message before building if the target
folder is too long. It builds without `--target` for the same reason (the target triple adds
22 characters) and checks instead that rustc's host is the gnu target. For developers: with
pikslar-cli's new build script, a release CARGO_TARGET_DIR of more than about 193 characters
fails to build in this checkout.

## Measurements

The packaged files, unpacked from the zip, measured by `spikes/measure-package.ps1` (the
spikes/limits.ps1 approach: window appearance for launch, cycle time for processor use,
private bytes for memory) with PATH cut to the Windows folders. No stray Pikslar process was
running before or after.

| Measure | Result | Limit |
|---|---|---|
| zip | 2,185,348 bytes | |
| unpacked | 4,328,405 bytes | |
| pikslar.exe | 2,804,736 bytes (2,739 KiB) | soft target 2,816 KiB; perf/limits.txt soft 2,432 KB |
| pikslar-cli.exe | 1,461,248 bytes (1,427 KiB) | |
| first launch after unpacking | 77 ms | (not a cold disk: the files were just written) |
| warm launch, median of 15 | 20 ms | 100 ms |
| private memory at start, 128 canvas | 7.18 MB | 12 MB |
| idle processor use over 5 s | 0.000 % | 0 |

pikslar.exe is under the 2,816 KiB soft target and over the 2,432 KB soft line in
perf/limits.txt, which was left unchanged.

## Verified and simulated

Verified on this machine: the build, both scans, both resources tests, `--version` of both
programs, byte-identical builds from two folders, the measurements above.

Simulated: a clean machine, by running the unpacked package with PATH holding only the Windows
folders, so no MinGW or Rust DLL could be found through PATH. It started, printed its version
and measured as above. The machine still has the development tools installed, the files were
never downloaded (no Mark of the Web), and the import table, not a second machine, is the
evidence that nothing else is needed. A run on a fresh Windows 10 and Windows 11 account is
still to do.

## Installer: compared on paper

| | None (zip) | Per-user installer, no registry | MSIX |
|---|---|---|---|
| Install | unzip anywhere | copy to %LOCALAPPDATA%\Programs, Start menu shortcut | Store-style install |
| Admin rights | none | none | none |
| Uninstall | delete the folder | an uninstall script or entry; without registry, no Apps list entry | clean, built in |
| Signing | optional | optional, SmartScreen still applies | required: unsigned MSIX will not install outside developer mode |
| Work | done | an installer to write and test, and its own SmartScreen prompt | packaging, a certificate, identity, and the package's virtualised AppData |

Recommendation for B1: no installer. The program is two files and keeps nothing beside itself;
a zip is what testers expect from a portable beta, and every installer option adds a program
to trust and test. Reconsider for 1.0, when a licence, a name and possibly a certificate exist:
MSIX then gives the cleanest uninstall, but only with signing.

## SmartScreen, Smart App Control and Mark of the Web

- A browser-downloaded zip carries a Mark of the Web (Zone.Identifier). Windows' own "Extract
  all" passes it to the extracted files; some third-party extractors do not.
- With the mark, the first start of an unsigned, unknown program shows Microsoft Defender
  SmartScreen's "Windows protected your PC". "More info", then "Run anyway" starts it; the
  mark can also be removed beforehand with File Properties, "Unblock". Reputation builds per
  file hash, so every new build starts unknown again; a code-signing certificate builds
  reputation per publisher instead.
- Smart App Control (Windows 11, on only for clean installs that pass its evaluation) blocks
  unsigned programs without reputation outright, with no "Run anyway". Testers who have it on
  cannot run this build without turning it off, which cannot be turned back on without
  reinstalling Windows. Signing is the only real answer; TROUBLESHOOTING.txt should say so.
- None of this was tried here: the package was never downloaded, so no mark was set. To be
  checked on a tester machine.

## Open items

- THIRD-PARTY-NOTICES: the verbatim text of mingw-w64's
  `COPYING.MinGW-w64-runtime.txt` (14.0.0) is not yet included; WinLibs does not ship it and
  it was not downloaded. Marked "NOT YET COMPLETE" in the file. Needed before any
  distribution.
- The owner: licence and copyright (D1), icon approval (D3), whether to pin the toolchain with
  `rust-toolchain.toml` (a download), and code signing.
