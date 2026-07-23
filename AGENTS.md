# PulseHub 开发指南（AGENTS）

本文件是本仓库内后续 Agent 的最高优先级开发约定。开始工作前先阅读本文件；再按任务类型阅读 `docs/` 中对应的支撑文档。若本文与旧设计记录冲突，以代码现状、用户当前需求和本文的安全边界为准；需要改变架构契约时，应同步更新本文及关联文档。

## 1. 项目与平台边界

- PulseHub 是 **仅面向 Windows 11 x64** 的 Rust 桌面程序；不接受 macOS、Linux、移动端、Web 或跨平台适配任务。
- Cargo Workspace 使用 Rust 2024、Rust 1.97，并全局禁止手写 `unsafe`；`pulsehub-ui` 是 Slint 生成代码所需 unsafe 的唯一受控边界。
- 运行时由 `pulsehub-agent.exe`（设备、持久化、系统事件和 Slint `AppTray` 托盘唯一所有者）与 `pulsehub-config.exe`（Slint GUI）构成，通过当前登录会话受限的 Windows Named Pipe 通信。
- 当前实机验证目标是 **Logitech G102 LIGHTSYNC**（已验证 `046d:c092`、release `0x5200`）。除非任务属于“其他物理设备适配”，不得放宽其设备写入保护或把其协议假设套用到其他设备。

## 2. 仅允许的开发方向

后续工作只能归入下列一个或多个方向；无法归类时，先向用户澄清，不自行扩张范围。

1. **重构项目**：改善结构、可维护性、性能或测试，同时保持既有 Windows 产品行为和安全不变量。
2. **开发新功能**：在 Windows 单平台范围内扩展 PulseHub 的用户能力；功能必须遵守代理唯一设备所有权、能力驱动和配置/应用状态分离。
3. **适配 Logitech G102 LIGHTSYNC 以外的物理设备**：可包括其他鼠标或键盘。每个设备均须独立发现、只读探测、能力建模、协议 POC、受保护写入及实机回读验证；禁止复制 G102 的 PID、feature index、报告格式、扇区布局或写入流程。

明确不做：多平台适配、内核驱动/服务、进程注入、输入模拟、宏/连发/脚本、云同步、账号、遥测，以及与 G HUB 争夺设备控制权。

## 3. 关键架构不变量

- **代理唯一写入者**：只有 agent 可持久化正式配置、打开设备、执行 HID I/O 和应用环境；GUI 不得依赖 `pulsehub-device`、不直接写配置文件或 HID。
- **串行设备所有权**：所有设备请求必须在单一协调/设备路径中串行执行；热拔插、睡眠恢复或请求过期后废弃旧连接和动态协议索引。
- **能力驱动**：GUI、配置校验和应用逻辑只能使用设备本次会话实际报告的能力；不得用静态 G102 值伪造其他设备能力。
- **保存不等于应用**：`commit_config` 成功仅表示原子保存成功；硬件写入、读回验证和 Windows 集成状态必须独立报告。
- **最小权限与会话隔离**：只使用普通用户权限和当前登录会话的受限 Named Pipe；不使用管理员权限、全局低级钩子、`SendInput`、游戏内存读取或任意 IPC 文件路径。
- **无高频闪存写入**：环境切换只可执行已验证的运行态操作。可能写入板载闪存的操作必须做内容幂等比较、限频、明确用户意图和完整回读。

## 4. 物理设备操作安全

设备写入具有真实硬件风险。任何 Agent 在执行或新增写入路径前必须：

1. 使用只读探测确认设备身份、传输接口、能力和当前状态；不以 VID/PID 候选过滤代替协议握手。
2. 保留显式确认参数；开发工具的只读模式必须是默认行为。不同类型的写入（DPI、板载闪存、模式切换）应互斥并使用单独确认。
3. 对每次写入做输入校验、超时、响应关联与写后回读；失败不得报告成功。
4. 对板载写入保留原始状态以支持受验证的恢复，并避免在自动环境切换中重复写闪存。
5. 始终保护可用的左键和右键；不自动结束、修改或抢占 G HUB 及其服务。遇到冲突仅提示用户完全退出 G HUB。
6. 不在普通测试、CI 或未获用户明确批准的命令中执行真实设备写入。

