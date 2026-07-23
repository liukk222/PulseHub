[CmdletBinding()]
param(
    [switch]$SkipRustBuild
)

$ErrorActionPreference = 'Stop'
$target = 'x86_64-pc-windows-msvc'
$cpuBaseline = 'x86-64-v2'
$repoRoot = Split-Path -Parent $PSScriptRoot
$buildDir = Join-Path $PSScriptRoot 'build'
$releaseDir = Join-Path $repoRoot "target\$target\release"
$configExe = Join-Path $releaseDir 'pulsehub-config.exe'
$agentExe = Join-Path $releaseDir 'pulsehub-agent.exe'
$trayIconSource = Join-Path $repoRoot 'apps\pulsehub-config\ui\assets\tray-icon.svg'
$isccCandidates = @(
    (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
    (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe')
)
$iscc = $isccCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $iscc) {
    throw 'Inno Setup 6 was not found. Install JRSoftware.InnoSetup first.'
}

if (-not $SkipRustBuild) {
    if ($env:OS -ne 'Windows_NT') {
        throw 'The installer can only be built on Windows 11.'
    }
    $osBuild = [Environment]::OSVersion.Version.Build
    if ($osBuild -lt 22000) {
        throw "Windows 11 build 22000 or later is required; current build: $osBuild"
    }
    Write-Host "Rust target: $target"
    Write-Host "Windows 11 x64 CPU baseline: $cpuBaseline"
    $previousRustFlags = $env:RUSTFLAGS
    try {
        $env:RUSTFLAGS = "-C target-cpu=$cpuBaseline"
        & cargo build --release --locked --target $target -p pulsehub-agent -p pulsehub-config
        if ($LASTEXITCODE -ne 0) { throw 'Rust release build failed.' }
    }
    finally {
        $env:RUSTFLAGS = $previousRustFlags
    }
}

if (-not (Test-Path -LiteralPath $configExe) -or -not (Test-Path -LiteralPath $agentExe)) {
    throw 'Required release executable is missing.'
}

New-Item -ItemType Directory -Path $buildDir -Force | Out-Null
$chineseLanguageFile = Join-Path $buildDir 'ChineseSimplified.isl'
$expectedLanguageHash = '6753BE2C5E2740D859900FD902824DB2EC568DA5C5B52486524C9762D778B0B0'
if (-not (Test-Path -LiteralPath $chineseLanguageFile)) {
    $languageSource = 'https://raw.githubusercontent.com/jrsoftware/issrc/main/Files/Languages/ChineseSimplified.isl'
    Invoke-WebRequest -Uri $languageSource -OutFile $chineseLanguageFile
}
$languageHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $chineseLanguageFile).Hash
if ($languageHash -ne $expectedLanguageHash) {
    throw "Inno Setup Simplified Chinese language file verification failed: $languageHash"
}

if (-not (Test-Path -LiteralPath $trayIconSource)) {
    throw "PulseHub tray icon source is missing: $trayIconSource"
}

# tray-icon.svg is intentionally the sole artwork source for the installer icon.
# It currently contains only the validated 32×32 PulseHub circle-and-P geometry below.
# Fail closed if the source artwork changes, rather than silently shipping stale branding.
$expectedTrayIcon = @'
<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32">
  <circle cx="16" cy="16" r="15" fill="#dc6547"/>
  <path d="M10 7h7.5c5 0 8 2.8 8 7.1 0 4.4-3 7.2-8 7.2h-3.1V26H10V7zm4.4 3.8v6.7h2.8c2.5 0 3.8-1.2 3.8-3.4 0-2.1-1.3-3.3-3.8-3.3h-2.8z" fill="#fff"/>
</svg>
'@.Trim()
$actualTrayIcon = (Get-Content -LiteralPath $trayIconSource -Raw).Replace("`r`n", "`n").Trim()
if ($actualTrayIcon -ne $expectedTrayIcon) {
    throw 'tray-icon.svg changed. Update the validated SVG-to-ICO renderer before building the installer.'
}

