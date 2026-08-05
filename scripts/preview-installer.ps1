# Compiles scripts/preview.nsi and runs it, to look at the installer theme
# without waiting for a full `tauri build`.
#
#   powershell -File scripts/preview-installer.ps1
#   powershell -File scripts/preview-installer.ps1 -NoRun   # compile only
#
# Uses the NSIS toolchain Tauri keeps in %LOCALAPPDATA%\tauri\NSIS, which the
# first `tauri build` on this machine downloads. Falls back to makensis on PATH.

[CmdletBinding()]
param([switch]$NoRun)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot

$candidates = @(
    (Join-Path $env:LOCALAPPDATA "tauri\NSIS\makensis.exe"),
    "${env:ProgramFiles(x86)}\NSIS\makensis.exe",
    "$env:ProgramFiles\NSIS\makensis.exe"
)
$makensis = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $makensis) {
    $onPath = Get-Command makensis -ErrorAction SilentlyContinue
    if ($onPath) { $makensis = $onPath.Source }
}
if (-not $makensis) {
    throw "makensis not found. Run ``pnpm tauri build`` once to let Tauri fetch NSIS, or install NSIS."
}

$outFile = Join-Path $env:TEMP "hangar-ai-preview-setup.exe"
$script = Join-Path $PSScriptRoot "preview.nsi"

# The same charset flags Tauri passes, so a non-ASCII character that survives
# here survives the real build too.
& $makensis "-INPUTCHARSET" "UTF8" "-OUTPUTCHARSET" "UTF8" "/DIB_OUTFILE=$outFile" $script
if ($LASTEXITCODE -ne 0) { throw "makensis failed with exit code $LASTEXITCODE" }

Write-Host "built $outFile"
if (-not $NoRun) { Start-Process $outFile }
