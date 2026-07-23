# PulseHub v0.1.2

## 简体中文

PulseHub v0.1.2 是面向 Windows 11 x64 与 **Logitech G102 LIGHTSYNC** 的维护版本。本版本修复了“登录时启动 PulseHub”在部分 Windows 11 系统上无法生效的问题；安装器和应用继续同时提供简体中文与 English。

### 修复内容

- 修复安装器此前创建空 `HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\\PulseHub` 值的问题；现在会写入带引号的 `pulsehub-agent.exe --run-agent --confirm-device-write` 启动命令。
- 修复 Windows Explorer `StartupApproved\\Run` 中遗留禁用状态导致 Run 启动项被跳过的问题。
- 启用“登录时启动”后，后台代理会同步 Run 命令并清除 PulseHub 的旧禁用记录；关闭该选项时会清理关联状态。
- 增加启动命令格式的回归测试。

### 安装与验证

下载 `PulseHub-Setup-0.1.2-windows-x64.exe` 和同名 `.sha256` 文件。安装向导、安装与使用协议、第三方声明和应用界面均提供简体中文与 English。

Windows 11 x64 为当前支持平台。安装器尚未进行商业代码签名，SmartScreen 可能显示“未知发布者”；请仅从本 GitHub Release 下载，并在运行前核对 SHA-256。

---

## English

PulseHub v0.1.2 is a maintenance release for Windows 11 x64 and the **Logitech G102 LIGHTSYNC**. It fixes sign-in startup on affected Windows 11 systems while retaining Simplified Chinese and English throughout the installer and application.

### Fixes

- Fixes an installer issue that could create an empty `HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\\PulseHub` value. The value now contains a quoted `pulsehub-agent.exe --run-agent --confirm-device-write` command.
- Fixes a stale Windows Explorer `StartupApproved\\Run` disabled state that could cause Windows to skip the Run entry.
- When “start at sign-in” is enabled, the background agent synchronizes the Run command and removes the stale PulseHub disabled state; disabling the option also cleans up associated state.
- Adds regression coverage for the startup command format.

### Installation and verification

Download `PulseHub-Setup-0.1.2-windows-x64.exe` and its matching `.sha256` file. The setup wizard, installation and usage agreement, third-party notice, and application interface are available in both Simplified Chinese and English.

Windows 11 x64 is the currently supported platform. This installer is not commercially code-signed, so SmartScreen may show an “Unknown publisher” warning. Download only from this GitHub Release and verify the SHA-256 before running it.
