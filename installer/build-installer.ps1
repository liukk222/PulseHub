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

Add-Type -AssemblyName System.Drawing
$icon = [System.Drawing.Icon]::ExtractAssociatedIcon($configExe)
if (-not $icon) { throw 'Could not extract an icon from pulsehub-config.exe.' }
$iconPath = Join-Path $buildDir 'PulseHub.ico'
$stream = [System.IO.File]::Create($iconPath)
try { $icon.Save($stream) } finally { $stream.Dispose(); $icon.Dispose() }

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