Add-Type -AssemblyName System.Drawing
$iconPath = Join-Path $buildDir 'PulseHub.ico'
$iconSizes = @(16, 20, 24, 32, 40, 48, 64, 256)
$pngImages = [System.Collections.Generic.List[byte[]]]::new()
foreach ($size in $iconSizes) {
    $bitmap = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try {
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
            $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.Clear([System.Drawing.Color]::Transparent)
            $scale = $size / 32.0
            $backgroundBrush = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml('#dc6547'))
            try {
                $graphics.FillEllipse($backgroundBrush, 1 * $scale, 1 * $scale, 30 * $scale, 30 * $scale)
            } finally { $backgroundBrush.Dispose() }
            $path = [System.Drawing.Drawing2D.GraphicsPath]::new([System.Drawing.Drawing2D.FillMode]::Alternate)
            try {
                $path.StartFigure()
                $path.AddLine(10 * $scale, 7 * $scale, 17.5 * $scale, 7 * $scale)
                $path.AddBezier(17.5 * $scale, 7 * $scale, 22.5 * $scale, 7 * $scale, 25.5 * $scale, 9.8 * $scale, 25.5 * $scale, 14.1 * $scale)
                $path.AddBezier(25.5 * $scale, 14.1 * $scale, 25.5 * $scale, 18.5 * $scale, 22.5 * $scale, 21.3 * $scale, 17.5 * $scale, 21.3 * $scale)
                $path.AddLine(17.5 * $scale, 21.3 * $scale, 14.4 * $scale, 21.3 * $scale)
                $path.AddLine(14.4 * $scale, 21.3 * $scale, 14.4 * $scale, 26 * $scale)
                $path.AddLine(14.4 * $scale, 26 * $scale, 10 * $scale, 26 * $scale)
                $path.CloseFigure()
                $path.StartFigure()
                $path.AddLine(14.4 * $scale, 10.8 * $scale, 14.4 * $scale, 17.5 * $scale)
                $path.AddLine(14.4 * $scale, 17.5 * $scale, 17.2 * $scale, 17.5 * $scale)
                $path.AddBezier(17.2 * $scale, 17.5 * $scale, 19.7 * $scale, 17.5 * $scale, 21 * $scale, 16.3 * $scale, 21 * $scale, 14.1 * $scale)
                $path.AddBezier(21 * $scale, 14.1 * $scale, 21 * $scale, 12 * $scale, 19.7 * $scale, 10.8 * $scale, 17.2 * $scale, 10.8 * $scale)
                $path.CloseFigure()
                $graphics.FillPath([System.Drawing.Brushes]::White, $path)
            } finally { $path.Dispose() }
        } finally { $graphics.Dispose() }
        $stream = [System.IO.MemoryStream]::new()
        try {
            $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
            $pngImages.Add($stream.ToArray())
        } finally { $stream.Dispose() }
    } finally { $bitmap.Dispose() }
}

$writer = [System.IO.BinaryWriter]::new([System.IO.File]::Open($iconPath, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write))
try {
    $writer.Write([UInt16]0); $writer.Write([UInt16]1); $writer.Write([UInt16]$pngImages.Count)
    $offset = 6 + (16 * $pngImages.Count)
    for ($index = 0; $index -lt $pngImages.Count; $index++) {
        $size = $iconSizes[$index]
        $writer.Write([byte]($(if ($size -eq 256) { 0 } else { $size })))
        $writer.Write([byte]($(if ($size -eq 256) { 0 } else { $size })))
        $writer.Write([byte]0); $writer.Write([byte]0); $writer.Write([UInt16]1); $writer.Write([UInt16]32)
        $writer.Write([UInt32]$pngImages[$index].Length); $writer.Write([UInt32]$offset)
        $offset += $pngImages[$index].Length
    }
    foreach ($image in $pngImages) { $writer.Write($image) }
} finally { $writer.Dispose() }

& $iscc (Join-Path $PSScriptRoot 'PulseHub.iss')
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup build failed.' }

$installer = Get-ChildItem (Join-Path $PSScriptRoot 'output') -Filter 'PulseHub-Setup-*.exe' |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
if (-not $installer) {
    throw 'Inno Setup did not produce an installer.'
}
$hash = Get-FileHash -Algorithm SHA256 -LiteralPath $installer.FullName
$hashFile = "$($installer.FullName).sha256"
$hashLine = "$($hash.Hash)  $($installer.Name)"
[System.IO.File]::WriteAllText($hashFile, "$hashLine`r`n", [System.Text.Encoding]::ASCII)
Write-Host "Installer: $($installer.FullName)"
Write-Host "SHA256: $($hash.Hash)"
Write-Host "Checksum file: $hashFile"
