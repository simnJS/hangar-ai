# Draws the Hangar.AI mark.
#
# The mark is the app itself in miniature: a workspace cut into panes, one of
# them active. Three rectangles on a 24-unit grid, no curves — the same squared
# language as the interface.
#
# This script is the single source for the drawing. It writes the vector master
# and the raster master from the same numbers, so the two can never drift:
#
#   powershell -File scripts/logo.ps1
#
# Then propagate it to everything that embeds a copy:
#
#   pnpm tauri icon assets/logo.png        # the OS icon set under src-tauri/icons/
#   powershell -File scripts/installer-art.ps1   # the installer bitmaps
#
# Windows-only: it draws through System.Drawing, which is also why the output is
# checked in rather than generated in CI. The in-app mark lives separately in
# src/components/Logo.tsx — it is the bare glyph, without the tile behind it.

Add-Type -AssemblyName System.Drawing

$root = Split-Path -Parent $PSScriptRoot
$outDir = Join-Path $root "assets"

if (-not (Test-Path $outDir)) { New-Item -ItemType Directory -Path $outDir | Out-Null }

# Geometry, in pixels of the 1024 master. The mark occupies 24 units of 26px,
# leaving a 200px quiet zone: 61% coverage, which is what a tile-backed icon
# wants. Every value is a whole pixel so the edges stay crisp when the icon set
# is downscaled to 16px.
$size = 1024
$margin = 200
$u = 26

$paneW = 10 * $u   # active pane, full height
$gutter = 3 * $u
$restX = $margin + $paneW + $gutter
$restW = 11 * $u
$topH = 10 * $u
$botY = $margin + $topH + $gutter
$botH = 11 * $u
$fullH = 24 * $u

# Tokyo Night, the theme the app opens with. The backdrop is a shade above the
# terminal background so the tile still reads as a shape on a black taskbar.
$bgTop = [System.Drawing.Color]::FromArgb(255, 36, 39, 57)
$bgBottom = [System.Drawing.Color]::FromArgb(255, 20, 21, 29)
$accent = [System.Drawing.Color]::FromArgb(255, 122, 162, 247)
$pane = [System.Drawing.Color]::FromArgb(255, 192, 202, 245)

$bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality

$rect = New-Object System.Drawing.Rectangle(0, 0, $size, $size)
$backdrop = New-Object System.Drawing.Drawing2D.LinearGradientBrush($rect, $bgTop, $bgBottom, 65.0)
$g.FillRectangle($backdrop, $rect)
$backdrop.Dispose()

$accentBrush = New-Object System.Drawing.SolidBrush($accent)
$paneBrush = New-Object System.Drawing.SolidBrush($pane)
$g.FillRectangle($accentBrush, $margin, $margin, $paneW, $fullH)
$g.FillRectangle($paneBrush, $restX, $margin, $restW, $topH)
$g.FillRectangle($paneBrush, $restX, $botY, $restW, $botH)
$accentBrush.Dispose()
$paneBrush.Dispose()

$pngPath = Join-Path $outDir "logo.png"
$g.Dispose()
$bmp.Save($pngPath, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host "wrote $pngPath"

# The vector master, on the same 1024 grid and with the same numbers.
$hex = {
    param([System.Drawing.Color]$C)
    "#{0:x2}{1:x2}{2:x2}" -f $C.R, $C.G, $C.B
}

$svg = @"
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 $size $size" width="$size" height="$size" role="img" aria-label="Hangar.AI">
  <defs>
    <linearGradient id="tile" x1="0" y1="0" x2="0.42" y2="1">
      <stop offset="0" stop-color="$(& $hex $bgTop)" />
      <stop offset="1" stop-color="$(& $hex $bgBottom)" />
    </linearGradient>
  </defs>
  <rect width="$size" height="$size" fill="url(#tile)" />
  <rect x="$margin" y="$margin" width="$paneW" height="$fullH" fill="$(& $hex $accent)" />
  <rect x="$restX" y="$margin" width="$restW" height="$topH" fill="$(& $hex $pane)" />
  <rect x="$restX" y="$botY" width="$restW" height="$botH" fill="$(& $hex $pane)" />
</svg>
"@

$svgPath = Join-Path $outDir "logo.svg"
Set-Content -Path $svgPath -Value $svg -Encoding utf8
Write-Host "wrote $svgPath"
