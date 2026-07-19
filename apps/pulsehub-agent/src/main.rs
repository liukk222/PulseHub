use pulsehub_config_store::CONFIG_SCHEMA_VERSION;
use pulsehub_core::Environment;
use pulsehub_ipc::PROTOCOL_VERSION;

fn main() {
    let environment = Environment::Office;

    println!(
        "PulseHub agent skeleton: environment={environment:?}, config_schema={CONFIG_SCHEMA_VERSION}, ipc={PROTOCOL_VERSION}"
    );
    println!("Win32 托盘、设备事件和 HID++ 后端将在协议 POC 完成后接入。");
}
