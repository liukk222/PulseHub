[CmdletBinding()]
param(
    [switch]$SkipRustBuild
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$buildDir = Join-Path $PSScriptRoot 'build'
$releaseDir = Join-Path $repoRoot 'target\release'
$configExe = Join-Path $releaseDir 'pulsehub-config.exe'
$agentExe = Join-Path $releaseDir 'pulsehub-agent.exe'
$isccCandidates = @(
    (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
    (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe')
)
$iscc = $isccCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $iscc) {
    throw '未找到 Inno Setup 6。请先安装 JRSoftware.InnoSetup。'
}

if (-not $SkipRustBuild) {
    & cargo build --release -p pulsehub-agent -p pulsehub-config
    if ($LASTEXITCODE -ne 0) { throw 'Rust Release 构建失败。' }
}
if (-not (Test-Path -LiteralPath $configExe) -or -not (Test-Path -LiteralPath $agentExe)) {
    throw '缺少 Release 可执行文件。'
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
    throw "Inno Setup 简体中文语言文件校验失败：$languageHash"
}
Add-Type -AssemblyName System.Drawing
$icon = [System.Drawing.Icon]::ExtractAssociatedIcon($configExe)
if (-not $icon) { throw '无法从 pulsehub-config.exe 提取程序图标。' }
$iconPath = Join-Path $buildDir 'PulseHub.ico'
$stream = [System.IO.File]::Create($iconPath)
try { $icon.Save($stream) } finally { $stream.Dispose(); $icon.Dispose() }

& $iscc (Join-Path $PSScriptRoot 'PulseHub.iss')
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup 构建失败。' }

$installer = Get-ChildItem (Join-Path $PSScriptRoot 'output') -Filter 'PulseHub-Setup-*.exe' |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
$hash = Get-FileHash -Algorithm SHA256 -LiteralPath $installer.FullName
Write-Host "安装器：$($installer.FullName)"
Write-Host "SHA256：$($hash.Hash)"
