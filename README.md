# PulseHub

[简体中文](README_ZH.md) | **English**

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev/)

PulseHub is a lightweight, open-source mouse configuration application for Windows 11. Version 0.1.2 provides tested hardware control for the **Logitech G102 LIGHTSYNC**: DPI, report rate, button mappings, portable configuration transfer, application profiles, automatic profile switching, safe shutdown restoration, and a bilingual Slint interface.

PulseHub is an independent project. It is not affiliated with, authorized by, or endorsed by Logitech.

## Download

Download the latest Windows installer from [GitHub Releases](https://github.com/liukk222/PulseHub/releases/latest):

- [PulseHub v0.1.2 Windows 11 x64 installer](https://github.com/liukk222/PulseHub/releases/download/v0.1.2/PulseHub-Setup-0.1.2-windows-x64.exe)
- [SHA-256 checksum file](https://github.com/liukk222/PulseHub/releases/download/v0.1.2/PulseHub-Setup-0.1.2-windows-x64.exe.sha256)

Verify the downloaded installer in PowerShell:

```powershell
Get-FileHash .\PulseHub-Setup-0.1.2-windows-x64.exe -Algorithm SHA256
```

Expected SHA-256:

```text
006AE69333DE4FC5547022D5FDFC473C79AD8217B9923F20576BF9B4B8BB08E3
```

The v0.1.2 installer is not digitally signed. Windows SmartScreen may display an unknown-publisher warning. Download it only from this repository and verify the checksum before running it.

## Supported platform and device

- Windows 11 x64
- Logitech G102 LIGHTSYNC, USB ID `046d:c092`
- Rust source builds use the MSVC toolchain

Other mouse models and operating systems are not declared supported by v0.1.2.

## Features

- Real HID/HID++ device discovery, capability queries, writes, and read-back verification
- Configurable DPI and native four-level DPI cycling
- Fixed report-rate choices: 1000, 500, 250, or 125 Hz
- Native actions or keyboard shortcuts for the middle button, G4, G5, and G6
- Protected native left and right clicks
- Independent Office, CS2, and user-imported EXE profiles, each with its own pointer speed and button mappings
- Portable configuration import and export for Office, CS2, exit, switching-rule, and imported application profiles
- Automatic pointer-speed and button-mapping switching based on the foreground application, or a fixed profile mode
- Device reconnect recovery and bounded retry behavior
- Lightweight background agent and system tray; closing the GUI does not stop profile switching
- User-configurable safe-exit profile with hardware read-back verification
- Optional start at sign-in and developer logs; developer logs are off by default
- Simplified Chinese and English interfaces
- Device lighting control is not supported; Logitech G102 LIGHTSYNC lighting is always disabled and cannot be changed

## Built for efficiency

PulseHub concentrates real-time device control in `pulsehub-agent.exe`. The agent owns the system tray, foreground-application detection, profile switching, device reconnect recovery, and HID++ communication. `pulsehub-config.exe` runs only while the settings window is open. Closing the GUI terminates the configuration process while leaving a single lightweight background agent running, so automatic switching continues uninterrupted.

Production builds use compiler optimization, Thin LTO, a single code-generation unit, `panic = "abort"`, and stripped symbols. In a real Windows 11 test for this project, Task Manager reported **approximately 0.9 MB of memory** for an idle `pulsehub-agent.exe` after the GUI was closed and developer logging was disabled. This is an observed result from one test environment, not a fixed guarantee for every Windows version, driver, or runtime state. Device reconnection, profile switching, and differences in system accounting can change the instantaneous value.

The goal is straightforward: the full GUI should not remain resident during everyday use. Only the device agent stays quietly in the tray. Open the GUI from the tray when settings need to change, then close it again and let the low-overhead agent continue working.

### Import a dedicated profile for each application

The App profiles page accepts the full path to any EXE, including Word, PowerPoint, design tools, and games. Every imported application can have independent DPI, report-rate, and button-mapping settings:

1. Import the target application's EXE.
2. Configure the preferred pointer speed and buttons.
3. Select Auto mode so PulseHub can identify the foreground application.
4. When the target application becomes active, PulseHub applies its profile automatically; when it loses focus, PulseHub restores the matching Office or other application profile.

This avoids repeatedly changing mouse speed by hand when moving between applications. Fixed mode can also keep Office, CS2, or any imported application profile active at all times.

### No lighting control

PulseHub focuses on mouse performance, button mappings, and application-aware profile switching. It **does not provide RGB or device-lighting controls**. Lighting on the supported Logitech G102 LIGHTSYNC is kept disabled, and the GUI has no color, brightness, or animation controls and no option to remove this restriction. Users who need lighting support can build it themselves from this MIT-licensed open-source project; the official v0.1.2 scope does not include lighting configuration.

## Install

1. Exit Logitech G HUB to prevent both applications from controlling the mouse simultaneously.
2. Run `PulseHub-Setup-0.1.2-windows-x64.exe`.
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
