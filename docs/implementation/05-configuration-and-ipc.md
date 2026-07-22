# PulseHub：配置与 IPC

> 涵盖配置模型、持久化、校验、迁移以及 Named Pipe IPC 协议。

每个 `ProfileConfig` 包含 DPI、四档 DPI、六个按键映射和固定四选一的 `report_rate_hz`。根配置另有 `shutdown_profile`；旧配置缺少这些字段时分别补为 `1000 Hz` 和原有安全退出默认值（`1600 DPI`、原生按键）。退出配置仍只由代理执行并回读验证。

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

[[applications]]
id = "winword"
name = "WINWORD"
executable_path = "C:\\Program Files\\Microsoft Office\\root\\Office16\\WINWORD.EXE"
process_name = "WINWORD.EXE"

[applications.profile]
dpi = 800
dpi_levels = [800, 1600, 2400, 3200]
~~~

`applications` 可包含任意数量的独立应用环境；每项拥有完整 `ProfileConfig`（DPI、四档和六键映射）。`id`、名称与 `process_name` 均需全局唯一，进程名比较不区分大小写。旧 schema v1 文件没有该字段时按空列表加载，无需人工迁移。自动模式按前台进程选择；固定导入环境使用 `mode = "application"` 与 `fixed_application_id = "<id>"`，此时不随前台程序变化。

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
7. 成功后增加 `config_revision` 并刷新代理快照。配置提交本身不写 HID；设备应用和系统集成由独立状态机处理。

迁移函数必须逐版本执行，例如 `v1 -> v2 -> v3`，不得直接把任意旧版本解析为最新结构。遇到未知的更高版本时只读并报错，不能覆盖文件。

上述流程提供同卷原子替换和尽力而为的崩溃恢复，不宣称在突然断电、磁盘控制器缓存未落盘等情况下具备数据库级持久性。

`start_with_windows` 是配置声明的期望状态。配置提交成功后由代理使用当前用户权限同步 `HKCU Run`；注册表同步失败不回滚配置文件，但必须返回独立的系统集成错误。安装器仅负责初始选择和卸载清理。

阶段 3 已实现 `ConfigRepository` 作为修订化存储边界：打开配置时建立进程内 revision 1；
`validate_draft` 将严格 JSON 草稿反序列化为 `ConfigDocument` 并复用完整 schema/业务校验，但不
落盘；`commit(base_revision, draft)` 先比较修订，冲突时不校验、不保存，匹配时完成校验和原子
保存，只有保存成功后才替换正式内存文档并递增 revision。测试覆盖成功提交、旧修订冲突和非法
JSON 不改变修订。该 revision 是代理进程生命周期内的并发控制编号，不写入 schema v1 文件；
代理重启后重新从 1 开始，旧 IPC 连接会随进程退出而失效。

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

当前传输使用 `interprocess 2.4.2` 的安全封装创建字节流管道，并由隔离的
`pulsehub-windows-session` crate 查询访问令牌 `TokenLogonSid`。管道名为
`PulseHub.Agent.<S-1-5-5-X-Y>`，受保护 DACL 为 `D:P(A;;GA;;;<TokenLogonSid>)`；同时禁止句柄
继承并保持 `accept_remote = false`（映射到 `PIPE_REJECT_REMOTE_CLIENTS`）。agent/config 已在
真实 Windows 登录会话中完成同 SID 路径和 DACL 下的内核往返。

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

当前 `pulsehub-ipc` 已实现上述纯协议层：所有 DTO 使用严格未知字段拒绝，读取端在分配 payload
前检查长度上限，`Session` 在成功处理 `hello` 前拒绝其他请求。单元测试覆盖正常往返、截断帧、
超长帧、非法 UTF-8/JSON、未知字段/消息类型、版本不匹配以及响应信封不变量。Windows Named
Pipe 帧传输、单连接会话、代理常驻 accept 循环、并发客户端上限和关机协调已实现；当前 logon
SID 命名是下一安全实现单元。开发期统一代理模式已经让前台环境监听、HID 应用和 IPC 服务共享
同一个 `RwLock<AgentSnapshot>`；环境切换完成并通过设备回读后才发布新的环境、当前 DPI 和目标
DPI。

阶段 3 当前已增加开发期常驻模式 `--serve-ipc`：主线程阻塞在 `accept`，每个连接使用独立会话
线程，最多允许 4 个活动客户端；每次请求从共享 `RwLock<AgentSnapshot>` 克隆最新快照，而不是
在连接建立时永久缓存。达到上限时立即关闭新连接。Ctrl+C 或验证超时通过本地自连接唤醒阻塞
的 `accept`，停止接收后为现有连接提供 1 秒退出窗口，剩余句柄最终由进程退出回收。该模式已经
用 3 个连续真实客户端验证快照一致性和无遗留会话退出。

开发期可在两个 PowerShell 终端执行以下端到端验证；代理先执行 HID++ 只读查询，服务一个客户端后退出，不写入设备：

