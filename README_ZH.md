# PulseHub

**简体中文** | [English](README.md)

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev/)

PulseHub 是面向 Windows 11 的轻量开源鼠标配置程序。v0.1.3 为 **Logitech G102 LIGHTSYNC** 提供经过实机验证的 DPI、回报率、按键映射、可移植配置迁移、应用环境、自动环境切换、可靠的登录启动、安全退出恢复，以及简体中文与 English 双语 Slint 图形界面。

PulseHub 是独立项目，与 Logitech 没有隶属、授权或背书关系。

## 下载

从 [GitHub Releases](https://github.com/liukk222/PulseHub/releases/latest) 下载最新 Windows 安装包：

- [PulseHub v0.1.3 Windows 11 x64 安装程序](https://github.com/liukk222/PulseHub/releases/download/v0.1.3/PulseHub-Setup-0.1.3-windows-x64.exe)
- [SHA-256 校验文件](https://github.com/liukk222/PulseHub/releases/download/v0.1.3/PulseHub-Setup-0.1.3-windows-x64.exe.sha256)

在 PowerShell 中验证安装包：

```powershell
Get-FileHash .\PulseHub-Setup-0.1.3-windows-x64.exe -Algorithm SHA256
```

请将输出与随附 `.sha256` 文件中的 SHA-256 值比对。

v0.1.3 安装包尚未进行数字签名，Windows SmartScreen 可能提示“未知发布者”。请只从本仓库下载，并在运行前核对 SHA-256。

## 支持范围

- Windows 11 x64
- Logitech G102 LIGHTSYNC，USB ID `046d:c092`
- 从源码构建时使用 Rust MSVC 工具链

v0.1.3 未声明支持其他鼠标型号或操作系统。

## 已完成功能

- 真实 HID/HID++ 设备发现、能力查询、写入和回读校验
- 自定义 DPI 与原生四档 DPI 循环
- 1000、500、250、125 Hz 四个固定回报率选项
- 中键、G4、G5、G6 可配置为原生动作或键盘快捷键
- 左键和右键保持原生点击并受到安全保护
- Office、CS2 和用户导入 EXE 的独立应用环境，每个环境可保存不同鼠标速度与按键配置
- 支持导入和导出 Office、CS2、退出配置、切换规则及用户导入的应用环境
- 根据前台程序自动切换鼠标速度和按键配置，或固定使用指定环境
- 设备拔插恢复和有界重试
- 轻量后台代理与系统托盘；关闭 GUI 后自动切换继续运行
- 用户可配置的退出恢复配置，并执行硬件回读校验
- 可选登录时启动与开发者日志；开发者日志默认关闭
- 简体中文和 English 界面
- 不支持设备灯光控制；Logitech G102 LIGHTSYNC 灯光固定关闭且不能修改

## 为极致而生

PulseHub 把实时设备控制集中在 `pulsehub-agent.exe`：它同时负责系统托盘、前台程序识别、环境切换、设备拔插恢复和 HID++ 通信。`pulsehub-config.exe` 只在用户打开设置界面时运行；关闭 GUI 后，配置界面进程退出，只保留一个轻量后台代理，自动切换功能不会中断。

生产版本启用了优化编译、Thin LTO、单代码生成单元、`panic = "abort"` 和符号剥离。在本项目的 Windows 11 实机测试中，关闭 GUI、关闭开发者日志并进入稳定空闲状态后，任务管理器曾测得 `pulsehub-agent.exe` **约 0.9 MB 内存占用**。这是特定测试环境的实测结果，不是对所有 Windows 版本、驱动和运行状态的固定保证；设备重连、环境切换或系统统计方式不同都会让瞬时数值变化。

这种架构的目标很直接：日常使用时不让完整 GUI 常驻内存，只让真正需要的设备代理安静地留在托盘后台。需要修改配置时，从托盘重新打开 GUI；完成后直接关闭，代理继续以极低资源占用工作。

### 为不同程序导入独立配置

在“应用环境”页面可以选择或输入任意 EXE 的完整路径，例如 Word、PowerPoint、设计软件或游戏。每个导入程序都拥有自己的 DPI、回报率和按键映射：

1. 导入目标程序的 EXE；
2. 为它设置合适的鼠标速度和按键；
3. 在自动模式下，让 PulseHub 识别当前前台程序；
4. 切入目标程序时自动应用其配置，离开后自动恢复 Office 或其他匹配环境。

因此，不同程序之间不再需要手动反复调整鼠标速度。也可以在设置页面选择固定模式，始终使用 Office、CS2 或任意已导入程序的配置。

### 不支持灯光控制

PulseHub 专注于鼠标性能、按键与环境切换，**不提供 RGB 或设备灯光控制功能**。受支持的 Logitech G102 LIGHTSYNC 灯光会固定关闭，GUI 中没有颜色、亮度或动画选项，也不能解除这一限制。需要灯光功能的用户可以基于本项目的 MIT 开源代码自行开发；官方 v0.1.3 不计划把灯光控制加入配置范围。

## 安装

1. 退出 Logitech G HUB，避免两个程序同时控制鼠标。
2. 运行 `PulseHub-Setup-0.1.3-windows-x64.exe`。
3. 选择安装向导语言：简体中文或 English。
4. 阅读并接受安装协议和第三方声明。
5. 选择安装目录和 PulseHub 默认界面语言。
6. 启动 PulseHub，配置 Office、CS2 或导入的应用环境。

安装目录中会同时提供 MIT 许可证和第三方声明。

## 从源码构建

### 环境要求

- Windows 11 x64
- Git
- Rust 1.97 或更高版本，以及 `x86_64-pc-windows-msvc` 工具链
- Microsoft C++ Build Tools，用于 MSVC 链接器和原生依赖
- 建议使用 PowerShell 7

克隆并进入仓库：

```powershell
git clone https://github.com/liukk222/PulseHub.git
cd PulseHub
```

检查并构建整个工作区：

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

构建经过优化的生产版本：

```powershell
cargo build --release -p pulsehub-agent -p pulsehub-config
```

生成的程序位于：

```text
target\release\pulsehub-agent.exe
target\release\pulsehub-config.exe
```

从源码运行 GUI：

```powershell
cargo run -p pulsehub-config
```

PulseHub 会在需要时启动后台代理。程序内部对设备写入执行显式确认；开发工具也提供确认参数，避免意外进行 HID 写入。

## 构建 Windows 安装包

先安装 [Inno Setup 6](https://jrsoftware.org/isinfo.php)，然后执行：

```powershell
winget install --id JRSoftware.InnoSetup -e
.\installer\build-installer.ps1
```

脚本会构建 Rust Release 程序、校验固定版本的 Inno Setup 简体中文语言文件、提取 PulseHub 图标，并把单文件安装程序输出到 `installer\output`。

如需复用已有 Release 程序：

```powershell
.\installer\build-installer.ps1 -SkipRustBuild
```

## 工程结构

```text
apps/pulsehub-agent       后台设备代理与系统托盘
apps/pulsehub-config      Slint 配置 GUI
crates/pulsehub-device    HID 发现与 Logitech HID++ 实现
crates/pulsehub-config-store
                          配置结构、校验和原子存储
crates/pulsehub-ipc       Named Pipe IPC 协议与 Windows 传输
crates/pulsehub-profile   环境选择与自动切换逻辑
crates/pulsehub-ui        公共 Slint 集成
tools/pulsehub-probe      只读发现与需要显式确认的测试写入
installer                 Windows 安装器源码和构建脚本
docs                      架构与实现文档
```

架构、HID++、IPC、配置、GUI、测试和发布文档的入口是 [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md)。

## 硬件安全

PulseHub 可以向真实鼠标写入 DPI、回报率、按键映射、灯光状态和板载配置。修改 HID++ 代码或执行写入测试前：

- 确认准确的设备身份；
- 退出 Logitech G HUB；
- 保留回读校验；
- 开发工具必须使用显式确认参数；
- 保持左键和右键的原生点击功能；
- 避免不必要的板载闪存写入。

## 许可证

PulseHub 自有源码采用 [MIT License](LICENSE)。第三方组件和兼容性研究资料仍适用各自条款，详见 [简体中文第三方声明](THIRD_PARTY_NOTICES_ZH.md) 和 [Windows 依赖许可证审计](docs/DEPENDENCY_LICENSE_AUDIT_ZH.md)。

Logitech、Logitech G、G102 LIGHTSYNC 及相关产品名称和标识归其各自权利人所有。
