# PulseHub

[简体中文](README_ZH.md) | **English**

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev/) <img src="apps/pulsehub-config/ui/assets/tray-icon.svg" alt="PulseHub 托盘图标" height="210">

PulseHub is a lightweight, open-source mouse configuration application for Windows 11. Version 0.1.3 provides tested hardware control for the **Logitech G102 LIGHTSYNC**: DPI, report rate, button mappings, portable configuration transfer, application profiles, automatic profile switching, reliable sign-in startup, safe shutdown restoration, and a bilingual Simplified Chinese and English Slint interface.

PulseHub is an independent project. It is not affiliated with, authorized by, or endorsed by Logitech.

## Interface and features

PulseHub is designed for **Windows 11 x64** and the **Logitech G102 LIGHTSYNC** (`046d:c092`). The screenshots below describe the available controls directly.

### Application header

![PulseHub application header](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20193616.png)

The compact header provides the current device connection state and navigation to each configuration page. The background agent continues profile switching after the settings window is closed.

### Device overview

![PulseHub overview page](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20194042.jpg)

Check the connected mouse, reported capabilities, and current profile status here. Use **Reapply** to explicitly send the saved profile to the device; the operation performs hardware read-back verification.

### DPI and report rate

![PulseHub device settings](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20194146.jpg)

Each profile can set DPI, a custom DPI value within the mouse capability range, four DPI-cycle levels, and a report rate of 1000, 500, 250, or 125 Hz. Changes are validated before they are saved and applied.

### Button mapping

![PulseHub button mapping](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20194221.jpg)

Configure the middle button, G4, G5, and G6 as native mouse actions or supported keyboard shortcuts. Left and right click remain protected native actions. Mapping edits can be restored to their original mouse functions.

### Built-in application profiles

![PulseHub application profiles](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20194318.jpg)

Office and CS2 have independent DPI, report-rate, DPI-cycle, and button-mapping settings. This page also shows the profile editing workflow before saving or explicitly reapplying a configuration.

### Import profiles for your applications

![PulseHub profile import and configuration](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20194344.jpg)

Import an existing `.exe` file to create a dedicated profile for a game, design tool, or other application. Each imported application has separate device settings and can participate in automatic switching.

### Language support

![PulseHub Simplified Chinese and English interface](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20194432.jpg)

PulseHub provides Simplified Chinese and English interfaces. Choose the default display language in settings; the installer supports the same two languages.

### Preferences and safe exit

![PulseHub preferences and safe exit](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20194421.jpg)

Set optional sign-in startup, developer logging, and the safe-exit profile. Before the tray agent exits, the safe-exit profile restores the selected DPI, report rate, DPI-cycle level, and safe button mappings. Device lighting is intentionally unavailable: the supported G102 LIGHTSYNC lighting stays disabled.

## Download

Download the latest Windows installer from [GitHub Releases](https://github.com/liukk222/PulseHub/releases/latest):

- [PulseHub v0.1.3 Windows 11 x64 installer](https://github.com/liukk222/PulseHub/releases/download/v0.1.3/PulseHub-Setup-0.1.3-windows-x64.exe)
- [SHA-256 checksum file](https://github.com/liukk222/PulseHub/releases/download/v0.1.3/PulseHub-Setup-0.1.3-windows-x64.exe.sha256)

Verify the downloaded installer in PowerShell:

```powershell
Get-FileHash .\PulseHub-Setup-0.1.3-windows-x64.exe -Algorithm SHA256
```

Compare the output with the SHA-256 value in the accompanying `.sha256` file.

The v0.1.3 installer is not digitally signed. Windows SmartScreen may display an unknown-publisher warning. Download it only from this repository and verify the checksum before running it.

## Install

1. Exit Logitech G HUB to prevent both applications from controlling the mouse simultaneously.
2. Run `PulseHub-Setup-0.1.3-windows-x64.exe`.
3. Choose Simplified Chinese or English for the installer.
4. Read and accept the installation agreement and third-party notice.
5. Choose the installation directory and the default PulseHub interface language.
6. Start PulseHub and configure the Office, CS2, or imported application profiles.

The installer places the MIT license and third-party notices in the installation directory.

## Build from source

### Requirements

- Windows 11 x64
- Git
- Rust 1.97 or newer with the `x86_64-pc-windows-msvc` toolchain
- Microsoft C++ Build Tools for the MSVC linker and native dependencies
- PowerShell 7 is recommended

Clone and enter the repository:

```powershell
git clone https://github.com/liukk222/PulseHub.git
cd PulseHub
```

Validate and build the workspace:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

Build the optimized production binaries:

```powershell
cargo build --release -p pulsehub-agent -p pulsehub-config
```

The binaries are written to:

```text
target\release\pulsehub-agent.exe
target\release\pulsehub-config.exe
```

Run the GUI from source:

```powershell
cargo run -p pulsehub-config
```

PulseHub starts the background agent when required. Commands that perform device writes require explicit confirmation internally; development tools also expose confirmation flags to prevent accidental HID writes.

## Build the Windows installer

Install [Inno Setup 6](https://jrsoftware.org/isinfo.php), then run:

```powershell
winget install --id JRSoftware.InnoSetup -e
.\installer\build-installer.ps1
```

The script builds the Rust release binaries, verifies the pinned Simplified Chinese Inno Setup language file, extracts the PulseHub icon, and writes the single-file installer to `installer\output`.

To reuse existing release binaries:

```powershell
.\installer\build-installer.ps1 -SkipRustBuild
```

## Workspace layout

```text
apps/pulsehub-agent       Background device agent and system tray
apps/pulsehub-config      Slint configuration GUI
crates/pulsehub-device    HID discovery and Logitech HID++ implementation
crates/pulsehub-config-store
                          Configuration schema, validation, and atomic storage
crates/pulsehub-ipc       Named Pipe IPC protocol and Windows transport
crates/pulsehub-profile   Profile selection and switching logic
crates/pulsehub-ui        Shared Slint integration
tools/pulsehub-probe      Read-only discovery and explicitly confirmed test writes
installer                 Windows installer source and build script
docs                      Architecture and implementation documentation
```

Detailed architecture, HID++, IPC, configuration, GUI, testing, and release documentation starts at [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md).

## Hardware safety

PulseHub can write DPI, report rate, button mappings, lighting state, and onboard configuration to a physical mouse. Before changing HID++ code or running write tests:

- confirm the exact device identity;
- exit Logitech G HUB;
- keep read-back verification enabled;
- use explicit confirmation arguments in development tools;
- preserve native left and right clicks;
- avoid unnecessary onboard flash writes.

## License

PulseHub's original source code is licensed under the [MIT License](LICENSE). Third-party components and compatibility research references remain subject to their own terms. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and the [Windows dependency license audit](docs/DEPENDENCY_LICENSE_AUDIT.md).

Logitech, Logitech G, G102 LIGHTSYNC, and related product names and marks belong to their respective owners.
