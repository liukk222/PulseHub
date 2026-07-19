# PulseHub

PulseHub 是一个面向 Windows 11 的轻量鼠标配置工具。MVP 目标是为 Logitech G102 LIGHTSYNC 提供真实硬件 DPI 与一对一按键映射，并在办公和 CS2 环境之间自动切换。

当前仓库已完成 Cargo Workspace 初始化。此阶段只包含可构建的领域模型、设备接口、配置切换、配置存储与 IPC 骨架；尚未实现 Win32 托盘、Slint GUI 或 Logitech HID++ 通信。

## 构建

~~~powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
~~~

完整架构、协议验证门槛、IPC 和测试策略见 [实现文档](docs/IMPLEMENTATION.md)。

