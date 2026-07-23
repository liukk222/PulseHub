# PulseHub v0.1.3

## 简体中文

PulseHub v0.1.3 是面向 **Windows 11 x64** 与 **Logitech G102 LIGHTSYNC** 的性能与发布维护版本。安装器、安装协议、第三方声明和应用界面均同时提供简体中文与 English。

### 主要更新

- 托盘语言状态改为代理内存原子同步；托盘不再每 500ms 读取和解析 `config.toml`。
- 托盘状态监控调整为每 1 秒执行一次，仅读取内存状态。
- HID DPI 健康回采调整为每 2 秒执行一次；连续 3 次失败才确认设备异常，最长确认时间约 6 秒。
- Windows 前台应用切换继续由系统前台事件即时驱动，不依赖上述定时器。
- Windows 11 x64 通用安装包使用 `x86-64-v2` CPU 基线编译，以覆盖 Windows 11 支持的现代 x64 处理器，而不是绑定单一设备的 `native` 指令集。

### 安装与验证

下载 `PulseHub-Setup-0.1.3-windows-x64.exe` 及同名 `.sha256` 文件。安装向导、安装与使用协议、第三方声明和应用界面均提供简体中文与 English。

Windows 11 x64 是当前支持平台。安装包尚未进行商业代码签名，SmartScreen 可能显示“未知发布者”；请仅从本 GitHub Release 下载，并在运行前核对 SHA-256。

---

## English

PulseHub v0.1.3 is a performance and release-maintenance update for **Windows 11 x64** and the **Logitech G102 LIGHTSYNC**. The installer, installation agreement, third-party notice, and application interface are available in both Simplified Chinese and English.

### Highlights

- Tray language state now synchronizes through an in-memory atomic value; the tray no longer reads and parses `config.toml` every 500ms.
- Tray monitoring now runs once per second and reads only in-memory state.
- HID DPI health read-back now runs every two seconds. A device is confirmed unhealthy only after three consecutive failures, for a maximum confirmation time of about six seconds.
- Windows foreground-application switching remains immediately event-driven by system foreground events and does not depend on these timers.
- The universal Windows 11 x64 installer is compiled with the `x86-64-v2` CPU baseline to cover modern x64 processors supported by Windows 11, rather than being tied to one machine's `native` instruction set.

### Installation and verification

Download `PulseHub-Setup-0.1.3-windows-x64.exe` and its matching `.sha256` file. The setup wizard, installation and usage agreement, third-party notice, and application interface are available in both Simplified Chinese and English.

Windows 11 x64 is the currently supported platform. This installer is not commercially code-signed, so SmartScreen may show an “Unknown publisher” warning. Download only from this GitHub Release and verify the SHA-256 before running it.
