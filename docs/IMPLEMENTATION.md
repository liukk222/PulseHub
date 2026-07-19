# PulseHub 实现文档

> 文档版本：0.1  
> 更新日期：2026-07-19  
> 目标平台：Windows 11 专业版 x64  
> 文档状态：设计基线，待实现

## 1. 文档定位

PulseHub 是一个使用 Rust 开发的轻量鼠标配置程序。第一阶段面向 Logitech G102 LIGHTSYNC，提供真实硬件 DPI 设置、鼠标按键分配，以及“办公”和“CS2”两套配置的自动切换。

当前工作区已初始化 Cargo Workspace，并包含领域模型、设备接口、配置切换、配置存储、IPC 与三个可执行入口的基础骨架；Windows HID 枚举、HID++ 功能发现、DPI 读写和板载配置只读解析已经通过 G102 实机验证。Win32 托盘、Slint GUI、Named Pipe 传输以及按键写入仍未实现。因此，本文同时记录已验证实现与后续编码基线；未明确标为实机验证的性能数字和协议细节仍属于待验证目标。

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
| 常驻 GUI/渲染器 | `0` | 配置窗口关闭或最小化后，GUI 进程退出 |

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
    User["用户"] -->|"托盘菜单"| Agent
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
| `pulsehub-agent.exe` | 用户登录后常驻 | 托盘、系统事件、配置持久化、设备连接、配置切换、IPC 服务 | 不加载 Slint，不创建渲染窗口，不轮询进程或设备 |
| `pulsehub-config.exe` | 用户配置时按需启动 | 展示能力、编辑并校验配置、提交配置、显示设备错误 | 不直接打开 HID 设备，不直接写配置文件，不在最小化后隐藏常驻 |

这条进程边界保证代理是设备和持久化状态的唯一写入者，同时使关闭 GUI 能确定性释放字体、渲染器和 GPU 资源。

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

## 4. 建议工程结构

~~~text
PulseHub/
├─ Cargo.toml
├─ Cargo.lock
├─ rust-toolchain.toml
├─ apps/
│  ├─ pulsehub-agent/
│  │  ├─ Cargo.toml
│  │  └─ src/
│  └─ pulsehub-config/
│     ├─ Cargo.toml
│     ├─ build.rs
│     ├─ src/
│     └─ ui/
│        └─ main.slint
├─ crates/
│  ├─ pulsehub-core/       # 领域类型、校验、错误和 DTO
│  ├─ pulsehub-device/     # HID 传输、HID++ 编解码、G102 适配
│  ├─ pulsehub-profile/    # 环境选择和幂等应用状态机
│  ├─ pulsehub-config/     # 配置 schema、迁移和原子持久化
│  └─ pulsehub-ipc/        # Named Pipe 帧、消息和协议版本
├─ tools/
│  └─ pulsehub-probe/      # 仅开发使用的设备探测工具
├─ assets/
│  ├─ icons/
│  └─ mouse-layouts/
├─ tests/
│  ├─ fixtures/
│  └─ hardware/
└─ docs/
   └─ IMPLEMENTATION.md
~~~

依赖方向必须保持单向：

~~~mermaid
flowchart TD
    Core["pulsehub-core"]
    Device["pulsehub-device"] --> Core
    Profile["pulsehub-profile"] --> Core
    Config["pulsehub-config"] --> Core
    IPC["pulsehub-ipc"] --> Core
    Agent["pulsehub-agent"] --> Device
    Agent --> Profile
    Agent --> Config
    Agent --> IPC
    UI["pulsehub-config app"] --> IPC
    UI --> Core
~~~

`pulsehub-core` 不得依赖 Win32、Slint 或具体 Logitech 协议。GUI 也不得依赖 `pulsehub-device`。

## 5. 模块职责

### 5.1 `pulsehub-core`

定义与平台无关的领域模型：

~~~rust
pub enum Environment {
    Office,
    Cs2,
}

pub enum DpiValues {
    Range { min: u16, max: u16, step: u16 },
    Discrete(Vec<u16>),
}

pub struct DeviceCapabilities {
    pub device: DeviceIdentity,
    pub dpi_values: DpiValues,
    pub controls: Vec<ControlCapability>,
    pub runtime_dpi: bool,
    pub runtime_button_mapping: bool,
    pub onboard_profile_count: u8,
}

pub enum MappingMechanism {
    RuntimeRemap,
    OnboardCommit,
}

pub enum ButtonAction {
    LogicalControl(LogicalControlId),
    OnboardKeyboard(HidKey),
    OnboardConsumer(HidConsumerControl),
    Disabled,
}

pub struct ActionCapability {
    pub action: ButtonAction,
    pub mechanism: MappingMechanism,
}

pub struct Profile {
    pub id: ProfileId,
    pub dpi: u16,
    pub buttons: Vec<ButtonMapping>,
}
~~~

`ButtonAction` 只能表示一个离散动作，不包含事件序列、循环、等待时间或重复次数。`LogicalControl` 表示设备通过运行时重映射功能明确公布的目标 Control ID；`OnboardKeyboard` 和 `OnboardConsumer` 只有在板载配置功能确认支持后才会出现在能力列表。UI 必须同时检查动作和 `MappingMechanism`，不能因领域 enum 存在某个变体就假定当前设备支持它。

### 5.2 `pulsehub-device`

该 crate 包含三层：

1. `transport`：枚举并打开 HID collection，收发报告。
2. `hidpp`：HID++ 报文编解码、根功能发现、错误解析和请求关联。
3. `devices/logitech_g102`：G102/G203 型号能力适配和配置应用顺序。

对上层暴露同步接口，因为所有调用已经被串行化到设备工作线程：

~~~rust
pub trait MouseDevice: Send {
    fn identity(&self) -> &DeviceIdentity;
    fn capabilities(&self) -> &DeviceCapabilities;
    fn read_state(&mut self) -> Result<DeviceState, DeviceError>;
    fn apply_profile(&mut self, profile: &Profile)
        -> Result<ApplyReport, DeviceError>;
}
~~~

传输层再通过内部 trait 隔离：

