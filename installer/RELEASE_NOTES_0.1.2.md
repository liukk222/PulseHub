# PulseHub v0.1.2

PulseHub v0.1.2 是面向 Windows 11 x64 与 **Logitech G102 LIGHTSYNC** 的生产优化版本，重点清理开发配置并降低后台托盘代理的空闲占用。

## 本次更新

- 首次运行不再包含任何开发应用环境，Word、POWERPNT 等测试环境均不会进入生产配置
- v0.1.2 首次载入旧配置时执行一次性安全迁移，清除历史开发环境并恢复生产默认配置；后续用户修改不会被重复重置
- Office、CS2 与退出配置默认统一为 1600 DPI
- 中键恢复为原生中键
- G4 恢复为后退，G5 恢复为前进
- G6 恢复为四档 DPI 循环
- 开发者日志默认关闭
- 托盘语言改为内存状态同步，不再每 500 ms 读取并解析配置文件
- 托盘监控调整为 1 秒，HID DPI 健康回采调整为 2 秒
- 前台应用切换继续由 Windows 事件即时触发，不受健康回采周期影响
- 增加生产默认配置回归测试

## Windows 11 生产构建

- Windows 11 x64（build 22000 或更高版本）
- Rust 目标：`x86_64-pc-windows-msvc`
- CPU 基线：`x86-64-v2`
- Release `opt-level = 3`、Thin LTO、单代码生成单元、`panic = "abort"`、符号剥离
- Inno Setup 简体中文与 English 双语安装器

## 安装与兼容性

下载 `PulseHub-Setup-0.1.2-windows-x64.exe`。建议安装和首次写入设备前退出 Logitech G HUB。

支持设备：Logitech G102 LIGHTSYNC（USB `046d:c092`）。其他设备和 Windows 版本尚未声明支持。

此安装器尚未进行商业代码签名，Windows SmartScreen 可能显示“未知发布者”。请仅从本 GitHub Release 下载，并使用同时提供的 `.sha256` 文件核对完整性。

PulseHub 自有源码采用 MIT License，是独立开源项目，与 Logitech 没有隶属、授权或背书关系。

---

# PulseHub v0.1.2

PulseHub v0.1.2 is a production-optimized release for Windows 11 x64 and the **Logitech G102 LIGHTSYNC**, focused on removing development configuration and reducing idle background-agent overhead.

## Changes

- Fresh installations no longer contain development application profiles; Word, POWERPNT, and other test profiles are excluded from production defaults
- The first v0.1.2 load performs a one-time safe migration of older configurations, removing historical development profiles and restoring production defaults; later user changes are not reset again
- Office, CS2, and exit profiles now default to 1600 DPI
- Middle click is restored to the native middle-button action
- G4 is restored to Back and G5 to Forward
- G6 is restored to native four-level DPI cycling
- Developer logging remains disabled by default
- Tray language now uses in-memory state synchronization instead of reading and parsing the configuration file every 500 ms
- Tray monitoring now runs every 1 second and HID DPI health polling every 2 seconds
- Foreground application switching remains event-driven and is unaffected by the health-poll interval
- Adds regression coverage for all production defaults

## Windows 11 production build

- Windows 11 x64 (build 22000 or later)
- Rust target: `x86_64-pc-windows-msvc`
- CPU baseline: `x86-64-v2`
- Release `opt-level = 3`, Thin LTO, one codegen unit, `panic = "abort"`, and stripped symbols
- Bilingual Simplified Chinese and English Inno Setup installer

## Installation and compatibility

Download `PulseHub-Setup-0.1.2-windows-x64.exe`. Exit Logitech G HUB before installation and before the first device write.

Supported device: Logitech G102 LIGHTSYNC (USB `046d:c092`). Other devices and Windows versions are not currently declared supported.

This installer is not commercially code-signed, so Windows SmartScreen may display “Unknown publisher.” Download it only from this GitHub Release and verify it with the accompanying `.sha256` file.

PulseHub source code is licensed under the MIT License. PulseHub is an independent open-source project and is not affiliated with, authorized by, or endorsed by Logitech.
