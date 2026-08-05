# Draws the bitmaps the installers show.
#
# The NSIS installer is a single custom window, so its whole backdrop — gradient,
# glow, logo, wordmark — is one bitmap with the live controls placed on top. The
# MSI keeps the stock WiX wizard and only swaps its two images.
#
# Neither installer can scale or tint a bitmap at build time, so the art is
# rendered once here and committed under src-tauri/installer/. Re-run after
# changing the icon, the window size or the palette:
#
#   powershell -File scripts/installer-art.ps1
#
# Windows-only: it draws through System.Drawing, which is also why the output is
# checked in rather than generated in CI.

Add-Type -AssemblyName System.Drawing

$root = Split-Path -Parent $PSScriptRoot
$logoPath = Join-Path $root "src-tauri\icons\icon.png"
$outDir = Join-Path $root "src-tauri\installer"

if (-not (Test-Path $outDir)) { New-Item -ItemType Directory -Path $outDir | Out-Null }

# Must match IB_WIN_W / IB_WIN_H in theme.nsh.
$winW = 520
$winH = 340

# Tokyo Night, the theme the app opens with. theme.nsh repeats the ones NSIS
# needs as hex strings.
$bgTop = [System.Drawing.Color]::FromArgb(255, 32, 34, 48)
$bgBottom = [System.Drawing.Color]::FromArgb(255, 18, 19, 28)
$textPrimary = [System.Drawing.Color]::FromArgb(255, 240, 243, 252)
$textMuted = [System.Drawing.Color]::FromArgb(255, 124, 132, 163)
$cyan = [System.Drawing.Color]::FromArgb(255, 34, 201, 221)
$amber = [System.Drawing.Color]::FromArgb(255, 251, 191, 36)

function New-Font {
    param([string[]]$Families, [single]$Size, [System.Drawing.FontStyle]$Style = [System.Drawing.FontStyle]::Regular)

    foreach ($family in $Families) {
        try {
            $ff = New-Object System.Drawing.FontFamily($family)
            return New-Object System.Drawing.Font($ff, $Size, $Style, [System.Drawing.GraphicsUnit]::Pixel)
        } catch {
            continue
        }
    }
    return New-Object System.Drawing.Font("Arial", $Size, $Style, [System.Drawing.GraphicsUnit]::Pixel)
}

$fontBanner = New-Font @("Segoe UI Semibold", "Segoe UI") 16

$centered = New-Object System.Drawing.StringFormat
$centered.Alignment = [System.Drawing.StringAlignment]::Center
$centered.LineAlignment = [System.Drawing.StringAlignment]::Near

$rightAligned = New-Object System.Drawing.StringFormat
$rightAligned.Alignment = [System.Drawing.StringAlignment]::Far
$rightAligned.LineAlignment = [System.Drawing.StringAlignment]::Center

