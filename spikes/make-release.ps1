# Builds the portable Windows package from the commit that is checked out, reproducibly
# (docs/b1-packaging.md). Refuses uncommitted work, so that BUILD.txt always names what was
# built. Derived from make-usability-kit.ps1.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File spikes\make-release.ps1 `
#       -TargetDir C:\short\path [-Out dist] [-Gcc <path to x86_64-w64-mingw32-gcc.exe>]
#
# The target folder is emptied first. Keep its path short (see $PATH_BUDGET below): gcc and
# Cargo's build scripts cannot use paths of 260 characters or more.
param(
    [Parameter(Mandatory = $true)][string]$TargetDir,
    [string]$Out = "dist",
    [string]$Gcc = "",
    # For test packages only: build even when the upstream notice files the owner must supply
    # (docs\release\licenses\README.txt) are missing. The package is then marked, in its
    # notices and BUILD.txt, as incomplete and not for distribution.
    [switch]$AllowIncompleteNotices
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

# The pinned toolchain (rustc -vV) and linker (gcc --version, ld --version).
$RUST_RELEASE = "1.98.1"
$RUST_COMMIT = "48a229ceaefd4985c50990b14116b6d856af0985"
$RUST_HOST = "x86_64-pc-windows-gnu"
$GCC_VERSION = "x86_64-w64-mingw32-gcc.exe (MinGW-W64 x86_64-ucrt-posix-seh, built by Brecht Sanders, r1) 16.2.0"
$LD_VERSION = "GNU ld (Binutils for MinGW-W64 x86_64, built by Brecht Sanders, r1) 2.47.20260726"
# The longest path a build makes below the target folder: pikslar-cli's build script.
$PATH_BUDGET = 260 - "\release\build\pikslar-cli-0123456789abcdef\build-script-build.exe".Length

function Fail($why) { throw "make-release: $why" }
function Sha($path) { (Get-FileHash $path -Algorithm SHA256).Hash.ToLowerInvariant() }
# A native program's output, stderr included, as text; $LASTEXITCODE is kept. Windows
# PowerShell turns stderr lines into errors, which must not stop the script here.
function Native($exe, [string[]]$arguments) {
    $saved = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try { return (& $exe @arguments 2>&1 | ForEach-Object { "$_" }) -join "`n" }
    finally { $ErrorActionPreference = $saved }
}
function Run($exe, [string[]]$arguments) {
    $text = Native $exe $arguments
    if ($LASTEXITCODE -ne 0) { Fail "$exe $($arguments -join ' ') failed:`n$text" }
    return $text
}

# CRLF text, as Notepad on older Windows needs.
function Write-Crlf([string]$path, [string[]]$lines) {
    [IO.File]::WriteAllText($path, (($lines -join "`r`n") + "`r`n"), (New-Object Text.UTF8Encoding $false))
}
function Copy-Crlf([string]$from, [string]$to) {
    Write-Crlf $to ([IO.File]::ReadAllText($from) -replace "`r`n", "`n" -split "`n" | Select-Object -SkipLast 1)
}

function Check-Toolchain {
    $v = Run "rustc" @("-vV")
    foreach ($want in @("release: $RUST_RELEASE", "commit-hash: $RUST_COMMIT", "host: $RUST_HOST")) {
        if ($v -notmatch [regex]::Escape($want)) { Fail "rustc must be $RUST_RELEASE for $RUST_HOST ($want):`n$v" }
    }
    return $v.Trim()
}

function Find-Gcc {
    $path = $Gcc
    if (-not $path) { $path = (Get-Command "x86_64-w64-mingw32-gcc.exe" -ErrorAction SilentlyContinue).Source }
    if (-not $path -or -not (Test-Path $path)) { Fail "the pinned WinLibs gcc was not found; pass -Gcc" }
    $path = (Resolve-Path $path).Path
    $first = ((Run $path @("--version")) -split "`r?`n")[0].Trim()
    if ($first -ne $GCC_VERSION) { Fail "gcc must be the pinned WinLibs build:`n  want $GCC_VERSION`n  have $first" }
    $ld = (Run $path @("-print-prog-name=ld")).Trim()
    $ldFirst = ((Run $ld @("--version")) -split "`r?`n")[0].Trim()
    if ($ldFirst -ne $LD_VERSION) { Fail "ld must be the pinned build:`n  want $LD_VERSION`n  have $ldFirst" }
    return @{ Gcc = $path; Ld = (Resolve-Path $ld).Path; GccLine = $first; LdLine = $ldFirst }
}

# Flags for the build. CARGO_ENCODED_RUSTFLAGS keeps paths with spaces whole.
function Set-BuildEnvironment($linker, $target, $commit) {
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { "$env:USERPROFILE\.cargo" }
    $rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { "$env:USERPROFILE\.rustup" }
    $flags = @(
        "-C", "link-self-contained=no",
        "-C", "link-arg=-Wl,--no-insert-timestamp",
        "--remap-path-prefix=$root=/pikslar",
        "--remap-path-prefix=$target=/target",
        "--remap-path-prefix=$cargoHome=/cargo",
        "--remap-path-prefix=$rustupHome=/rustup"
    )
    Remove-Item Env:RUSTFLAGS, Env:CARGO_BUILD_RUSTFLAGS, Env:PIKSLAR_COPYRIGHT -ErrorAction SilentlyContinue
    $env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]0x1f
    $env:CARGO_TARGET_DIR = $target
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = $linker
    $env:PIKSLAR_COMMIT = $commit
    # As printed in BUILD.txt, without this machine's paths.
    return "-C link-self-contained=no -C link-arg=-Wl,--no-insert-timestamp " +
        "--remap-path-prefix=<workspace>=/pikslar --remap-path-prefix=<target>=/target " +
        "--remap-path-prefix=<CARGO_HOME>=/cargo --remap-path-prefix=<RUSTUP_HOME>=/rustup"
}

