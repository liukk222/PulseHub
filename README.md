# PulseHub

PulseHub 是一个面向 Windows 11 的轻量鼠标配置工具。MVP 目标是为 Logitech G102 LIGHTSYNC 提供真实硬件 DPI 与一对一按键映射，并在办公和 CS2 环境之间自动切换。

当前仓库已完成 Cargo Workspace、Windows HID 枚举、HID++ 功能发现和 DPI 读写 POC。默认探测只读；DPI 写入必须同时提供目标值与显式设备写入确认。Win32 托盘、Slint GUI、Named Pipe 和自动配置切换仍未实现。

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

完整架构、协议验证门槛、IPC 和测试策略见 [实现文档](docs/IMPLEMENTATION.md)。