~~~rust
pub trait HidTransport {
    fn transact(
        &mut self,
        request: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, TransportError>;
}
~~~

协议验证阶段可先使用 `hidapi` 的 Windows 后端；生产版本是否改为 `windows` crate 直接调用 SetupAPI/HID API，以实测资源、设备兼容和维护成本为准。上层不应感知这个选择。

### 5.3 `pulsehub-profile`

负责：

- 将前台进程解析为 `Environment`。
- 支持自动模式和手动覆盖模式。
- 对短时间内连续的前台事件做一次性合并，不使用周期轮询。
- 计算期望配置指纹。
- 判断是否需要应用配置。
- 维护设备连接、应用中、就绪和降级状态。

### 5.4 `pulsehub-config`

负责：

- `schema_version` 和数据迁移。
- 默认配置生成。
- 领域级校验。
- 同目录临时文件写入、落盘和原子替换。
- 主文件损坏时读取备份并报告恢复事件。

阶段 3 当前实现使用 `serde`/`toml` 严格解析 schema v1，并拒绝未知字段；校验覆盖必需的 Office/CS2
配置、自动选择进程规则、重复物理控制、Keyboard HID Usage 和左/右主点击保护。默认配置为 Office
`1800 DPI`、CS2 `800 DPI`，两者共用已经验证的板载办公按键映射，因此环境切换不会反复提交
按键闪存。`pulsehub-agent` 首次运行已在 `%APPDATA%\PulseHub\config.toml` 创建并重新加载该配置。

### 5.5 `pulsehub-ipc`

负责：

- IPC 协议版本协商。
- 长度前缀帧的编解码。
- 请求、响应和事件 DTO。
- 最大消息长度和字段上限校验。
- Windows Named Pipe 客户端/服务端封装。

## 6. 代理进程实现

### 6.1 线程模型

MVP 不引入 Tokio 等通用异步运行时。计划使用三条长期线程：

| 线程 | 所有资源 | 阻塞点 |
|---|---|---|
| Win32 消息线程 | 隐藏窗口、托盘、WinEvent hook、设备/电源通知 | `GetMessageW` |
| 设备工作线程 | `AgentState`、Profile Engine、正式配置、HID 句柄和 HID++ 会话 | 有界命令队列或 HID 响应 |
| IPC 线程 | Named Pipe 监听和连接 | Overlapped pipe I/O |

若后续能在不增加复杂度的前提下把 IPC 等待合并到消息线程，可降为两条长期线程；不得为了追求线程数字而在窗口回调中执行阻塞操作。

### 6.2 代理状态

~~~rust
pub struct AgentState {
    pub lifecycle: AgentLifecycle,
    pub device: DeviceConnectionState,
    pub connection_generation: u64,
    pub processed_invalidation_epoch: u64,
    pub selection_mode: SelectionMode,
    pub foreground: Option<ProcessIdentity>,
    pub desired_profile: ProfileId,
    pub applied_fingerprint: Option<ProfileFingerprint>,
    pub config_revision: u64,
    pub last_error: Option<PublicError>,
}
~~~

对 IPC 返回的是脱敏后的只读快照，不暴露原始句柄、任意文件路径或未经整理的 HID 报文。

`AgentState.processed_invalidation_epoch` 仍是设备工作线程独占的普通 `u64`。线程之间另共享一个不属于 `AgentState` 的 `Arc<AtomicU64> invalidation_epoch`；工作线程处理 pending flags 或出队修改命令后，把观察值复制到 `processed_invalidation_epoch`。

每次应用创建 `ApplyToken { connection_generation, invalidation_epoch }`。设备事务结束后，工作线程先校验 token 的连接代次及 `invalidation_epoch.load(...)`。只有 token 仍有效时，才允许发布成功或当前失败并建立退避；token 过期时，必须丢弃成功、失败、`PartialApply` 及其重试预算，转而处理最新 pending flags/命令。这样外部线程只修改独立的失效令牌，不违反 `AgentState` 的单线程所有权。

每次 HID 请求必须有硬超时，初始建议为 `500 ms`，整套配置应用的累计上限建议为 `3 s`，最终数值由协议 POC 校准但不得无限等待。退出时先设置原子取消标志，并在传输实现支持时调用 `CancelIoEx`；不支持取消的阻塞调用最多只能持续到当前请求超时。

应用过程中收到设备移除事件，或事务返回 `Disconnected` 时，应取消剩余 I/O、关闭句柄、清空本次连接的动态 feature index、增加 `connection_generation` 并进入 `NoDevice`；这是正常生命周期路径，不计入通用 `Degraded` 重试预算。

### 6.3 状态机

~~~mermaid
stateDiagram-v2
    [*] --> Starting
    Starting --> NoDevice: 未发现受支持设备
    Starting --> Applying: 已连接且配置有效
    Starting --> Stopping: FatalInitError / Exit
    NoDevice --> Applying: DeviceArrived
    Applying --> Ready: 全部写入成功
    Applying --> Degraded: 超时、拒绝或部分失败
    Applying --> Applying: invalidation epoch 改变，旧结果作废
    Applying --> NoDevice: DeviceRemoved / Disconnected
    Applying --> Stopping: Exit / Cancel
    Ready --> Applying: 期望配置改变
    Ready --> NoDevice: DeviceRemoved
    Degraded --> Applying: 有预算的退避 / 重连 / 配置修改 / 用户重试
    Degraded --> NoDevice: DeviceRemoved
    Ready --> Stopping: Exit
    NoDevice --> Stopping: Exit
    Degraded --> Stopping: Exit
    Stopping --> [*]
~~~

只有 `Busy` 和可恢复 `Protocol` 错误创建退避定时器，最多重试四次，间隔建议为 `250 ms、1 s、5 s、30 s`。预算耗尽后保持 `Degraded`，不再定时唤醒，直到设备到达、配置修改或用户“重试”才建立新预算。`Unsupported`、`Validation` 不自动重试；`Disconnected` 只等待设备事件。

### 6.4 启动顺序

1. 通过当前登录会话范围的命名互斥体确保代理单实例。
2. 加载并迁移配置；失败时尝试备份，再失败则使用安全默认值。
3. 启动设备工作线程和 IPC 服务。
4. 创建消息窗口，注册托盘、前台、设备和电源通知。
5. 枚举受支持设备并建立 HID++ 会话。
6. 查询当前前台进程，计算目标环境。
7. 应用目标配置并发布状态快照。

若托盘初始化失败，代理仍可继续设备控制和 IPC，但必须记录可诊断错误；若 IPC 或设备线程初始化失败，代理应退出，避免留下无法管理的半工作进程。

### 6.5 退出顺序

1. 停止接受新 IPC 请求。
2. 注销 WinEvent hook、设备通知和电源通知。
3. 删除托盘图标。
4. 请求设备线程关闭 HID 句柄并等待线程结束。
5. 完成正在进行的原子配置写入和日志刷新。
6. 销毁消息窗口并释放单实例互斥体。

进程退出时不恢复某个固定 DPI；设备保留最后一次成功设置。下次代理启动会按当前环境重新应用配置。

## 7. 环境识别与配置切换

### 7.1 前台进程识别

`EVENT_SYSTEM_FOREGROUND` 回调收到窗口句柄后，仅把事件投递给代理。实际解析流程为：

1. 用 `GetForegroundWindow` 读取最新前台窗口，丢弃过期事件。
2. 用 `GetWindowThreadProcessId` 获取 PID。
3. 用 `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` 打开进程。
4. 用 `QueryFullProcessImageNameW` 获取规范化路径。
5. 按不区分大小写的文件名匹配 `cs2.exe`；配置可选精确路径约束。

目标是“CS2 位于前台”而不是“系统中存在 CS2 进程”。无法读取进程信息时保留上一稳定环境，并记录短期诊断状态，不应立刻反复切换。

连续前台事件可使用约 `50–100 ms` 的一次性合并窗口，只处理最后一个窗口；该值必须通过实际 Alt+Tab 与游戏启动测试确定。

阶段 3 当前先实现可独立验证的一次性路径：`pulsehub-agent --inspect-foreground` 通过安全封装库读取
前台进程完整路径，并只输出匹配结果；
`--apply-current-environment --confirm-device-write` 才按 schema v1 的选择模式与进程规则应用运行态
DPI。2026-07-20 实机识别前台 `ChatGPT.exe` 为 Office，目标为 `1800 DPI`；设备状态已经一致，
因此命中幂等分支且未发送 `SET_SENSOR_DPI`。`EnvironmentTracker` 已对映射到同一环境的连续事件
去重，并支持在设备重连/恢复时失效。常驻 `EVENT_SYSTEM_FOREGROUND` hook 与消息合并尚未接入，
不能把当前一次性命令描述为已经实现自动切换。

后续实现已使用安全封装的 `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` 接入常驻监听。hook 在专用消息
线程运行，回调只向容量为 1 的通道执行 `try_send`；工作线程收到通知后等待 75 ms 并排空重复通知，
再读取最新前台进程。`EnvironmentTracker` 继续过滤映射到同一环境的窗口变化，设备层在当前 DPI
已一致时不发送写命令。`Ctrl+C` 或验证时限到期会显式卸载 hook。开发命令必须同时提供
`--watch-foreground --confirm-device-write`，可选 `--exit-after-seconds 1..3600`；整个路径只调用
`ADJUSTABLE_DPI` 运行时功能，不调用板载内存写入。

2026-07-20 的 3 秒 Windows 实机验证完成了 hook 安装、`ChatGPT.exe → Office` 初始选择、1800 DPI
幂等跳过、到期停止和 hook 卸载。由于验证期间未启动 `cs2.exe`，Office→CS2→Office 的真实事件、
800/1800 DPI 回读和快速 Alt+Tab 合并仍属于待验收项。

同日完成第二轮真实前台切换验收。用户配置临时调整为 Office `3200 DPI`、CS2 `100 DPI`；监听启动
后依次观察到 `ChatGPT.exe → Office` 将运行态 DPI 从 1800 写为 3200、`cs2.exe → Cs2` 从 3200
写为 100、`explorer.exe → Office` 从 100 恢复为 3200，三次写入均通过设备回读。随后
`ChatGPT.exe` 等仍属于 Office 的窗口事件被环境状态机去重，没有重复应用。用户现实操作确认 Office
鼠标移动明显更快、CS2 明显更慢，功能正常；退出时 hook 正常卸载。因此
Office→CS2→Office 自动 DPI 切换的主要实机路径已通过，快速连续 Alt+Tab、睡眠恢复、热拔插和
设备忙退避仍待专项测试。

常驻可靠性随后增加两项保护：

- 代理启动即持有当前 Windows 会话中的命名 mutex `Local\PulseHub.Agent.v1`，第二实例返回退出码 3，
  不加载配置也不访问 HID。跨进程实测中，第一个监听实例运行 5 秒，第二个实例被拒绝，第一个到期
  后正常释放 mutex 与 hook。
- 前台识别或临时 HID 错误不再直接终止监听。退避依次为 250 ms、500 ms、1 s、2 s、5 s、10 s，
  之后保持 10 s 上限；成功应用后重置。重试时重新解析最新前台进程，因此新环境会替换旧失败目标。
  非法 DPI 和不支持平台属于永久错误，不安排定时重试。退避序列、上限与成功复位已由单元测试
  覆盖；真实热拔插、设备忙和 G HUB 抢占仍待实机验证。

### 7.2 切换流程

~~~mermaid
sequenceDiagram
    participant W as Windows
    participant A as Agent 消息线程
    participant D as Device Worker + Profile Engine
    participant M as G102