# Every text a user or workspace path could leave in a binary.
function Forbidden-Texts($target) {
    $texts = @($env:USERNAME, $env:USERPROFILE, $root, $target, "$env:USERPROFILE\.cargo",
        "$env:USERPROFILE\.rustup", "\$(Split-Path $root -Leaf)\", "\Users\", "/Users/")
    # The checkout's folder name only as a path part: "b1-pack" alone also matches b1-packaging.
    $texts += $texts | ForEach-Object { $_ -replace "\\", "/" }
    return $texts | Where-Object { $_ -and $_.Trim().Length -ge 3 } | Sort-Object -Unique
}

function Scan-Binary($checker, $exe, $target) {
    $arguments = @($exe)
    foreach ($t in Forbidden-Texts $target) { $arguments += @("--forbid", $t) }
    $report = Native $checker $arguments
    $code = $LASTEXITCODE
    # The report names the forbidden texts only by count, so it carries no path itself.
    $report
    if ($code -ne 0) { Fail "the import or path scan failed for $exe" }
}

function Check-Text-Files($folder, $target) {
    foreach ($file in Get-ChildItem $folder -Filter *.txt) {
        $text = [IO.File]::ReadAllText($file.FullName)
        $texts = @(Forbidden-Texts $target)
        for ($i = 0; $i -lt $texts.Count; $i++) {
            # Named by its place in the list, so the message itself carries no path.
            if ($text.IndexOf($texts[$i], [StringComparison]::OrdinalIgnoreCase) -ge 0) {
                Fail "$($file.Name) names a local path or user (forbidden text $($i + 1) of $($texts.Count))"
            }
        }
    }
}

# A zip with fixed entry times, sorted, forward slashes: the same files give the same zip.
function Write-Zip($folder, $zip, [DateTimeOffset]$when) {
    Add-Type -AssemblyName System.IO.Compression
    if (Test-Path $zip) { Remove-Item $zip }
    $base = Split-Path $folder -Leaf
    $stream = [IO.File]::Open($zip, [IO.FileMode]::CreateNew)
    try {
        $archive = New-Object IO.Compression.ZipArchive($stream, [IO.Compression.ZipArchiveMode]::Create)
        foreach ($file in Get-ChildItem $folder -File | Sort-Object Name) {
            $entry = $archive.CreateEntry("$base/$($file.Name)", [IO.Compression.CompressionLevel]::Optimal)
            $entry.LastWriteTime = $when
            $to = $entry.Open()
            try { $bytes = [IO.File]::ReadAllBytes($file.FullName); $to.Write($bytes, 0, $bytes.Length) } finally { $to.Dispose() }
        }
        $archive.Dispose()
    } finally { $stream.Dispose() }
}

# The upstream notice files the owner places in docs\release\licenses, verbatim, in the order
# they are added to the notices (see docs\release\licenses\README.txt).
$UPSTREAM_NOTICES = @(
    @{ File = "COPYING.MinGW-w64-runtime.txt"; From = "mingw-w64 v14.0.0, COPYING.MinGW-w64-runtime/COPYING.MinGW-w64-runtime.txt" },
    @{ File = "aseprite-doc-LICENSE.txt"; From = "Aseprite commit 9cf3c128, src/doc/LICENSE.txt" },
    @{ File = "pixman-COPYING.txt"; From = "pixman, COPYING" }
)

# The upstream notice files that are missing, by name.
function Missing-Notices {
    return @($UPSTREAM_NOTICES | Where-Object { -not (Test-Path "docs\release\licenses\$($_.File)") } | ForEach-Object { $_.File })
}

function Upstream-Notices {
    $lines = @()
    foreach ($n in $UPSTREAM_NOTICES) {
        $path = "docs\release\licenses\$($n.File)"
        if (-not (Test-Path $path)) {
            $lines += @("", ("=" * 78), "MISSING: $($n.File) ($($n.From))", "THIS PACKAGE IS INCOMPLETE AND NOT FOR DISTRIBUTION.", ("=" * 78))
            continue
        }
        $lines += @("", ("=" * 78), "Notice file: $($n.File) (verbatim from $($n.From))", ("=" * 78), "")
        $lines += [IO.File]::ReadAllText($path) -replace "`r`n", "`n" -split "`n"
    }
    return $lines
}

function Notices($sysroot) {
    $licenses = Join-Path $sysroot "share\doc\rust\licenses"
    $lines = @([IO.File]::ReadAllText("docs\release\THIRD-PARTY-NOTICES.txt") -replace "`r`n", "`n" -split "`n")
    $lines += Upstream-Notices
    foreach ($name in @("MIT.txt", "Apache-2.0.txt", "Unicode-3.0.txt", "GCC-exception-3.1.txt", "GPL-3.0-or-later.txt")) {
        $path = Join-Path $licenses $name
        if (-not (Test-Path $path)) { Fail "the toolchain has no $name" }
        $lines += @("", ("=" * 78), "Licence text: $name (from the Rust $RUST_RELEASE toolchain)", ("=" * 78), "")
        $lines += [IO.File]::ReadAllText($path) -replace "`r`n", "`n" -split "`n"
    }
    return $lines
}

Push-Location $root
# .NET file calls below resolve relative paths against this, not against the location.
$savedDirectory = [Environment]::CurrentDirectory
[Environment]::CurrentDirectory = $root
try {
    if (git status --porcelain) { Fail "commit everything first: the package must match one commit" }
    $missing = @(Missing-Notices)
    if ($missing.Count -gt 0 -and -not $AllowIncompleteNotices) {
        Fail "docs\release\licenses lacks $($missing -join ', '); see its README.txt (or pass -AllowIncompleteNotices for a test package)"
    }
    $completeness = if ($missing.Count -gt 0) { "INCOMPLETE: notice files missing ($($missing -join ', ')); NOT FOR DISTRIBUTION" } else { "complete (all upstream notice files present)" }
    $commit = (git rev-parse --short=12 HEAD).Trim()
    $tag = (git tag --points-at HEAD) -join ", "
    if (-not $tag) { $tag = "(none)" }
    $when = [DateTimeOffset]::Parse((git log -1 --format=%cI HEAD).Trim())
    $rustc = Check-Toolchain
    $cargo = (Run "cargo" @("-V")).Trim()
    $linker = Find-Gcc
    $sysroot = (Run "rustc" @("--print", "sysroot")).Trim()

    $target = [IO.Path]::GetFullPath($TargetDir)
    if ($target.Length -gt $PATH_BUDGET) { Fail "the target folder's path is $($target.Length) characters; at most $PATH_BUDGET" }
    if (Test-Path $target) { Remove-Item -Recurse -Force $target }
    New-Item -ItemType Directory $target | Out-Null
    $flags = Set-BuildEnvironment $linker.Gcc $target $commit

    "Building $commit with rustc $RUST_RELEASE and $($linker.GccLine)"
    $build = Run "cargo" @("build", "--release", "--locked", "-p", "pikslar", "-p", "pikslar-cli")
    if ($build -match "warning: pikslar") { Fail "the build warned:`n$build" }
    Run "cargo" @("build", "--release", "--locked", "-p", "pikslar-res", "--bin", "pe-check") | Out-Null
    $checker = "$target\release\pe-check.exe"

    $version = ((Run "$target\release\pikslar-cli.exe" @("--version")).Trim() -split " ")[1]
    $name = "Pikslar-$version-windows-x64"
    $package = Join-Path $Out $name
    if (Test-Path $package) { Remove-Item -Recurse -Force $package }
    New-Item -ItemType Directory -Force $package | Out-Null
    Copy-Item "$target\release\pikslar.exe", "$target\release\pikslar-cli.exe" $package
    $exe = (Resolve-Path "$package\pikslar.exe").Path
    $cli = (Resolve-Path "$package\pikslar-cli.exe").Path

    "Scanning the binaries"
    Scan-Binary $checker $exe $target
    Scan-Binary $checker $cli $target
    $said = (Run $exe @("--version")).Trim()
    if ($said -ne "Pikslar $version ($commit)") { Fail "pikslar.exe --version said '$said'" }
    "Checking the embedded resources"
    $env:PIKSLAR_EXE = $exe
    $env:PIKSLAR_CLI_EXE = $cli
    try {
        foreach ($crate in @("pikslar", "pikslar-cli")) {
            $t = Run "cargo" @("test", "--release", "--locked", "-p", $crate, "--test", "resources")
            ($t -split "`n" | Where-Object { $_ -match "^test result" }) -join "`n"
        }
    } finally { Remove-Item Env:PIKSLAR_EXE, Env:PIKSLAR_CLI_EXE }

    foreach ($doc in @("README", "RELEASE-NOTES", "KNOWN-ISSUES", "TROUBLESHOOTING", "LICENSE")) {
        Copy-Crlf "docs\release\$doc.txt" "$package\$doc.txt"
    }
    Write-Crlf "$package\THIRD-PARTY-NOTICES.txt" (Notices $sysroot)
    $sizes = foreach ($f in @($exe, $cli)) { "  {0,-16} {1,10:N0} bytes  sha256 {2}" -f (Split-Path $f -Leaf), (Get-Item $f).Length, (Sha $f) }
    Write-Crlf "$package\BUILD.txt" (@(
        "Pikslar $version, Windows x64 portable build",
        "",
        "Commit:     $commit",
        "Tag:        $tag",
        "Committed:  $($when.ToString('yyyy-MM-dd HH:mm:ss zzz'))",
        "",
        "Toolchain:  $($rustc -replace "`r?`n", "`r`n            ")",
        "Cargo:      $cargo",
        "Target:     $RUST_HOST (release profile: fat LTO, one codegen unit, panic=abort, stripped)",
        "Linker:     $($linker.GccLine)",
        "            $($linker.LdLine)",
        "            gcc.exe sha256 $(Sha $linker.Gcc)",
        "            ld.exe  sha256 $(Sha $linker.Ld)",
        "Mode:       pinned WinLibs MinGW-w64 linker, Universal CRT, link-self-contained=no;",
        "            the MinGW default manifest replaced by Pikslar's own (crates/pikslar/build.rs)",
        "Flags:      $flags",
        "Notices:    $completeness",
        "",
        "Program files:"
    ) + $sizes + @("", "Rebuild: spikes\make-release.ps1 at this commit; see docs\b1-packaging.md."))

    $files = Get-ChildItem $package -File | Sort-Object Name
    Write-Crlf "$package\SHA256SUMS.txt" ($files | ForEach-Object { "$(Sha $_.FullName)  $($_.Name)" })
    Check-Text-Files $package $target
    foreach ($f in Get-ChildItem $package -File) { $f.LastWriteTime = $when.LocalDateTime }

    $zip = Join-Path $Out "$name.zip"
    Write-Zip (Resolve-Path $package).Path ([IO.Path]::GetFullPath($zip)) $when
    Write-Crlf "$zip.sha256.txt" @("$(Sha $zip)  $name.zip")

    $unpacked = ($files + (Get-Item "$package\SHA256SUMS.txt") | Measure-Object Length -Sum).Sum
    ""
    "Package:  $package"
    "Zip:      $zip ($((Get-Item $zip).Length) bytes, unpacked $unpacked bytes)"
    $sizes
    "Zip sha256: $(Sha $zip)"
} finally {
    Remove-Item Env:CARGO_ENCODED_RUSTFLAGS, Env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER, Env:PIKSLAR_COMMIT -ErrorAction SilentlyContinue
    [Environment]::CurrentDirectory = $savedDirectory
    Pop-Location
}
