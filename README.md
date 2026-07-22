# PulseHub

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev/)

PulseHub 是一个面向 Windows 11 的轻量鼠标配置工具。MVP 目标是为 Logitech G102 LIGHTSYNC 提供真实硬件 DPI 与一对一按键映射，并在办公和 CS2 环境之间自动切换。

当前仓库已实现并通过 G102 实机验证：Windows HID/HID++ 探测、DPI 与按键配置读写、Office/CS2 自动切换、设备重连恢复、Named Pipe IPC、Slint GUI、系统托盘和可选开发者日志。`pulsehub-agent.exe` 独立负责设备控制，关闭 GUI 后自动切换仍会继续运行。

## G102 探测

~~~powershell
# 只读：枚举 HID、功能表和当前 DPI
cargo run -p pulsehub-probe

# 写入：缺少确认参数时程序会拒绝执行
cargo run -p pulsehub-probe -- --set-dpi 800 --confirm-device-write
~~~

`--set-dpi` 会先按设备运行时公布的范围和步进校验数值，写入后再次读取 DPI；回读不一致时返回失败。该命令修改真实硬件 DPI，执行前应退出 Logitech G HUB。

## 构建

~~~powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
~~~

若全局 Cargo 镜像无法返回 `config.json` 或依赖索引，应修复/移除该镜像配置后再构建；不要修改 `Cargo.lock` 来绕过索引故障。

完整架构、协议验证门槛、IPC 和测试策略见 [实现文档索引](docs/IMPLEMENTATION.md)。继续开发前请阅读 [下一阶段开发交接](docs/NEXT_STEPS.md)。

## 许可证

PulseHub 以 [MIT 许可证](LICENSE)开源。第三方组件及兼容性研究参考资料的声明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)，Windows 依赖审计见 [docs/DEPENDENCY_LICENSE_AUDIT.md](docs/DEPENDENCY_LICENSE_AUDIT.md)。

PulseHub 是独立开源项目，与 Logitech 不存在关联，也未获得其认可或赞助。Logitech 及相关产品名称是其各自权利人的商标。
