# PulseHub：目标与架构

> 本文由原 `docs/IMPLEMENTATION.md` 拆分，涵盖文档定位、目标边界与总体架构。

# PulseHub 实现文档

> 文档版本：0.1  
> 更新日期：2026-07-21
> 目标平台：Windows 11 专业版 x64  
> 文档状态：持续实现与实机验证

## 1. 文档定位

PulseHub 是一个使用 Rust 开发的轻量鼠标配置程序。第一阶段面向 Logitech G102 LIGHTSYNC，提供真实硬件 DPI 设置、鼠标按键分配，以及“办公”和“CS2”两套配置的自动切换。

当前工作区已实现 Cargo Workspace、领域模型、设备接口、配置切换、配置存储、IPC、Slint GUI 和系统托盘入口。Windows HID 枚举、HID++ 功能发现、DPI 与板载按键配置读写、设备拔插恢复、Office/CS2 自动切换，以及关闭配置窗口后由托盘重新打开 GUI，均已通过 G102 LIGHTSYNC 实机验证。IPC v1 使用版本化 DTO、长度前缀帧和 Windows Named Pipe，管道名称及受保护 DACL 绑定精确 TokenLogonSid 并拒绝远程客户端。未明确标为实机验证的性能数字和协议细节仍属于待验证目标。

本文使用以下状态词：

- **确定需求**：来自现有需求，可直接作为产品范围。
- **设计基线**：本文选定的实现方案，编码时默认遵循。
- **待验证**：必须通过 G102 LIGHTSYNC 实机或性能测试确认。
- **暂不实现**：不属于 MVP。

## 2. 目标与边界

### 2.1 功能目标

- 在 Windows 11 专业版上运行。
- 修改鼠标传感器的真实 DPI，不修改 Windows 指针速度来模拟 DPI。
- 为可编程鼠标按键设置一对一动作。
- 提供 `office` 与 `cs2` 两套配置。
- 当前台窗口属于 `cs2.exe` 时应用 CS2 配置；离开后恢复办公配置。
- 鼠标重连、Windows 睡眠恢复后，重新应用当前应生效的配置。
- 通过系统托盘打开配置界面、查看状态和退出常驻代理。

### 2.2 非功能目标

| 指标 | MVP 目标 | 验证条件 |
|---|---:|---|
| 代理空闲 CPU | 五分钟平均值小于 `0.1%` | Release 构建、GUI 已退出、前台程序稳定 |
| Private Working Set | 五分钟采样 P95 不超过 `15 MB`；延伸目标不超过 `10 MB` | 同上；这是发布门槛，尚无当前结果 |
| 常驻线程 | `1–3` 个 | 不包含配置 GUI |
| 固定周期轮询 | `0` | 仅允许事件触发和故障后的单次退避定时器 |
| 托盘宿主 | `1` 个按需常驻进程 | 配置窗口关闭后仅隐藏窗口，托盘仍可重新打开；设备控制始终由代理负责 |

还应记录 Private Bytes、Commit Size、句柄数、上下文切换和空闲唤醒频率，但首版不为它们预设未经验证的硬门槛。

### 2.3 暂不实现

- Windows 以外的平台。
- RGB 灯效、固件升级、报告率调整。
- 连发、压枪、多动作序列、延迟脚本等宏功能。
- 全局低级鼠标/键盘钩子和 `SendInput` 输入模拟。
- 内核驱动、Windows Service 或管理员权限常驻。
- 云同步、账号系统、遥测和自动更新。
- 与 G HUB 同时控制同一设备的仲裁。

## 3. 架构决策

### 3.1 总体架构

采用“轻量代理常驻、配置 GUI 按需运行”的双进程架构：

~~~mermaid
flowchart LR
    User["用户"] -->|"托盘菜单"| UI
    User -->|"编辑 DPI / 按键"| UI

    subgraph Session["Windows 当前用户会话"]
        Agent["pulsehub-agent.exe<br/>原生 Win32 常驻代理"]
        UI["pulsehub-config.exe<br/>Slint 配置界面"]
        Store[("config.toml")]

        UI <-->|"版本化 Named Pipe IPC"| Agent
        Agent -->|"唯一写入者"| Store
        Events["前台窗口 / 设备插拔 / 电源事件"] --> Agent
    end

    Agent --> Engine["配置选择与幂等应用引擎"]
    Engine --> Backend["Logitech 设备后端"]
    Backend -->|"Windows HID + HID++"| Mouse["G102 LIGHTSYNC"]
