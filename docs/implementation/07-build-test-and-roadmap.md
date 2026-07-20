# PulseHub：构建、测试与路线图

> 涵盖构建发布、测试验收、实施阶段、风险和参考资料。

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
