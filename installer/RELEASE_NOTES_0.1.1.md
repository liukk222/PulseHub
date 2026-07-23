# PulseHub v0.1.1

PulseHub 面向 Windows 11 与 **Logitech G102 LIGHTSYNC** 的开源配置工具。本版本增加可移植的配置导入与导出，并继续提供简体中文与 English 双语安装器和应用界面。

## 主要功能

- 新增配置导入与导出，覆盖 Office、CS2、退出配置、切换规则及用户导入的应用环境
- 跨机器导入时保留新机器的登录启动、开发者日志和界面语言偏好
- 自动模式按 EXE 文件名匹配；安装路径可以不同，但只有文件名相同才能保证自动切换正常
- 真实 HID/HID++ DPI、回报率和按键映射读写
- Office、CS2 与用户导入 EXE 环境自动切换
- 支持固定模式和自动模式
- DPI 原生四档循环与自定义 DPI
- 中键、G4、G5、G6 原生动作或键盘快捷键映射
- 关闭 GUI 后由轻量后台代理继续运行
- 系统托盘、登录时静默启动和安全退出恢复配置
- Logitech G102 LIGHTSYNC 灯光固定关闭
- 简体中文与 English 界面
- 可选开发者日志，默认关闭

## 安装

下载 `PulseHub-Setup-0.1.1-windows-x64.exe`：

1. 选择安装向导语言：简体中文或 English。
2. 阅读并接受中英双语安装与使用协议。
3. 阅读第三方及兼容性研究声明。
4. 选择安装目录。
5. 选择 PulseHub 默认界面语言。
6. 完成安装并启动 PulseHub。

建议安装和首次写入设备前退出 Logitech G HUB。

## 系统与设备

- Windows 11 x64（build 22000 或更高版本）
- Logitech G102 LIGHTSYNC（USB `046d:c092`）

其他设备和 Windows 版本尚未声明支持。

## 开源、完整性与 SmartScreen

PulseHub 自有源码采用 MIT License。安装目录包含 MIT `LICENSE`、第三方声明以及安装风险协议。PulseHub 是独立开源项目，与 Logitech 没有隶属、授权或背书关系。

本版本安装器尚未进行商业代码签名，Windows SmartScreen 可能显示“未知发布者”。请仅从本 GitHub Release 下载，并使用同时提供的 `.sha256` 文件核对 SHA-256。

---

# PulseHub v0.1.1

PulseHub is an open-source configuration utility for Windows 11 and the **Logitech G102 LIGHTSYNC**. This release adds portable configuration import and export while continuing to provide a bilingual Simplified Chinese and English installer and application interface.

## Highlights

- Adds configuration import and export for Office, CS2, exit profiles, switching rules, and user-imported application profiles
- Keeps the destination computer's startup, developer logging, and UI language preferences during import
- Automatic mode matches EXE filenames; installation paths may differ, but matching filenames are required to guarantee automatic switching
- Real HID/HID++ DPI, report-rate, and button-mapping access with read-back verification
- Automatic switching among Office, CS2, and imported EXE profiles
- Fixed and automatic selection modes
- Native four-level DPI cycling and custom DPI values
- Native actions or keyboard shortcuts for middle click, G4, G5, and G6
- Lightweight background agent continues after the GUI closes
- System tray, silent sign-in startup, and safe exit restoration
- Logitech G102 LIGHTSYNC lighting remains disabled
- Simplified Chinese and English interface
- Optional developer logging, disabled by default

## Installation

Download `PulseHub-Setup-0.1.1-windows-x64.exe`:

1. Choose Simplified Chinese or English for the setup wizard.
2. Read and accept the bilingual installation and usage agreement.
3. Review the third-party and compatibility research notice.
4. Choose the installation directory.
5. Choose the default PulseHub interface language.
6. Finish installation and launch PulseHub.

Exit Logitech G HUB before installation and before the first device write.

## System and device

- Windows 11 x64 (build 22000 or later)
- Logitech G102 LIGHTSYNC (USB `046d:c092`)

Other devices and Windows versions are not currently declared supported.

## Open source, integrity, and SmartScreen

PulseHub source code is licensed under the MIT License. The installation directory includes the MIT `LICENSE`, third-party notices, and the installation risk agreement. PulseHub is an independent open-source project and is not affiliated with, authorized by, or endorsed by Logitech.

This installer is not commercially code-signed, so Windows SmartScreen may display “Unknown publisher.” Download it only from this GitHub Release and verify its SHA-256 with the accompanying `.sha256` file.
