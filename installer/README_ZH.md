# PulseHub Windows 11 安装器

**简体中文** | [English](README.md)

本目录包含 PulseHub Windows 11 x64 安装程序的 Inno Setup 源码和 PowerShell 构建脚本。

## 安装向导流程

生成的安装程序按以下顺序显示页面：

1. 选择安装向导语言：简体中文或 English。
2. 阅读并接受中英双语安装与使用协议；不同意时不能继续安装。
3. 阅读第三方与兼容性研究声明。
4. 选择安装目录。
5. 选择 PulseHub 默认界面语言。
6. 安装 PulseHub，并可选择立即启动。

## 环境要求

- Windows 11 x64
- 建议使用 PowerShell 7
- Rust 1.97 或更高版本，以及 `x86_64-pc-windows-msvc` 工具链
- Microsoft C++ Build Tools
- [Inno Setup 6](https://jrsoftware.org/isinfo.php)

可通过 Windows 程序包管理器安装 Inno Setup：

```powershell
winget install --id JRSoftware.InnoSetup -e
```

## 构建

在仓库根目录执行构建脚本：

```powershell
.\installer\build-installer.ps1
```

脚本会：

1. 构建经过优化的 `pulsehub-agent.exe` 和 `pulsehub-config.exe`；
2. 在没有缓存时，从 Inno Setup 官方源码仓库下载简体中文语言文件；
3. 使用固定的 SHA-256 校验该语言文件；
4. 从 GUI 程序中提取橙色 PulseHub P 图标；
5. 调用 Inno Setup 命令行编译器；
6. 输出安装程序路径和 SHA-256。

安装程序输出到：

```text
installer\output\PulseHub-Setup-0.1.3-windows-x64.exe
```

`installer\build` 和 `installer\output` 是生成目录，已被 Git 忽略。

## 复用已有 Release 程序

如果 `target\release` 中已经存在所需的 Rust Release 程序，可以跳过 Rust 构建：

```powershell
.\installer\build-installer.ps1 -SkipRustBuild
```

缺少任意一个所需程序时，脚本会停止并报告错误。

## 安装包内的声明

安装程序包含：

- `LICENSE-AGREEMENT.txt`：中英双语安装风险与使用协议；
- `THIRD_PARTY_NOTICES.txt`：中英双语安装器兼容性声明；
- 根目录 `LICENSE`：PulseHub MIT 许可证；
- 根目录 `THIRD_PARTY_NOTICES.md`：项目完整第三方声明。

安装协议不会替代、限制或修改 MIT 许可证授予的源码权利。

## 发布校验

构建完成后验证安装程序：

```powershell
Get-FileHash .\installer\output\PulseHub-Setup-0.1.3-windows-x64.exe -Algorithm SHA256
Get-AuthenticodeSignature .\installer\output\PulseHub-Setup-0.1.3-windows-x64.exe
```

开源 v0.1.3 安装包尚未进行数字签名，Windows SmartScreen 可能提示“未知发布者”。公开发布时应同时上传独立的 SHA-256 校验文件。

## 文件说明

```text
PulseHub.iss                Inno Setup 定义
build-installer.ps1         可重复执行的安装器构建脚本
LICENSE-AGREEMENT.txt       中英双语安装协议
THIRD_PARTY_NOTICES.txt     中英双语安装器兼容性声明
README.md                   英文文档
README_ZH.md                简体中文文档
```