~~~

进程职责如下：

| 进程 | 生命周期 | 职责 | 禁止事项 |
|---|---|---|---|
| `pulsehub-agent.exe` | 用户登录后常驻 | 系统事件、配置持久化、设备连接、配置切换、IPC 服务 | 不加载 Slint，不创建渲染窗口，不轮询进程或设备 |
| `pulsehub-config.exe` | 用户启动后作为托盘宿主运行 | 展示能力、编辑并校验配置、提交配置、显示设备错误、隐藏/恢复窗口、退出托盘 | 不直接打开 HID 设备，不直接写配置文件 |

这条进程边界保证代理是设备和持久化状态的唯一写入者。关闭主窗口只隐藏界面，托盘宿主继续运行；选择“退出托盘”会结束 `pulsehub-config.exe`，但不停止代理或自动切换。

### 3.2 事件驱动

代理使用阻塞式 Win32 消息循环等待系统事件。正常空闲时不设置周期定时器，不反复扫描 `cs2.exe`、HID 设备、配置文件或当前 DPI。

事件来源包括：

- `Shell_NotifyIconW`：托盘交互。
- `SetWinEventHook(EVENT_SYSTEM_FOREGROUND, ...)`：前台窗口变化。
- `RegisterDeviceNotificationW`：HID 设备到达/移除。
- `WM_POWERBROADCAST / PBT_APMRESUMEAUTOMATIC`：睡眠恢复。
- Named Pipe：GUI 请求。
- `TaskbarCreated` 注册消息：Explorer 重启后恢复托盘图标。

Win32 回调只生成内部事件，不执行 HID I/O、文件 I/O 或耗时的进程查询。

承载这些消息的窗口应是不可见的普通顶层窗口，而不是 `HWND_MESSAGE` 消息专用窗口；后者不会接收 `TaskbarCreated`、`WM_POWERBROADCAST` 等广播消息。

### 3.3 单一状态所有者

设备工作线程同时充当代理协调者：`pulsehub-profile` 的选择逻辑在该线程内调用，设备句柄、正式配置、期望配置、已应用配置和可变 `AgentState` 均由它独占。Win32 与 IPC 线程只提交 DTO，不直接调用 Profile Engine 或设备后端。

Win32 回调使用非阻塞 `try_send`。为防止有界队列满时丢失关键状态，回调按“写入 `foreground_dirty`、`device_dirty`、`power_dirty` 等原子 pending flag → 增加 `invalidation_epoch` → 尝试投递 `Wake`”的顺序执行；工作线程每处理完一条命令都会交换并清空 pending flags，然后查询最新系统状态。这样既不会阻塞消息线程，也不会依赖每个边沿事件都进入队列。

IPC 修改请求允许有上限地等待队列槽位。队列封装必须先成功保留槽位，再在同一入队临界区内写入命令并增加 `invalidation_epoch`，最后才让消费者看到命令；保留失败时既不入队也不增加 epoch。任何失效递增之前都必须已经存在可恢复的 pending flag 或已保留的命令，禁止产生“只有 epoch、没有事件来源”的状态。

设备工作线程在状态变化后发布不可变的 `Arc<AgentSnapshot>`。IPC 线程只能克隆最新快照；需要修改状态的请求仍必须进入协调者队列并等待有上限的响应。

核心不变量：

1. 只有代理能保存正式配置。
2. 只有设备工作线程能发送 HID++ 请求。
3. 只有整套配置成功并完成必要校验后，才能更新 `applied_fingerprint`。
4. 设备重连会增加连接代次，使旧的已应用缓存立即失效。
5. 相同期望指纹在同一设备连接代次内不会重复写入。
6. 每次应用携带 `connection_generation + invalidation_epoch`；任一值过期时，成功、失败、部分应用错误和退避计划都不得发布。

