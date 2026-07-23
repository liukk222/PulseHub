# PulseHub

**简体中文** | [English](README.md)

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev/) <img src="apps/pulsehub-config/ui/assets/tray-icon.svg" alt="PulseHub 托盘图标" height="210">

PulseHub 是面向 Windows 11 的轻量开源鼠标配置程序。v0.1.3 为 **Logitech G102 LIGHTSYNC** 提供经过实机验证的 DPI、回报率、按键映射、可移植配置迁移、应用环境、自动环境切换、可靠的登录启动、安全退出恢复，以及简体中文与 English 双语 Slint 图形界面。

PulseHub 是独立项目，与 Logitech 没有隶属、授权或背书关系。

## 界面与功能

PulseHub 面向 **Windows 11 x64** 和 **Logitech G102 LIGHTSYNC**（`046d:c092`）设计。以下按截图顺序说明各页面可用的功能。

### 应用标题栏

![PulseHub 应用标题栏](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20193616.png)

性能优先：PulseHub 不让完整 GUI 常驻后台。关闭设置窗口后，仅保留轻量后台代理继续执行环境切换；在本项目 Windows 11 实机空闲测试中，关闭开发者日志后 `pulsehub-agent.exe` 内存占用约 **0.8 MB**。实际 CPU 与内存占用会随设备连接、环境切换和系统状态变化。

### 设备概览

![PulseHub 概览页面](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20194042.jpg)

在这里查看已连接鼠标、设备能力和当前环境状态。点击“重新应用”可把已保存的环境显式写入设备，并执行硬件回读校验。

### DPI 与回报率

![PulseHub 设备设置](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20194146.jpg)

每个环境可设置 DPI、设备能力范围内的自定义 DPI、四档 DPI 循环，以及 1000、500、250、125 Hz 回报率。修改会在保存和应用前完成校验。

### 按键映射

![PulseHub 按键映射](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20194221.jpg)

可将中键、G4、G5、G6 设置为原生鼠标动作或受支持的键盘快捷键；左键和右键始终作为受保护的原生动作。已修改的按键可随时还原为鼠标原本功能。

### 内置应用环境

![PulseHub 应用环境](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20194318.jpg)

Office 与 CS2 分别拥有独立的 DPI、回报率、DPI 档位和按键映射。本页展示配置在保存或显式重新应用前的编辑流程。

### 为程序导入专属环境

![PulseHub 环境导入与配置](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20194344.jpg)

导入任意已有 `.exe` 文件，即可为游戏、设计软件或其他程序建立专属环境。每个导入程序均可保存独立的设备设置，并参与自动切换。

### 语言支持

![PulseHub 简体中文与 English 界面](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20194432.jpg)

PulseHub 提供简体中文与 English 双语界面；可在设置中选择默认显示语言，安装向导也支持相同的两种语言。

### 偏好设置与安全退出

![PulseHub 偏好设置与安全退出](docs/imges/%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%202026-07-23%20194421.jpg)

可设置登录启动、开发者日志以及退出安全环境。退出安全环境会在托盘代理结束前恢复指定的 DPI、回报率、DPI 档位与安全按键映射。产品不提供设备灯光控制，受支持的 G102 LIGHTSYNC 灯光保持关闭。

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
