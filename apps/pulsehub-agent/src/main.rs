use std::process::ExitCode;

use pulsehub_config_store::{default_config_path, load_or_create_default};
use pulsehub_ipc::PROTOCOL_VERSION;

fn main() -> ExitCode {
    let path = match default_config_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("PulseHub 配置路径错误：{error}");
            return ExitCode::FAILURE;
        }
    };
    let config = match load_or_create_default(&path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("PulseHub 配置加载失败：{error}");
            return ExitCode::FAILURE;
        }
    };

    println!(
        "PulseHub agent skeleton: selection={:?}, config_schema={}, ipc={PROTOCOL_VERSION}",
        config.selection.mode, config.schema_version
    );
    println!("配置：{}", path.display());
    println!(
        "Office={} DPI，CS2={} DPI",
        config.profiles.office.dpi, config.profiles.cs2.dpi
    );
    println!("Win32 托盘、设备事件和 HID++ 后端将在后续步骤接入。");
    ExitCode::SUCCESS
}
