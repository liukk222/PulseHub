use std::env;
use std::process::ExitCode;

use pulsehub_device::discovery::{HidCollectionInfo, enumerate_hid_collections};
use pulsehub_device::hidpp::{HidppProbeResult, probe_first_g102, set_first_g102_dpi};

fn main() -> ExitCode {
    let mut include_all = false;
    let mut protocol_trace = false;
    let mut requested_dpi = None;
    let mut confirm_device_write = false;
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--all" => include_all = true,
            "--protocol-trace" => protocol_trace = true,
            "--confirm-device-write" => confirm_device_write = true,
            "--set-dpi" => {
                let Some(value) = arguments.next() else {
                    eprintln!("--set-dpi 后必须提供 DPI 数值");
                    return ExitCode::from(2);
                };
                requested_dpi = match value.parse::<u16>() {
                    Ok(value) => Some(value),
                    Err(_) => {
                        eprintln!("无效 DPI 数值：{value}");
                        return ExitCode::from(2);
                    }
                };
            }
            "-h" | "--help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            unknown => {
                eprintln!("未知参数：{unknown}");
                print_help();
                return ExitCode::from(2);
            }
        }
    }

    if requested_dpi.is_some() && !confirm_device_write {
        eprintln!("拒绝写入：--set-dpi 必须同时提供 --confirm-device-write");
        return ExitCode::from(2);
    }
    if requested_dpi.is_none() && confirm_device_write {
        eprintln!("--confirm-device-write 只能与 --set-dpi 一起使用");
        return ExitCode::from(2);
    }

    println!("PulseHub HID 只读探测");
    println!(
        "模式：{}",
        if include_all {
            "全部 HID collection"
        } else {
            "Logitech collection"
        }
    );
    if let Some(dpi) = requested_dpi {
        println!("写入模式：已显式请求将 DPI 设置为 {dpi}，完成后执行回读校验。\n");
    } else {
        println!("安全性：本工具仅枚举设备并执行 HID++ 查询，不发送配置写入。\n");
    }

    let collections = match enumerate_hid_collections(include_all) {
        Ok(collections) => collections,
        Err(error) => {
            eprintln!("探测失败：{error}");
            return ExitCode::FAILURE;
        }
    };

    if collections.is_empty() {
        println!("未发现匹配的 HID collection。");
        return ExitCode::SUCCESS;
    }

    for (index, collection) in collections.iter().enumerate() {
        print_collection(index + 1, collection);
    }

    let known_g102_count = collections
        .iter()
        .filter(|item| item.is_known_g102())
        .count();
    let vendor_interface_count = collections
        .iter()
        .filter(|item| item.is_known_g102() && item.is_vendor_defined())
        .count();
    println!(
        "汇总：{} 个 collection，{} 个已知 G102/G203 collection，{} 个厂商自定义候选接口。",
        collections.len(),
        known_g102_count,
        vendor_interface_count
    );

    if known_g102_count > 0 {
        println!("\n开始 HID++ 只读能力查询……");
        match probe_first_g102(protocol_trace) {
            Ok(result) => print_hidpp_probe(&result),
            Err(error) => {
                eprintln!("HID++ 探测失败：{error}");
                return ExitCode::FAILURE;
            }
        }
    }

    if let Some(dpi) = requested_dpi {
        println!("\n执行已确认的 DPI 写入……");
        match set_first_g102_dpi(dpi, protocol_trace) {
            Ok(result) => println!(
                "DPI 写入成功：传感器 {}，写入前 {}，请求 {}，回读 {}。",
                result.sensor_index, result.before, result.requested, result.after
            ),
            Err(error) => {
                eprintln!("DPI 写入失败：{error}");
                return ExitCode::FAILURE;
            }
        }
    }

    ExitCode::SUCCESS
}

fn print_collection(index: usize, collection: &HidCollectionInfo) {
    let model = if collection.is_known_g102() {
        "G102/G203 候选"
    } else if collection.is_logitech() {
        "其他 Logitech 设备"
    } else {
        "其他 HID 设备"
    };

    println!("[{index}] {model}");
    println!(
        "  USB: {:04x}:{:04x} release={:04x} bus={}",
        collection.vendor_id, collection.product_id, collection.release_number, collection.bus_type
    );
    println!(
        "  名称: manufacturer={} product={} serial={}",
        display_optional(collection.manufacturer.as_deref()),
        display_optional(collection.product.as_deref()),
        display_serial(collection.serial_number.as_deref())
    );
    println!(
        "  Collection: interface={} usage_page=0x{:04x} usage=0x{:04x}",
        collection.interface_number, collection.usage_page, collection.usage
    );

    match (
        collection.report_descriptor_length,
        collection.report_lengths,
    ) {
        (Some(descriptor), Some(reports)) => println!(
            "  报告: descriptor={}B input={}B output={}B feature={}B",
            descriptor, reports.input, reports.output, reports.feature
        ),
        _ => println!("  报告: 无法读取"),
    }
    if let Some(error) = &collection.open_error {
        println!("  读取警告: {error}");
    }
    println!();
}

fn display_optional(value: Option<&str>) -> &str {
    value.filter(|value| !value.is_empty()).unwrap_or("<无>")
}

fn display_serial(value: Option<&str>) -> &str {
    if value.is_some_and(|value| !value.is_empty()) {
        "<已隐藏>"
    } else {
        "<无>"
    }
}

fn print_help() {
    println!(
        "用法：pulsehub-probe [--all] [--protocol-trace] [--set-dpi <DPI> --confirm-device-write]"
    );
    println!("  --all  枚举全部 HID collection；默认只显示 Logitech 设备");
    println!("  --protocol-trace  输出有上限的原始 HID++ 请求与响应");
    println!("  --set-dpi <DPI>  设置真实硬件 DPI；单独使用时拒绝执行");
    println!("  --confirm-device-write  显式确认本次 --set-dpi 写入");
}

fn print_hidpp_probe(result: &HidppProbeResult) {
    println!(
        "HID++ 协议：{}.{}，功能数：{}",
        result.protocol_major,
        result.protocol_minor,
        result.features.len()
    );
    for feature in &result.features {
        println!(
            "  feature index=0x{:02x} id=0x{:04x} type=0x{:02x} version={}",
            feature.index, feature.id, feature.feature_type, feature.version
        );
    }
    if result.dpi_sensors.is_empty() {
        println!("DPI：设备未公开 ADJUSTABLE_DPI (0x2201)");
    } else {
        for sensor in &result.dpi_sensors {
            let range = match sensor.step {
                Some(step) => format!("{}–{}，步进 {}", sensor.minimum, sensor.maximum, step),
                None => format!("离散值 {:?}", sensor.discrete_values),
            };
            println!(
                "DPI 传感器 {}：{}；当前 {}；默认 {}",
                sensor.index, range, sensor.current, sensor.default
            );
        }
    }
}