新增设备适配还必须将传输/协议逻辑与领域模型解耦，建立脱敏 fixture 和单元测试，并把设备特有写入门槛记录到对应实现文档。

## 5. 工作区导航

| 位置                              | 职责                                             |
| --------------------------------- | ------------------------------------------------ |
| `apps/pulsehub-agent`             | 常驻代理、设备协调、系统事件、IPC 服务、登录启动 |
| `apps/pulsehub-config`            | Slint 配置界面与草稿；不接触 HID，也不承载托盘   |
| `crates/pulsehub-core`            | 领域类型和跨层契约                               |
| `crates/pulsehub-device`          | Windows HID 枚举、传输与设备协议适配             |
| `crates/pulsehub-profile`         | 环境解析、去重、幂等应用与退避逻辑               |
| `crates/pulsehub-config-store`    | `config.toml` schema、校验、迁移和原子存储       |
| `crates/pulsehub-ipc`             | 版本化 DTO、帧协议和 Windows Named Pipe          |
| `crates/pulsehub-windows-session` | 当前 Windows 登录会话/SID 边界                   |
| `crates/pulsehub-ui`              | 公共 Slint 集成与受控生成代码边界                |
| `tools/pulsehub-probe`            | 默认只读的硬件探测和受保护 POC 工具              |
| `installer`                       | Windows 安装器与发布脚本                         |

## 6. 按任务阅读文档

所有 `docs/` 与 `docs/implementation/` 文档均是本文件的专题支撑材料：

- 总览与进程边界：`docs/implementation/01-overview-and-architecture.md`
- crate 依赖与模块职责：`docs/implementation/02-workspace-and-modules.md`
- agent、环境切换和生命周期：`docs/implementation/03-agent-and-profile-switching.md`
- G102 与新增物理设备协议工作：`docs/implementation/04-g102-hid.md`
- 配置、迁移、IPC 与安全边界：`docs/implementation/05-configuration-and-ipc.md`
- Slint GUI、Windows 集成、日志和兼容性：`docs/implementation/06-gui-windows-and-observability.md`
- 构建、测试、发布、硬件验收和风险：`docs/implementation/07-build-test-and-roadmap.md`
- GUI 行为与视觉验收：`docs/DESIGN.md`
- 第三方依赖合规：`docs/DEPENDENCY_LICENSE_AUDIT_ZH.md`（英文副本为 `docs/DEPENDENCY_LICENSE_AUDIT.md`）

`docs/IMPLEMENTATION.md` 是实现文档索引，不替代本文件。

## 7. 标准开发流程

1. 先定位并阅读涉及代码及以上对应文档；重构前使用引用搜索确认影响面。
2. 保持依赖方向：领域 crate 不依赖 Win32/Slint/具体厂商协议；GUI 不依赖设备 crate；跨进程变化先设计 IPC DTO 的兼容性与校验。
3. 为行为变化补充或更新测试；不要以真实硬件作为普通测试的前置条件。
4. 修改完成后至少执行：

   ```powershell
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo build --workspace
   ```

   仅在需要验证发行产物时再执行 Release/安装器构建。

5. 若修改 `Cargo.lock`、依赖、feature、构建目标、安装器内容、Slint 版本或引用/移植第三方源码，更新许可证审计并复核第三方声明。
6. 变更架构边界、设备安全规则或开发范围时，同步更新本 `AGENTS.md` 和相关 `docs` 专题文档；不要另建无请求的总结文档。

## 8. 文档维护约定

- 默认使用简体中文；保留的英文许可证审计副本用于发行合规，需要与中文内容保持语义一致。
- 文档描述“已实现”“已实机验证”时必须可由现有代码或明确测试记录支撑；计划、假设和历史记录应清晰标记。
- 新功能、重构或设备适配完成后，更新其对应专题文档、测试/验收说明及本文件中受影响的导航或不变量。
