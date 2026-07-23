# PulseHub 实现文档

> 更新日期：2026-07-21
> 文档状态：持续实现与实机验证

原单文件实现文档已按主题拆分。本文件只作为稳定入口，详细内容位于 [implementation](./implementation/) 目录。

## 阅读顺序

1. [目标与架构](./implementation/01-overview-and-architecture.md)
2. [工程结构与模块职责](./implementation/02-workspace-and-modules.md)
3. [代理与环境切换](./implementation/03-agent-and-profile-switching.md)
4. [G102 HID 与 HID++ 实现](./implementation/04-g102-hid.md)
5. [配置与 IPC](./implementation/05-configuration-and-ipc.md)
6. [GUI、Windows 集成与可观测性](./implementation/06-gui-windows-and-observability.md)
7. [构建、测试与路线图](./implementation/07-build-test-and-roadmap.md)

## 按任务查阅

| 任务 | 文档 |
|---|---|
| 理解总体架构和进程边界 | [01-overview-and-architecture.md](./implementation/01-overview-and-architecture.md) |
| 修改 crate 或依赖关系 | [02-workspace-and-modules.md](./implementation/02-workspace-and-modules.md) |
| 修改后台代理、自动切换或恢复逻辑 | [03-agent-and-profile-switching.md](./implementation/03-agent-and-profile-switching.md) |
| 修改 G102 DPI、按键或板载闪存 | [04-g102-hid.md](./implementation/04-g102-hid.md) |
| 修改 config.toml 或 Named Pipe IPC | [05-configuration-and-ipc.md](./implementation/05-configuration-and-ipc.md) |
| 修改 Slint GUI、托盘或日志 | [06-gui-windows-and-observability.md](./implementation/06-gui-windows-and-observability.md) |
| 执行发布、测试或规划后续阶段 | [07-build-test-and-roadmap.md](./implementation/07-build-test-and-roadmap.md) |



