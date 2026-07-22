# PulseHub

[简体中文](README_ZH.md) | **English**

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev/)

PulseHub is a lightweight, open-source mouse configuration application for Windows 11. Version 0.1.0 provides tested hardware control for the **Logitech G102 LIGHTSYNC**: DPI, report rate, button mappings, application profiles, automatic profile switching, safe shutdown restoration, and a bilingual Slint interface.

PulseHub is an independent project. It is not affiliated with, authorized by, or endorsed by Logitech.

## Download

Download the latest Windows installer from [GitHub Releases](https://github.com/liukk222/PulseHub/releases/latest):

- [PulseHub v0.1.0 Windows 11 x64 installer](https://github.com/liukk222/PulseHub/releases/download/v0.1.0/PulseHub-Setup-0.1.0-windows-x64.exe)
- [SHA-256 checksum file](https://github.com/liukk222/PulseHub/releases/download/v0.1.0/PulseHub-Setup-0.1.0-windows-x64.exe.sha256)

Verify the downloaded installer in PowerShell:

```powershell
Get-FileHash .\PulseHub-Setup-0.1.0-windows-x64.exe -Algorithm SHA256
```

Expected SHA-256:

```text
1B5D06DF1E35BAAD81F2EC68F0808AAD6BCA42E9549F574C415E0611AE67F1D8
```

The v0.1.0 installer is not digitally signed. Windows SmartScreen may display an unknown-publisher warning. Download it only from this repository and verify the checksum before running it.

## Supported platform and device

- Windows 11 x64
- Logitech G102 LIGHTSYNC, USB ID `046d:c092`
- Rust source builds use the MSVC toolchain

Other mouse models and operating systems are not declared supported by v0.1.0.

## Features

- Real HID/HID++ device discovery, capability queries, writes, and read-back verification
- Configurable DPI and native four-level DPI cycling
- Fixed report-rate choices: 1000, 500, 250, or 125 Hz
- Native actions or keyboard shortcuts for the middle button, G4, G5, and G6
- Protected native left and right clicks
- Office, CS2, and user-imported EXE profiles
- Automatic foreground-application switching or a fixed profile mode
- Device reconnect recovery and bounded retry behavior
- Lightweight background agent and system tray; closing the GUI does not stop profile switching
- User-configurable safe-exit profile with hardware read-back verification
- Optional start at sign-in and developer logs; developer logs are off by default
- Simplified Chinese and English interfaces
- Logitech G102 LIGHTSYNC lighting is always disabled and is not configurable

## Install

1. Exit Logitech G HUB to prevent both applications from controlling the mouse simultaneously.
2. Run `PulseHub-Setup-0.1.0-windows-x64.exe`.
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