~~~powershell
cargo run -p pulsehub-agent -- --serve-ipc-once
cargo run -p pulsehub-config -- --inspect-agent
~~~

常驻服务验证：

~~~powershell
cargo run -p pulsehub-agent -- --serve-ipc --exit-after-seconds 30
cargo run -p pulsehub-config -- --inspect-agent
~~~

统一代理验证（允许运行态 DPI 写入，不写板载闪存）：

~~~powershell
cargo run -p pulsehub-agent -- --run-agent --confirm-device-write
cargo run -p pulsehub-config -- --inspect-agent
cargo run -p pulsehub-config -- --validate-current-config
cargo run -p pulsehub-config -- --commit-current-config
~~~

`--run-agent` 在 IPC listener 就绪后安装前台 WinEvent hook。初始事件和后续环境变化均通过同一
设备应用路径；写后回读成功才更新共享快照。Ctrl+C 或 `--exit-after-seconds` 结束监听后，主线程
设置共享停止标志并自连接唤醒 IPC `accept`，最后等待 IPC 主线程退出。G102 实机已验证 Office
前台时幂等保持 3200 DPI，IPC 同时返回 `ready / office / current=3200 / desired=3200`，退出时
WinEvent hook 与 IPC listener 均释放。

统一代理持有 `ConfigRepository`，并在前台监听线程上串行处理 `apply_now`、`validate_draft` 和
`commit_config`。验证只执行严格反序列化与业务规则校验；提交先比较客户端读取到的
`base_revision`，冲突返回 `conflict`，成功后原子保存并更新共享快照。提交不会自动执行 HID 写入，
因此客户端能够明确区分“已保存”和“已应用”。`pulsehub-config` 的验证与提交参数用于现阶段端到端
联调；提交前会先读取代理快照中的当前修订号。

受控 `apply_now` 写入 POC 使用独立单连接模式，必须在代理进程侧显式确认设备写入：

~~~powershell
cargo run -p pulsehub-agent -- --serve-ipc-apply-once --confirm-device-write
cargo run -p pulsehub-config -- --apply-agent
~~~

普通 `--serve-ipc` 与 `--serve-ipc-once` 不接受 HID 修改。写入 POC 先完成 `hello`，随后代理在
自己的线程中重新解析当前前台目标、调用既有 DPI 应用路径，并只在写后回读成功后返回包含真实
`current_dpi` 的快照。G102 实机已完成 `3200 → 1600 → apply_now → 3200` 往返，三个阶段均有
独立回读；未写入板载闪存。

统一代理现已实现协调路径：IPC 会话线程把 `DeviceCommand::ApplyNow` 投递到容量 16 的有界
队列，并通过容量 1 的响应通道最多等待 5 秒；队列满或超时返回可重试的 `PH-IPC-BUSY`。前台
监听/设备协调线程排空命令，独占执行 HID 应用和回读，成功后先发布共享快照再响应客户端；IPC
客户端线程不直接访问 HID。G102 实机已在统一代理运行期间完成外部临时写入 1600 DPI、常驻
IPC `apply_now` 恢复 3200、随后 `get_snapshot` 回读 3200 的完整验证。

已验证输出能够从实际 `%APPDATA%\PulseHub\config.toml` 解析当前前台目标，查询 G102 的真实
运行态 DPI，并经 `hello` 与 `get_snapshot` 返回脱敏快照。设备断开、忙或协议查询失败时仍返回
对应的降级状态和 `current_dpi = null`，不得把目标 DPI 伪装成硬件当前值。

由于完整 HID++ 探测可能晚于 GUI 进程启动，配置端连接使用最长 5 秒、间隔 100 ms 的有界重试；
只重试“管道尚不存在”和“所有实例忙”，访问拒绝等安全错误立即返回。

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
    "config_revision": 7,
    "current_dpi": 1800,
    "desired_dpi": 1800
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
| `shutdown` | 安全还原鼠标后退出代理；`force=true` 明确跳过恢复并退出 |
| `set_selection_mode` | 切换自动/手动环境 |
| `attach_ui` | 注册当前 GUI，以接收状态变化和激活事件 |

`shutdown` 由当前登录会话内受 Named Pipe DACL 保护的已协商客户端调用。`force=false` 时，设备协调线程进入停止状态，阻断后续前台环境应用，将 DPI 设置为 `1600`，恢复六个原生按键并回读；可重试 HID 超时最多尝试 3 次、总预算 10 秒。设备离线时在预算耗尽后记录审计日志并退出；设备在线且恢复失败时返回可重试错误，由 GUI 提供“重试恢复 / 仍然退出”。`force=true` 不声称设备已恢复，只记录强制退出审计事件。

服务端事件：

- `snapshot_changed`
- `apply_started`
- `apply_finished`
- `device_changed`
- `activate_ui`

`commit_config` 的成功响应只表示文件已经提交；设备应用和登录启动同步分别通过 `apply_status`、`integration_status` 或后续事件表达，不能混为一个布尔值。
