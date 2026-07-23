# PulseHub v0.1.4

## 简体中文

PulseHub v0.1.4 是面向 **Windows 11 x64** 与 **Logitech G102 LIGHTSYNC** 的安装体验与发布维护版本。安装器、安装协议、第三方声明和应用界面均同时提供简体中文与 English。

### 主要更新

- 安装包、开始菜单快捷方式和 Windows 程序条目统一使用由 Slint `tray-icon.svg` 生成的 PulseHub 托盘程序图标。
- 安装器在打包前校验托盘 SVG 源文件，并生成多尺寸 ICO，避免图标来源悄然偏离 GUI 和 agent 托盘。
- 继续使用 Windows 11 x64 的 `x86-64-v2` 通用 CPU 基线编译，不绑定某一台机器的 `native` 指令集。
- 安装向导、安装与使用协议、第三方声明和应用界面继续同时提供简体中文与 English。

### 安装与验证

下载 `PulseHub-Setup-0.1.4-windows-x64.exe` 及同名 `.sha256` 文件。Windows 11 x64 是当前支持平台。安装包尚未进行商业代码签名，SmartScreen 可能显示“未知发布者”；请仅从本 GitHub Release 下载，并在运行前核对 SHA-256。

---

## English

PulseHub v0.1.4 is an installer-experience and release-maintenance update for **Windows 11 x64** and the **Logitech G102 LIGHTSYNC**. The installer, installation agreement, third-party notice, and application interface are available in both Simplified Chinese and English.

### Highlights

- The installer, Start Menu shortcut, and Windows program entry consistently use the PulseHub tray-program icon generated from Slint `tray-icon.svg`.
- Before packaging, the installer verifies the tray SVG source and generates a multi-size ICO, preventing silent divergence between the GUI/agent tray and the program icon.
- The build continues to use the universal Windows 11 x64 `x86-64-v2` CPU baseline rather than a machine-specific `native` instruction set.
- The setup wizard, installation and usage agreement, third-party notice, and application interface continue to provide both Simplified Chinese and English.

### Installation and verification

Download `PulseHub-Setup-0.1.4-windows-x64.exe` and its matching `.sha256` file. Windows 11 x64 is the currently supported platform. This installer is not commercially code-signed, so SmartScreen may show an “Unknown publisher” warning. Download only from this GitHub Release and verify the SHA-256 before running it.
