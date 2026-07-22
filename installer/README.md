# PulseHub Windows 11 installer

[简体中文](README_ZH.md) | **English**

This directory contains the Inno Setup source and PowerShell build script for the PulseHub Windows 11 x64 installer.

## Installer flow

The generated installer uses the following page order:

1. Choose the setup language: Simplified Chinese or English.
2. Read and accept the bilingual installation and use agreement. Setup cannot continue unless the agreement is accepted.
3. Read the third-party and compatibility research notice.
4. Choose the installation directory.
5. Choose the default PulseHub interface language.
6. Install PulseHub and optionally launch it.

## Requirements

- Windows 11 x64
- PowerShell 7 recommended
- Rust 1.97 or newer with the `x86_64-pc-windows-msvc` toolchain
- Microsoft C++ Build Tools
- [Inno Setup 6](https://jrsoftware.org/isinfo.php)

Install Inno Setup with Windows Package Manager:

```powershell
winget install --id JRSoftware.InnoSetup -e
```

## Build

Run the build script from the repository root:

```powershell
.\installer\build-installer.ps1
```

The script:

1. builds optimized `pulsehub-agent.exe` and `pulsehub-config.exe` binaries;
2. downloads the Simplified Chinese Inno Setup language file from the official source repository when it is not cached;
3. verifies that language file against a pinned SHA-256 value;
4. extracts the orange PulseHub P icon from the GUI executable;
5. invokes the Inno Setup command-line compiler;
6. prints the installer path and SHA-256 value.

The installer is written to:

```text
installer\output\PulseHub-Setup-0.1.0-windows-x64.exe
```

`installer\build` and `installer\output` are generated directories and are ignored by Git.

## Reuse existing release binaries

If the required Rust release binaries already exist under `target\release`, skip the Rust build:

```powershell
.\installer\build-installer.ps1 -SkipRustBuild
```

The script stops with an error if either required executable is missing.

## Packaged notices

The installer includes:

- `LICENSE-AGREEMENT.txt`: bilingual installation risk and use agreement;
- `THIRD_PARTY_NOTICES.txt`: bilingual installer compatibility notice;
- root `LICENSE`: PulseHub MIT License;
- root `THIRD_PARTY_NOTICES.md`: complete project third-party notices.

The installation agreement does not replace, restrict, or modify the source-code rights granted by the MIT License.

## Release verification

After building, verify the installer:

```powershell
Get-FileHash .\installer\output\PulseHub-Setup-0.1.0-windows-x64.exe -Algorithm SHA256
Get-AuthenticodeSignature .\installer\output\PulseHub-Setup-0.1.0-windows-x64.exe
```

The open-source v0.1.0 installer is not digitally signed. Windows SmartScreen may display an unknown-publisher warning. A public release should always include a separately uploaded SHA-256 checksum file.

## Files

```text
PulseHub.iss                Inno Setup definition
build-installer.ps1         Reproducible installer build script
LICENSE-AGREEMENT.txt       Bilingual installation agreement
THIRD_PARTY_NOTICES.txt     Bilingual installer compatibility notice
README.md                   English documentation
README_ZH.md                Simplified Chinese documentation
```
