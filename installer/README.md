# PulseHub Windows 11 安装器

安装向导顺序：

1. 选择安装向导语言（简体中文或 English）。
2. 阅读并接受中英双语安装与使用协议；拒绝时不能继续。
3. 阅读第三方与兼容性研究声明。
4. 选择安装目录。
5. 选择 PulseHub 默认界面语言。
6. 安装，可选立即启动 PulseHub。

构建：

```powershell
.\installer\build-installer.ps1
```

脚本会构建 Rust Release 程序、从 Inno Setup 官方源码仓库取得简体中文语言文件、从 GUI 可执行文件提取橙色 P 图标，并调用 Inno Setup 6 生成单文件 Windows x64 安装器。输出位于 `installer/output`。

安装包包含安装风险协议 `LICENSE-AGREEMENT.txt`、安装器兼容性说明 `THIRD_PARTY_NOTICES.txt`、PulseHub 的 MIT `LICENSE`，以及项目完整第三方声明 `THIRD_PARTY_NOTICES.md`。安装风险协议不替代、限制或修改 MIT 许可证授予的源码权利。