    W->>A: EVENT_SYSTEM_FOREGROUND
    A->>A: 设置 foreground_dirty 并 try_send(Wake)
    A->>D: Wake
    D->>D: 读取最新前台进程并选择 office/cs2
    D->>D: 计算期望配置指纹和 ApplyToken
    alt 与当前指纹相同
        D->>D: NoOp
    else 需要切换
        D->>M: 写入运行时 DPI
        D->>M: 写入受支持的一对一按键映射
        D->>M: 匹配成功响应并尽可能读回
        alt 全部成功
            D->>D: token 未过期则发布 Applied
        else 任一步失败
            D->>D: token 未过期才发布 Degraded/退避
        end
    end
~~~

睡眠恢复和设备重连不改变目标环境，但会使 `applied_fingerprint` 失效并强制重放当前目标配置。

电源恢复只以 `PBT_APMRESUMEAUTOMATIC` 作为新恢复代次的触发信号；同一次恢复随后出现的 `PBT_APMRESUMESUSPEND` 只更新可见状态，不再次增加连接代次或重复应用。

### 7.3 应用语义

硬件协议不提供跨 DPI 和按键映射的通用事务，所以整套配置可能部分成功。处理规则如下：

- 写入前完成全部静态校验。
- 先应用运行时 DPI，再应用按键映射；具体顺序可在设备 POC 后调整。
- 每一步等待并校验与请求匹配的成功响应；支持读取时执行读回校验。
- 任一步失败都不更新 `applied_fingerprint`，状态进入 `Degraded`。
- 失败后保留目标配置，后续按退避策略重新应用完整配置。
- 不把“期望状态”伪装成“设备实际状态”；UI 必须显示降级原因。
- 不自动执行未经验证的板载闪存回滚。

## 8. G102 LIGHTSYNC 设备实现

### 8.1 已知能力与证据等级

Logitech 官方产品页列出 G102 LIGHTSYNC 的 `200–8,000 DPI` 和 6 个可编程按键，说明硬件和官方软件具备对应配置能力。开源 libratbag 的设备数据库把 `046d:c084`、`046d:c092` 和 `046d:c09d` 归入 G102/G103/G203，并使用 HID++ 2.0 驱动。

这些信息不能替代目标设备的运行时能力查询：

| 项目 | 当前结论 | 实现要求 |
|---|---|---|
| DPI 范围 | 实机返回 `50–8,000`、步进 `50` | 从设备返回值构建可选集合，不使用官网范围覆盖 |
| 可编程按键 | 官方页面标称 6 个 | 逐个查询控制 ID 和允许动作 |
| 常见 VID/PID | `046d:c092` 等 | 仅用于候选过滤，不作为最终能力判断 |
| `ADJUSTABLE_DPI 0x2201` | 实机确认 index `0x0A`、version `1` | 已实现范围查询、运行时写入和回读校验 |
| `REPROG_CONTROLS_V4 0x1B04` | 实机功能表未发现 | 后续从板载配置能力验证可用映射路径 |
| `ONBOARD_PROFILES 0x8100` | 实机确认 index `0x0F`、version `0`、1 个用户配置和 6 个按键 | 已实现目录/配置扇区只读与 CRC 校验；禁止写入 |

2026-07-19 的阶段 0 实机只读探测已确认首台目标设备为 `046d:c092`、release `0x5200`、产品名
`G102 LIGHTSYNC Gaming Mouse`。该设备暴露 6 个物理 USB HID collection，其中包括
`Usage Page 0xFF00 / Usage 0x0001` 的 7 字节输入/输出报告和
`Usage Page 0xFF00 / Usage 0x0002` 的 20 字节输入/输出报告；这两个 collection 是后续 HID++
短/长报文握手的候选接口。随后只读握手确认设备报告 HID++ `4.2`，Feature Set 返回 19 个功能；
`ADJUSTABLE_DPI 0x2201` 查询返回单个传感器、`50–8,000 DPI`、步进 `50`、默认 `800 DPI`，
并能读取当前 DPI。`ONBOARD_PROFILES 0x8100` 存在且可只读解析配置内容；功能表中未发现
`REPROG_CONTROLS_V4 0x1B04`。探测输出默认隐藏序列号，fixture 不得保存设备路径或原始序列号。

2026-07-20 对 `ONBOARD_PROFILES` 的只读查询进一步确认：设备当前处于 `Host` 模式，提供 1 个用户
配置、1 个 ROM 配置、6 个按键和 `16 × 255 B` 板载扇区，当前配置索引为 `0`，配置格式 ID 为
`0x04`。由于设备只有一个用户板载配置且没有公开 `REPROG_CONTROLS_V4`，不能假设“办公/CS2”按键
映射可通过两个板载槽位直接切换。阶段 2 必须先只读解析配置扇区与按键编码，再决定是否允许有限频率的
板载提交；在完成闪存耐久性和恢复验证前，自动环境切换不得写入板载配置。

同日的阶段 2 只读验证使用长报告 `MEMORY_READ`，以 16 字节分块读取目录扇区 `0x0000` 和用户配置
扇区 `0x0001`。两个 255 字节扇区的尾部 CRC-CCITT 均校验通过。解析结果为 `250 Hz`、一个
`1800 DPI` 槽位和 6 个按键动作；探测器只输出结构化动作摘要，不保存配置名称、扇区原文或宏内容。
设备同时报告运行态 `800 DPI` 和板载槽 `1800 DPI` 是预期状态：当前模式为 `Host`，前者来自
`ADJUSTABLE_DPI` 运行时状态，后者是尚未写回设备闪存的板载配置。当前实现只包含功能号 `0x50`
的读取路径，不包含 `0x60`、`0x70`、`0x80` 三种内存写入功能。

HID++ 功能索引由设备运行时分配。代码可固定查询功能 ID，但绝不能把某次设备返回的 feature index 固定写死。

### 8.2 设备发现

Windows 用户态发现流程：

1. 通过 `HidD_GetHidGuid` 获取 HIDClass GUID。
2. 使用 `SetupDiGetClassDevs` 和 `SetupDiEnumDeviceInterfaces` 枚举当前 HID collection。
3. 使用 `SetupDiGetDeviceInterfaceDetail` 获取设备路径。
4. 用 `CreateFileW` 打开候选 collection。
5. 查询 VID、PID、产品字符串、Usage Page、Usage 和报告长度。
6. 对 Logitech 候选接口发送无副作用的 HID++ 根功能查询。
7. 只有协议握手和必需能力均成功时，才创建 `LogitechG102Device`。

一个物理鼠标可能暴露多个 HID collection。实现必须选择承载 HID++ 短/长报告的厂商接口，不能误把标准鼠标输入 collection 当成配置接口。

设备唯一标识优先使用 `VID + PID + serial`；若设备不提供 serial，使用能力指纹并明确 MVP 只管理一个匹配设备，不能把可能变化的设备路径持久化为稳定 ID。

### 8.3 HID++ 会话

每条请求必须：

- 使用根功能查询把功能 ID 映射为本次连接的 feature index。
- 校验报告 ID、设备索引、feature index、函数号和错误响应。
- 通过 software ID 与预期响应元组关联请求和响应，忽略无关异步通知。
- 限制报告长度并设置超时。
- 在同一设备上串行发送，避免响应交叉。
- 热拔插或 `ERROR_DEVICE_NOT_CONNECTED` 后立即废弃句柄和全部动态索引。

开发工具 `pulsehub-probe` 应能只读输出以下信息：

- USB 标识和产品字符串。
- 所有 HID collection 的 Usage 与报告长度。
- HID++ 版本与功能 ID 列表。
- DPI 范围/离散值、当前 DPI。
- 控制 ID、当前按键动作和是否支持运行时重映射。
- 板载配置数量与当前配置，但默认不得执行写入。

原始 HID 报文仅允许在显式 `--protocol-trace` 调试模式下记录，并对日志大小设限。

### 8.4 DPI

实现规则：

- 优先使用 HID++ 运行时 DPI 功能，不调用 `SystemParametersInfoW(SPI_SETMOUSESPEED, ...)`。
- UI 只展示设备声明支持的值。
- 非法值直接拒绝，不静默四舍五入。
- 写入成功后尽可能读回当前 DPI；无法读回时把结果标记为“设备已确认、未读回”。
- Office/CS2 切换只写运行时状态，不因切换而提交板载闪存。

阶段 1 POC 已实现受保护的运行时 DPI 写入接口。开发工具只有在同时收到
`--set-dpi <DPI>` 与 `--confirm-device-write` 时才调用 `SET_SENSOR_DPI`；写入前使用设备本次会话返回的
最小值、最大值和步进校验，写入后强制执行 `GET_SENSOR_DPI` 回读，回读值与请求值不一致即返回失败。
单独提供任一参数都会在打开 HID++ 会话前拒绝执行。该保护只属于开发工具防误触措施，正式 GUI 仍需
通过代理的配置校验与应用流程执行写入。

2026-07-20 的实机写入验证将目标 G102 从 `3200 DPI` 设置为 `800 DPI`，设备确认请求且随后的
`GET_SENSOR_DPI` 回读为 `800 DPI`；用户同时观察到指针移动速度明显降低。该结果证明写入作用于鼠标
传感器运行时 DPI，而非 Windows 指针速度设置。400/800 Raw Input 定量比值、热拔插、超时和 G HUB
冲突测试仍需完成，阶段 1 尚不能标记为全部验收完成。

真实 DPI 验收：

1. 固定 Windows 指针速度并关闭“提高指针精确度”。
2. 使用单独的 Raw Input 测试工具记录原始位移。
3. 分别设置 400 和 800 DPI。
4. 使用固定起止标记，在相同物理距离、相同方向下各重复至少 10 次。
5. 计算每组原始计数中位数，验收比值 `count_800 / count_400` 位于 `1.90–2.10`。

### 8.5 按键映射

动作集合由“目标动作 + 写入机制”共同决定，不能预先承诺所有六个按键都支持任意动作：

- `REPROG_CONTROLS_V4 / 0x1B04` 路径只提供设备声明的逻辑 Control ID。鼠标左/右/中/前进/后退或特殊控制只有出现在该列表中时，才能作为运行时动作。
- `ONBOARD_PROFILES / 0x8100` 路径可能支持单个 Keyboard HID Usage、单个 Consumer Control 或禁用，但这些动作属于板载提交能力，必须经 POC 确认。
- 首版不提供通用 `DpiCycle` 领域动作；若设备把 DPI 切换公布为逻辑 Control ID，可作为普通 `LogicalControl` 原样保留。
- UI 对每个动作显示“运行时”或“板载写入”标记，并禁止把板载写入动作加入高频自动切换，除非协议 POC 证明切换本身不重复写闪存。

安全校验至少包括：

- 不允许动作序列、延迟、循环或重复。
- 默认要求仍有一个物理控制映射为鼠标左键。
- 对“禁用主按键”等危险修改使用代理侧的暂存应用：若 GUI 未在倒计时内确认，或 GUI/IPC 断开，代理恢复先前映射。
- 提供“恢复设备默认映射”入口。

自动切换按键映射存在一个关键实现门槛：

1. 若设备暴露可逆的运行时重映射能力，则可随 Office/CS2 环境切换。
2. 若设备支持至少两个板载配置且切换配置不写闪存，可预先写入后只切换活动配置。
3. 若设备只有 `ONBOARD_PROFILES` 且每次修改都会提交闪存，在确认写入寿命和协议语义前，不得在每次前台切换时写入。此时 DPI 自动切换可以先交付，按键自动切换必须降级为手动提交或调整产品范围。

该门槛必须在协议 POC 阶段解决，不能由 UI 层绕过。

### 8.6 与 G HUB 的冲突

PulseHub 和 G HUB 可能同时向设备写配置。MVP 采用以下策略：

- 代理是 PulseHub 内部唯一设备写入者。
- 连接或写入被拒绝时，错误信息提示用户完全退出 G HUB。
- 不结束 G HUB 进程，不修改其服务或文件。
- 不尝试通过高频重写“抢回”设备状态。
- 后续只有在实测可安全共存时，才修改此策略。

## 9. 配置模型与持久化

### 9.1 文件位置

建议使用每用户目录：

~~~text
%LOCALAPPDATA%\PulseHub\
├─ config.toml
├─ config.toml.bak
└─ logs\
   └─ agent.log
~~~

应用不需要管理员权限。`.snow` 目录中的文件是开发工具元数据，不属于 PulseHub 配置，运行时不得读取。

### 9.2 配置示例

以下是正式 schema 的结构示例。`g102:*` 是 G102 适配器返回的稳定物理控制 ID；`mouse:*` 必须先解析为设备当前声明的逻辑 Control ID。示例只管理两个侧键，未列出的控制保持不变。

~~~toml
schema_version = 1

[agent]
start_with_windows = true

[selection]
mode = "auto"

[[selection.rules]]
environment = "cs2"
process_names = ["cs2.exe"]

[profiles.office]
dpi = 1200

[[profiles.office.button_mappings]]
physical_control = "g102:left"
action = { kind = "logical_control", value = "mouse:left" }

[[profiles.office.button_mappings]]
physical_control = "g102:right"
action = { kind = "logical_control", value = "mouse:right" }

[[profiles.office.button_mappings]]
physical_control = "g102:middle"
action = { kind = "onboard_keyboard", usage_page = 0x07, usage = 0x2a, modifiers = 0x00 }

[[profiles.office.button_mappings]]
physical_control = "g102:side_back"
action = { kind = "onboard_keyboard", usage_page = 0x07, usage = 0x19, modifiers = 0x01 }

[[profiles.office.button_mappings]]
physical_control = "g102:side_forward"
action = { kind = "onboard_keyboard", usage_page = 0x07, usage = 0x06, modifiers = 0x01 }

[[profiles.office.button_mappings]]
physical_control = "g102:dpi"
action = { kind = "onboard_keyboard", usage_page = 0x07, usage = 0x04, modifiers = 0x01 }

[profiles.cs2]
dpi = 800

[[profiles.cs2.button_mappings]]
physical_control = "g102:side_back"
action = { kind = "logical_control", value = "mouse:forward" }

[[profiles.cs2.button_mappings]]
physical_control = "g102:side_forward"
action = { kind = "logical_control", value = "mouse:back" }
~~~

`physical_control` 必须使用能力快照返回的 canonical ID，不能保存本地化展示名。若 POC 证实板载单键动作可用，其编码采用结构化 HID Usage，例如 `{ kind = "onboard_keyboard", usage_page = 0x07, usage = 0x1e, modifiers = 0x00 }`，不能使用有歧义的 `key:1` 字符串。

办公配置的 G102 profile format `0x04` 槽位顺序固定为：左键、右键、中键、侧后 G4、侧前 G5、
DPI 键 G6。2026-07-20 确认的目标动作依次为：左键、右键、`Backspace`、`Ctrl+V`、`Ctrl+C`、
`Ctrl+A`。键盘动作使用 HID Keyboard Usage：`Backspace=0x2A`、`A=0x04`、`C=0x06`、`V=0x19`，
左 Ctrl modifier 为 `0x01`。配置层必须保留 modifier，不能把组合键表示成宏。

受保护的开发工具写入必须同时指定 `--apply-office-buttons` 和
`--confirm-onboard-flash-write`，并且不能在同一次执行中混入 DPI 写入。实现仅接受已经实机验证的
`046d:c092 / release 0x5200 / memory format 0x01 / profile format 0x04 / 6 buttons / 255 B`；
任一身份或格式字段不匹配都在首个写命令之前拒绝。事务流程如下：

1. 读取目录与原配置扇区并校验 CRC，将原扇区保留在内存中。
2. 只替换偏移 `32..56` 内的六个四字节按键绑定，其他字段保持原样，并重新计算尾部 CRC-CCITT。
3. 发送 `MEMORY_ADDR_WRITE (0x60)`、16 个 `MEMORY_WRITE (0x70)` 长报告和
   `MEMORY_WRITE_END (0x80)`；声明长度为 255 字节，最后一个传输块的第 256 字节补零。
4. 重新读取整个 255 字节扇区，要求逐字节相同且 CRC 有效；失败时写回原扇区并再次完整回读。
5. 目标内容已一致时返回成功但不重复写闪存。

2026-07-20 的首次实机事务只改变槽位 2（中键）和槽位 5（DPI G6），整扇区写后回读通过；随后
使用新 HID++ 会话再次读取，确认中键为 `Backspace`、DPI G6 为 `Ctrl+A`，左/右键和 G4/G5 未变。
写入前临时停止的 `LGHUBUpdaterService` 与 `logi_lamparray_service` 已在 `finally` 中恢复为原来的
运行状态，未修改其自动启动设置。设备仍处于 `Host` 模式；板载内容持久化成功不等同于已经完成
办公/CS2 高频自动切换方案，后者仍禁止在每次前台切换时提交闪存。

首次验证完成后，按用户要求再次停止 `LGHUBUpdaterService`，最终状态为 `Stopped / Automatic`；
没有删除服务或修改启动类型，`logi_lamparray_service` 保持运行。再次执行同一办公映射命令命中
幂等检查并跳过闪存写入。此时运行态 DPI 读数为板载槽中的 `1800`，程序没有额外修改 DPI。

首次现实按键测试发现，配置扇区虽然写入成功，但 `mode=Host` 时板载 G4/G5 动作没有生效。这证明
“闪存内容已验证”与“当前设备正在使用板载配置”是两个独立状态。开发工具因此增加受保护的
`--activate-onboard-mode --confirm-device-mode-change`：只对已验证的 `046d:c092 / release 0x5200`
发送 `SET_ONBOARD_MODE (0x10)`，并使用 `GET_ONBOARD_MODE (0x20)` 强制回读；该操作与 DPI、板载
闪存写入互斥。2026-07-20 实机从 `Host` 切换到 `Onboard` 成功，新会话确认 `mode=Onboard`、
`current=1`，G4/G5 仍分别为 `Ctrl+V` 与 `Ctrl+C`。启用板载模式也会同时采用板载的
`1800 DPI / 250 Hz`，不能把它视为仅切换按键的无副作用操作。

用户随后在真实办公应用中确认侧后 G4 的 `Ctrl+V` 与侧前 G5 的 `Ctrl+C` 均正常触发，证明
profile format `0x04` 的槽位顺序、Keyboard HID Usage 和左 Ctrl modifier 编码正确。中键
`Backspace` 与 DPI G6 `Ctrl+A` 尚未完成同等级现实验收，不能仅凭协议回读标记为通过。

### 9.3 校验

校验分两层：

- 无设备也能执行：schema 版本、必填 profile、整数范围、唯一 ID、动作格式、禁止宏、至少保留主点击等。
- 连接设备后执行：DPI 是否属于能力集合、控制 ID 是否存在、动作是否受该控制支持、运行时切换是否可用。

无设备时可以保存“待设备校验”的草稿，但不能把它标记为已应用。

### 9.4 原子保存与迁移

保存流程：

1. 校验传入的 `base_revision`，防止多个 GUI 覆盖新配置。
2. 序列化到同一目录的临时文件。
3. 对临时文件调用 `FlushFileBuffers` 并关闭句柄。
4. 当前实现先复制仍有效的主文件为 `config.toml.bak`，再使用同目录 `NamedTempFile::persist` 原子替换
   主文件；替换前不移走或删除主文件。发布加固阶段可改用一次 `ReplaceFileW(main, temp, backup, ...)`
   同时完成替换和备份。
5. 首次保存时目标文件不存在，使用同卷的 `MoveFileExW(temp, main, MOVEFILE_WRITE_THROUGH)`。
6. 失败时保留并重新校验主文件与备份，清理仍存在的临时文件；不能把未知的中间状态当成保存成功。
7. 成功后增加 `config_revision`，再触发系统集成和设备应用。

迁移函数必须逐版本执行，例如 `v1 -> v2 -> v3`，不得直接把任意旧版本解析为最新结构。遇到未知的更高版本时只读并报错，不能覆盖文件。

上述流程提供同卷原子替换和尽力而为的崩溃恢复，不宣称在突然断电、磁盘控制器缓存未落盘等情况下具备数据库级持久性。

`start_with_windows` 是配置声明的期望状态。配置提交成功后由代理使用当前用户权限同步 `HKCU Run`；注册表同步失败不回滚配置文件，但必须返回独立的系统集成错误。安装器仅负责初始选择和卸载清理。

## 10. IPC 协议

### 10.1 管道和权限

建议管道名：

~~~text
\\.\pipe\PulseHub.Agent.<LogonSID>
~~~

服务端创建管道时必须显式设置安全描述符：

- 只允许当前 logon SID 读写。
- 可按运维需要额外允许 LocalSystem，但普通运行不依赖该主体。
- 拒绝匿名、Everyone 和网络访问。
- 使用 `PIPE_REJECT_REMOTE_CLIENTS`。

不能依赖 Named Pipe 默认 DACL，因为默认描述符可能向 Everyone/匿名主体授予读取权限。

### 10.2 帧格式

使用字节流管道和显式长度前缀：

~~~text
u32 little-endian payload_length
payload_length bytes UTF-8 JSON
~~~

规则：

- 最大 payload 为 `64 KiB`。
- 超长、截断、非法 UTF-8 或非法 JSON 立即关闭该客户端连接。
- 每个请求包含 `version`、`request_id` 和 snake_case `type`。
- 每个响应复用 `request_id`。
- 未协商版本前只接受 `type = "hello"`。
- 协议错误不能导致代理崩溃或影响其他连接。

首次连接先完成版本协商：

~~~json
{
  "version": 1,
  "request_id": "hello-1",
  "type": "hello",
  "supported_versions": [1],
  "client": "pulsehub-config"
}
~~~

~~~json
{
  "version": 1,
  "request_id": "hello-1",
  "ok": true,
  "data": { "selected_version": 1 }
}
~~~

协商成功后才能读取快照：

~~~json
{"version":1,"request_id":"42","type":"get_snapshot"}
~~~

~~~json
{
  "version": 1,
  "request_id": "42",
  "ok": true,
  "data": {
    "device_status": "ready",
    "active_environment": "office",
    "config_revision": 7
  }
}
~~~

错误响应使用稳定错误码，不把内部错误字符串当作协议：

~~~json
{
  "version": 1,
  "request_id": "42",
  "ok": false,
  "error": {
    "code": "PH-IPC-INVALID-REQUEST",
    "message": "请求字段无效",
    "retryable": false
  }
}
~~~

### 10.3 消息集合

下表名称就是 JSON `type` 的线格式，不是 Rust enum 变体名：

| 请求 `type` | 用途 |
|---|---|
| `hello` | 协商协议版本和客户端类型 |
| `get_snapshot` | 获取设备、能力、活动环境、配置和错误快照 |
| `validate_draft` | 不持久化地验证配置草稿 |
| `commit_config` | 带 `base_revision` 原子保存并按需应用 |
| `apply_now` | 用户显式重试当前目标配置 |
| `set_selection_mode` | 切换自动/手动环境 |
| `attach_ui` | 注册当前 GUI，以接收状态变化和激活事件 |

服务端事件：

- `snapshot_changed`
- `apply_started`
- `apply_finished`
- `device_changed`
- `activate_ui`

`commit_config` 的成功响应只表示文件已经提交；设备应用和登录启动同步分别通过 `apply_status`、`integration_status` 或后续事件表达，不能混为一个布尔值。

## 11. 配置 GUI

GUI 使用 Slint，仅在主线程运行 Slint 事件循环。所有 IPC 均在工作线程完成，再通过 Slint 的事件循环投递接口更新 UI。

首版页面：

1. 设备概览：连接状态、型号、能力和冲突错误。
2. Office 配置：DPI 和按键映射。
3. CS2 配置：DPI、按键映射和进程规则。
4. 设置：登录启动、自动/手动切换、诊断日志。

交互规则：

- DPI 控件由能力集合生成，不显示任意自由输入值。
- 不受支持的动作不出现在按键下拉列表。
- 保存前先调用 `validate_draft`。
- 保存成功但硬件应用失败时，明确显示“已保存、未应用”。
- GUI 由用户直接启动但代理未运行时，可用绝对安装路径启动代理并在有限时间内重连。

MVP 的最小化行为固定为“保存并关闭配置窗口”，不作为可配置项：

1. 页面无脏数据时直接退出 GUI。
2. 页面有脏数据且校验通过时自动调用 `commit_config`；提交成功后退出。
3. 校验失败、修订冲突或保存失败时取消最小化，恢复窗口并定位错误，GUI 不退出。
4. 用户若不希望保存，必须显式选择“放弃更改并关闭”；关闭操作仍提供“保存并关闭 / 放弃 / 取消”三种选择。

代理跟踪已连接 GUI。再次点击托盘“打开设置”时，若 GUI 已连接，则发送 `activate_ui`；否则使用绝对路径启动新的 GUI。多个编辑器通过 `config_revision` 冲突检测避免静默覆盖。

## 12. Windows 集成

| 能力 | API/机制 | 实现要点 |
|---|---|---|
| 托盘 | `Shell_NotifyIconW` | `NIM_ADD` 后设置 `NOTIFYICON_VERSION_4`；退出时 `NIM_DELETE` |
| Explorer 恢复 | `RegisterWindowMessageW("TaskbarCreated")` | 收到后重新添加图标 |
| 阻塞消息循环 | `GetMessageW / TranslateMessage / DispatchMessageW` | 正确区分 `0`、`-1` 和普通消息 |
| 前台变化 | `SetWinEventHook` | `WINEVENT_OUTOFCONTEXT \| WINEVENT_SKIPOWNPROCESS`；注册线程必须有消息循环 |
| 设备变化 | `RegisterDeviceNotificationW` | 窗口过程只排队，枚举在工作线程执行 |
| 电源恢复 | `WM_POWERBROADCAST / PBT_APMRESUMEAUTOMATIC` | 每个恢复代次只废弃一次旧句柄并重新枚举；不因后续 `PBT_APMRESUMESUSPEND` 重放 |
| 单实例 | 当前会话命名互斥体 | 名称包含 logon SID，不跨用户互斥 |
| 登录启动 | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` | 每用户、无需提权；代理按配置同步，安装器卸载时清理 |

代理不是 Windows Service，因为托盘、前台窗口和每用户配置都属于交互式用户会话。

## 13. 错误处理与可观测性

### 13.1 错误分类

| 类别 | 例子 | 策略 |
|---|---|---|
| `Unsupported` | 型号、功能或动作不支持 | 不重试；提示用户 |
| `Validation` | DPI 越界、非法映射、版本冲突 | 不写设备和文件 |
| `Busy` | G HUB 占用或访问被拒绝 | 有界退避；提示退出 G HUB |
| `Disconnected` | 热拔插、睡眠后句柄失效 | 清空连接状态，等待设备事件 |
| `Protocol` | 超时、短包、错误 feature index | 重建会话后有限重试 |
| `PartialApply` | DPI 成功、按键失败 | 标记降级；不更新已应用指纹 |
| `ConfigIo` | 文件损坏、替换失败 | 保留旧文件；尝试备份 |
| `Ipc` | 版本不兼容、超长帧 | 断开该客户端，不影响代理 |

所有 Win32/HID 句柄使用 RAII 封装；Drop 负责关闭句柄，显式 shutdown 负责有序停止业务。

### 13.2 日志

- Release 默认记录生命周期、设备连接、配置修订、切换结果和错误码。
- 不记录键盘输入内容、窗口标题或完整用户文件路径。
- 不记录鼠标移动报告。
- 协议原始报文仅在用户显式启用的诊断会话中记录。
- 日志按大小轮转并限制总量；同步写入发生频率很低，不额外创建常驻日志线程。
- UI 展示稳定的公开错误码，例如 `PH-DEV-BUSY`，详细 Win32/HID 错误留在本地日志。

## 14. 安全与游戏兼容

- 以普通用户权限运行；不安装驱动、不注入进程、不请求 `SeDebugPrivilege`。
- 只用前台窗口事件识别环境，不读取游戏内存。
- 不安装 `WH_MOUSE_LL` / `WH_KEYBOARD_LL` 钩子。
- 不使用 `SendInput` 生成游戏操作。
- 只实现硬件支持的一对一映射，不实现宏。
- Named Pipe 只允许当前登录会话访问，且限制帧长和字段数量。
- 启动另一个可执行文件时使用安装目录中的绝对路径，并通过 `std::process::Command` 逐项传参，不拼接 shell 命令。
- 配置和日志保存在当前用户目录，不接受 IPC 传入任意输出路径。
- 对逆向得到的协议值进行严格边界检查，任何未知响应都按错误处理。

这些约束降低兼容和反作弊风险，但不构成对第三方游戏或反作弊系统的保证。发布前仍需核对 Valve/CS2 当时有效的规则。

## 15. 构建、发布与部署

### 15.1 构建基线

- Rust stable，使用 `rust-toolchain.toml` 固定团队工具链。
- 目标：`x86_64-pc-windows-msvc`。
- Slint、`windows`、`serde` 等依赖由 `Cargo.lock` 固定精确版本。
- 两个发布应用的 Release 构建使用 Windows GUI subsystem，避免启动控制台窗口；`pulsehub-probe` 保留控制台子系统。
- 依赖版本和 Slint 许可证必须在首次发布前复核。

工程初始化后，基础验证命令应为：

~~~powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release --target x86_64-pc-windows-msvc
~~~

Cargo Workspace 已初始化；上述命令用于验证当前基础骨架。引入 `windows`、Slint、`serde` 或 HID 传输依赖前，应先完成设备协议 POC 并更新 `Cargo.lock`。

### 15.2 发布产物

~~~text
PulseHub/
├─ pulsehub-agent.exe
├─ pulsehub-config.exe
├─ assets/
└─ licenses/
~~~

安装器应：

- 使用每用户安装目录，例如 `%LOCALAPPDATA%\Programs\PulseHub`。
- 创建开始菜单入口。
- 按用户选择添加 `HKCU Run` 登录启动项。
- 升级时先请求代理正常退出，再原子替换文件。
- 卸载时移除启动项和程序文件；是否保留用户配置必须提示。
- 正式发布的可执行文件和安装包使用代码签名。

具体安装器技术尚未确定，不应在代码中耦合某一种打包工具。

## 16. 测试与验收

### 16.1 自动化测试

| 层次 | 必测内容 |
|---|---|
| `pulsehub-core` | DPI 边界/步长、动作白名单、配置指纹、主点击保护 |
| `pulsehub-profile` | 进入/离开 CS2、重复事件去重、手动覆盖、连接代次/invalidation epoch 失效 |
| `pulsehub-config` | 默认值、每版迁移、损坏恢复、原子保存、修订冲突 |
| `pulsehub-ipc` | 版本协商、截断帧、超长帧、未知字段、断连和 ACL |
| `pulsehub-device` | HID++ 编解码 golden test、错误响应、超时和乱序通知 |
| Windows 集成 | Explorer 重启、登录启动、恢复事件去重、热拔插、队列满时全量重扫、双实例 |

硬件测试使用 `#[ignore]` 或单独测试程序，不能在普通 CI 中自动改写开发者鼠标。

### 16.2 G102 硬件门槛

在开始完整 GUI 前必须完成：

1. 记录目标鼠标型号、硬件修订、固件、VID/PID 和所有 HID collection。
2. 证明可无副作用枚举 HID++ 功能。
3. 读出 DPI 能力并完成 400/800 DPI 运行时切换。
4. 用 Raw Input 验证至少 10 组样本的中位计数比例处于 `1.90–2.10`。
5. 重启代理、热拔插和睡眠恢复后重新应用成功。
6. 枚举六个物理控制，并逐项记录设备实际声明的可映射动作与写入机制。
7. 确认按键映射是运行时、配置切换还是板载闪存写入。
8. 验证 G HUB 运行、完全退出两种情况下的行为。

第 7 项若不能满足安全的频繁切换，必须在继续开发前调整按键自动切换方案。

### 16.3 性能验收

测试条件：

- Release + MSVC 构建。
- 目标 Windows 11 专业版实体机。
- G102 已连接，Office 配置已稳定。
- GUI 进程已退出。
- 停止调试器和协议 trace。
- 预热五分钟，再以一秒间隔采样五分钟，共 300 个样本。

使用基于 `GetProcessTimes` 和 `GetProcessMemoryInfo` 的固定测试脚本采样，并用 WPR/WPA 复核唤醒来源。CPU 按以下方式归一化到整机：

~~~text
cpu_percent =
  process_cpu_time_delta
  / (wall_time_delta * logical_processor_count)
  * 100
~~~

发布门槛：

- 300 个样本的平均 CPU 小于 `0.1%`。
- Private Working Set 的 P95 不超过 `15 MB`；不超过 `10 MB` 是延伸目标。
- 稳定态线程数不超过 3，且进程模块中不含 Slint 或渲染后端。
- 稳定态和错误重试预算耗尽后均无周期定时唤醒；WPR 中的唤醒必须能归因于系统/IPC/设备事件。
- 同时记录峰值 CPU、Private Bytes、Commit Size、句柄数，以及设备插拔、Alt+Tab 和睡眠恢复时的短时峰值，作为回归基线。

若不满足目标，先检查周期定时器、日志循环、失败重试、隐藏 GUI 和重复 HID 写入，再评估替换依赖。

## 17. 实施阶段

### 阶段 0：工程与只读探测

- 初始化 Cargo Workspace、格式化、lint 和测试框架。
- 实现 `pulsehub-probe` 的 HID 枚举与只读能力输出。
- 保存脱敏的设备描述符和 HID++ 响应为测试 fixture。

完成标准：能够稳定识别目标 G102，不执行任何配置写入。

### 阶段 1：DPI 协议 POC

- 实现 HID++ 根功能发现和 DPI 读写。
- 完成 400/800 DPI Raw Input 验证。
- 验证热拔插、超时和 G HUB 冲突。

完成标准：在不修改 Windows 指针速度的情况下稳定切换真实 DPI。

### 阶段 2：按键映射 POC

- 枚举控制和动作。
- 验证运行时映射、板载配置和持久性。
- 加入主点击保护和恢复默认功能。

完成标准：确认 Office/CS2 自动切换是否不会频繁写入闪存。

### 阶段 3：代理与配置引擎

- 实现托盘、系统事件、状态机、配置持久化和 IPC。
- 实现幂等应用、连接代次和故障退避。
- 完成资源基准测试。

完成标准：无 GUI 时满足常驻资源目标，所有生命周期事件可恢复。

### 阶段 4：Slint GUI

- 实现能力驱动的 DPI/按键界面。
- 实现草稿校验、修订冲突和“已保存/已应用”分离状态。
- 验证关闭/最小化后 GUI 进程彻底退出。

完成标准：用户可以安全完成两套配置，GUI 不参与设备所有权。

### 阶段 5：发布加固

- 完成安装、登录启动、签名、升级和卸载。
- 完成 Windows 11 实机、CS2、G HUB 冲突和性能回归。
- 冻结 IPC/config schema v1。

完成标准：签名安装包可在干净的 Windows 11 专业版机器完成安装、登录启动、升级和卸载；所有 P0 风险关闭，自动化、硬件与性能发布门槛全部通过。

## 18. 风险与待确认项

| 优先级 | 问题 | 影响 | 决策门槛 |
|---|---|---|---|
| P0 | 首发 G102 的准确硬件修订与固件未知 | 协议和 PID 可能不同 | 阶段 0 实机报告 |
| P0 | 按键映射是否支持无闪存的频繁切换未知 | 可能无法完整实现双环境自动切换 | 阶段 2 POC |
| P0 | HID++ 是厂商私有协议 | 固件兼容和维护风险 | fixture + 能力探测 + 严格错误处理 |
| P1 | G HUB 并发访问 | 配置互相覆盖或设备忙 | 明确退出提示；不抢占 |
| P1 | P95 `15 MB` 内存门槛尚未实测 | 可能需要精简依赖或线程 | 阶段 3 Release 基准 |
| P1 | “最小化即退出 GUI”的交互预期 | 用户可能认为程序退出 | UI 文案和首次提示 |
| P1 | Slint 版本、后端和许可证未冻结 | 构建体积及发布合规 | 工程初始化和发布评审 |
| P2 | 安装器和更新策略未选定 | 影响交付体验 | 阶段 5 决策 |

## 19. 参考资料

- [Logitech G102 LIGHTSYNC 官方产品规格](https://www.logitechg.com/en-ph/shop/p/g203-lightsync-rgb-gaming-mouse)
- [Microsoft：Finding and Opening a HID Collection](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/finding-and-opening-a-hid-collection)
- [Microsoft：Obtaining HID Reports](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/obtaining-hid-reports)
- [Microsoft：Shell_NotifyIconW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw)
- [Microsoft：SetWinEventHook](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook)
- [Microsoft：RegisterDeviceNotificationW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerdevicenotificationw)
- [Microsoft：Messages and Message Queues](https://learn.microsoft.com/en-us/windows/win32/winmsg/messages-and-message-queues)
- [Microsoft：Named Pipe Security and Access Rights](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)
- [Slint：Windows 桌面支持](https://docs.slint.dev/latest/docs/slint/guide/platforms/desktop/)
- [Slint：后端与渲染器](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/)
- [libratbag：G102/G103/G203 设备定义](https://github.com/libratbag/libratbag/blob/master/data/devices/logitech-g102-g203.device)
- [libratbag：HID++ 2.0 驱动实现](https://github.com/libratbag/libratbag/blob/master/src/driver-hidpp20.c)

> libratbag 是逆向协议的开源实现，可用于兼容性研究和测试交叉验证，但不是 Logitech 对 PulseHub 的稳定协议承诺。引用或移植代码前必须核对其许可证并保留要求的声明。
