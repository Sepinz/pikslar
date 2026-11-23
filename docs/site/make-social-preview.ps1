# Draws site/assets/og.png, the 1280 by 640 social preview for the site and the repository.
# Left: the program's mark and its name in the program's own 5 by 7 glyphs, both drawn as whole
# pixels, and one line of text. Right: the canvas of a screenshot, cropped at 1:1 so no pixel is
# resampled. Windows PowerShell and System.Drawing only.
#
#   powershell -NoProfile -File docs\site\make-social-preview.ps1 -Shot site\assets\img\editor-2x.png `
#       -Crop "490,165,580,580" -Out site\assets\og.png
param(
    [Parameter(Mandatory = $true)][string]$Shot,
    # x,y,width,height of the canvas in the screenshot, in its own pixels.
    [Parameter(Mandatory = $true)][string]$Crop,
    [Parameter(Mandatory = $true)][string]$Out
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
function C($hex) { [System.Drawing.ColorTranslator]::FromHtml("#$hex") }
function Fill($g, $hex, $x, $y, $w, $h) { $b = New-Object System.Drawing.SolidBrush (C $hex); $g.FillRectangle($b, $x, $y, $w, $h); $b.Dispose() }

$W = 1280; $H = 640
$bmp = New-Object System.Drawing.Bitmap $W, $H
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.InterpolationMode = "NearestNeighbor"; $g.PixelOffsetMode = "Half"; $g.SmoothingMode = "None"
$g.TextRenderingHint = "AntiAliasGridFit"

# The base and a faint 24-pixel grid, as behind the site's hero.
Fill $g "121316" 0 0 $W $H
$grid = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(14, 232, 228, 218))
for ($x = 0; $x -lt $W; $x += 24) { $g.FillRectangle($grid, $x, 0, 1, $H) }
for ($y = 0; $y -lt $H; $y += 24) { $g.FillRectangle($grid, 0, $y, $W, 1) }

# The mark: the program icon's motif, an amber P on a 4 by 5 light box, the last cell the cursor.
$mx = 72; $my = 88; $cell = 16; $gap = 4
Fill $g "6a6e7e" $mx $my 120 140
Fill $g "1b1c21" ($mx + 2) ($my + 2) 116 136
$motif = "1110", "1001", "1110", "1000", "1002"
for ($r = 0; $r -lt 5; $r++) { for ($c = 0; $c -lt 4; $c++) {
    $x = $mx + 22 + $c * ($cell + $gap); $y = $my + 22 + $r * ($cell + $gap)
    switch ($motif[$r][$c]) {
        "1" { Fill $g "ffb347" $x $y $cell $cell }
        "0" { Fill $g "121316" $x $y $cell $cell }
        "2" { Fill $g "4cc9f0" $x $y $cell $cell; Fill $g "121316" ($x + 3) ($y + 3) ($cell - 6) ($cell - 6) }
    }
} }

# "PIKSLAR" in the program's 5 by 7 glyphs, one column a byte, bit 0 at the top.
$glyphs = @{ P = 0x7F, 0x09, 0x09, 0x09, 0x06; I = 0x41, 0x7F, 0x41; K = 0x7F, 0x08, 0x14, 0x22, 0x41
    S = 0x46, 0x49, 0x49, 0x49, 0x31; L = 0x7F, 0x40, 0x40, 0x40, 0x40; A = 0x7E, 0x11, 0x11, 0x11, 0x7E; R = 0x7F, 0x09, 0x19, 0x29, 0x46 }
$u = 10; $x = 72; $top = 290
foreach ($ch in "PIKSLAR".ToCharArray()) {
    foreach ($col in $glyphs["$ch"]) {
        for ($y = 0; $y -lt 7; $y++) { if (($col -shr $y) -band 1) { Fill $g "e8e4da" $x ($top + $y * $u) $u $u } }
        $x += $u
    }
    $x += $u
}

# One line of text and the state, in the system's face.
$font = New-Object System.Drawing.Font("Segoe UI", 26, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
$small = New-Object System.Drawing.Font("Cascadia Mono", 18, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
$text = New-Object System.Drawing.SolidBrush (C "e8e4da"); $muted = New-Object System.Drawing.SolidBrush (C "a3a1ab")
$g.DrawString("A lightweight pixel-art editor", $font, $text, 68, 400)
$g.DrawString("and animation studio for Windows.", $font, $text, 68, 436)
Fill $g "ffb347" 72 520 10 10
# The dot as a code, since Windows PowerShell reads a script without a byte-order mark as ANSI.
$dot = "  $([char]0xB7)  "
$g.DrawString("Native${dot}Rust${dot}0.9.0 beta", $small, $muted, 92, 513)

# The canvas, cropped from the screenshot at 1:1, in a one-unit frame.
$c = $Crop -split "," | ForEach-Object { [int]$_ }
$src = [System.Drawing.Image]::FromFile((Resolve-Path $Shot).Path)
$fx = $W - 72 - $c[2]; $fy = [int](($H - $c[3]) / 2)
Fill $g "2a2c34" ($fx - 7) ($fy - 7) ($c[2] + 14) ($c[3] + 14)
Fill $g "1b1c21" ($fx - 6) ($fy - 6) ($c[2] + 12) ($c[3] + 12)
$g.DrawImage($src, (New-Object System.Drawing.Rectangle $fx, $fy, $c[2], $c[3]), (New-Object System.Drawing.Rectangle $c[0], $c[1], $c[2], $c[3]), "Pixel")
$src.Dispose(); $g.Dispose()
$bmp.Save((Join-Path (Resolve-Path (Split-Path $Out)).Path (Split-Path $Out -Leaf)), [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
"wrote ${Out}: $((Get-Item $Out).Length) bytes"