function New-Canvas {
    param([int]$Width, [int]$Height)

    # 24bpp: Win32 static controls paint a 32bpp BMP's alpha channel as black.
    $bmp = New-Object System.Drawing.Bitmap($Width, $Height, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    # ClearType's subpixel output fringes against the dark gradient; grayscale
    # antialiasing does not.
    $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
    return @{ Bitmap = $bmp; Graphics = $g }
}

# A soft radial bloom, drawn under the logo so the mark does not sit flat on the
# gradient.
function Add-Glow {
    param([System.Drawing.Graphics]$G, [single]$CX, [single]$CY, [single]$Radius, [System.Drawing.Color]$Color, [int]$Alpha)

    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $path.AddEllipse($CX - $Radius, $CY - $Radius, $Radius * 2, $Radius * 2)
    $brush = New-Object System.Drawing.Drawing2D.PathGradientBrush($path)
    $brush.CenterColor = [System.Drawing.Color]::FromArgb($Alpha, $Color.R, $Color.G, $Color.B)
    $brush.SurroundColors = [System.Drawing.Color[]]@([System.Drawing.Color]::FromArgb(0, $Color.R, $Color.G, $Color.B))
    $G.FillPath($brush, $path)
    $brush.Dispose()
    $path.Dispose()
}

function Add-Gradient {
    param([System.Drawing.Graphics]$G, [int]$X, [int]$Y, [int]$W, [int]$H, [single]$Angle = 70.0)

    $rect = New-Object System.Drawing.Rectangle($X, $Y, $W, $H)
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush($rect, $bgTop, $bgBottom, $Angle)
    $G.FillRectangle($brush, $rect)
    $brush.Dispose()
}

# The logo and, for the MSI only, the name under it.
#
# The NSIS window draws its own text: a bitmap can only be handed to a static
# control at one size, and anything but an exact match goes through a nearest-
# neighbour stretch that leaves letterforms ragged. Gradients and the logo
# survive that; text does not. WiX has no controls to draw with, so its panel
# keeps the drawn version.
function Add-Masthead {
    param(
        [System.Drawing.Graphics]$G,
        [System.Drawing.Image]$Logo,
        [int]$X, [int]$Y, [int]$W,
        [single]$Scale = 1.0,
        [bool]$WithText = $false
    )

    $cx = $X + $W / 2.0

    Add-Glow $G $cx ($Y + 80 * $Scale) (150 * $Scale) $cyan 40
    Add-Glow $G ($cx + 52 * $Scale) ($Y + 44 * $Scale) (100 * $Scale) $amber 28

    $logoSize = 88 * $Scale
    $G.DrawImage($Logo, ($cx - $logoSize / 2.0), ($Y + 36.0 * $Scale), [single]$logoSize, [single]$logoSize)

    if (-not $WithText) { return }

    # Sizes are expressed against the 520x340 window and multiplied here.
    $wordFont = New-Font @("Segoe UI Semibold", "Segoe UI") (34.0 * $Scale)
    $tagFont = New-Font @("Segoe UI") (13.0 * $Scale)

    $wordBrush = New-Object System.Drawing.SolidBrush($textPrimary)
    $wordRect = New-Object System.Drawing.RectangleF -ArgumentList ([single]$X), ([single]($Y + 138.0 * $Scale)), ([single]$W), ([single](52.0 * $Scale))
    $G.DrawString("Hangar.AI", $wordFont, $wordBrush, $wordRect, $centered)
    $wordBrush.Dispose()

    $taglineBrush = New-Object System.Drawing.SolidBrush($textMuted)
    $taglineRect = New-Object System.Drawing.RectangleF -ArgumentList ([single]$X), ([single]($Y + 190.0 * $Scale)), ([single]$W), ([single](44.0 * $Scale))
    $G.DrawString("A terminal multiplexer for CLI coding agents", $tagFont, $taglineBrush, $taglineRect, $centered)
    $taglineBrush.Dispose()

    $wordFont.Dispose()
    $tagFont.Dispose()
}

function Save-Bmp {
    param($Canvas, [string]$Name)

    $path = Join-Path $outDir $Name
    $Canvas.Graphics.Dispose()
    $Canvas.Bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Bmp)
    $Canvas.Bitmap.Dispose()
    Write-Host "wrote $path"
}

$logo = [System.Drawing.Bitmap]::FromFile($logoPath)

# The NSIS window backdrop, at 100% and at 200%. theme.nsh picks whichever is
# closer to the display it ends up on, so neither is stretched far enough to
# show it.
foreach ($s in @(1, 2)) {
    $c = New-Canvas ($winW * $s) ($winH * $s)
    Add-Gradient $c.Graphics 0 0 ($winW * $s) ($winH * $s) 65.0
    Add-Masthead $c.Graphics $logo 0 0 ($winW * $s) $s
    if ($s -eq 1) { Save-Bmp $c "backdrop.bmp" } else { Save-Bmp $c "backdrop@2x.bmp" }
}

# WiX has no equivalent of a custom window, so the MSI keeps the stock light
# wizard and only its two images are ours.
$c = New-Canvas 493 58
$c.Graphics.Clear([System.Drawing.Color]::White)
$logoSize = 32
$name = "Hangar.AI"
$nameWidth = [Math]::Ceiling($c.Graphics.MeasureString($name, $fontBanner).Width)
$c.Graphics.DrawImage($logo, [single](493 - 12 - $nameWidth - 6 - $logoSize), [single]((58 - $logoSize) / 2.0), [single]$logoSize, [single]$logoSize)
$nameBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 26, 27, 38))
$c.Graphics.DrawString($name, $fontBanner, $nameBrush, (New-Object System.Drawing.RectangleF(0.0, 0.0, 481.0, 58.0)), $rightAligned)
$nameBrush.Dispose()
$rule = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    (New-Object System.Drawing.Rectangle(0, 55, 493, 3)), $cyan, $amber, 0.0)
$c.Graphics.FillRectangle($rule, 0, 55, 493, 3)
$rule.Dispose()
Save-Bmp $c "wix-banner.bmp"

# WixUI writes its welcome text over the right-hand side, which has to stay
# light; only the first 164px are ours to decorate.
$c = New-Canvas 493 312
$c.Graphics.Clear([System.Drawing.Color]::White)
Add-Gradient $c.Graphics 0 0 164 312
Add-Masthead $c.Graphics $logo 0 30 164 0.62 $true
$edge = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 226, 230, 240))
$c.Graphics.FillRectangle($edge, 164, 0, 1, 312)
$edge.Dispose()
Save-Bmp $c "wix-dialog.bmp"

$logo.Dispose()
$fontBanner.Dispose()
