#![forbid(unsafe_code)]

mod foreground;

use std::env;
use std::process::ExitCode;

use pulsehub_config_store::{
    ConfigDocument, ProfileName, SelectionMode, default_config_path, load_or_create_default,
};
use pulsehub_core::Environment;
use pulsehub_device::hidpp::set_first_g102_dpi;
use pulsehub_ipc::PROTOCOL_VERSION;
use pulsehub_profile::{ProcessRule, SelectionPolicy, select_environment_with_rules};

#[derive(Debug, Default)]
struct Arguments {
    inspect_foreground: bool,
    apply_current_environment: bool,
    confirm_device_write: bool,
}

fn main() -> ExitCode {
    let arguments = match parse_arguments() {
        Ok(arguments) => arguments,
        Err(message) => {
            eprintln!("{message}");
            print_help();
            return ExitCode::from(2);
        }
    };
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
        "PulseHub agent: selection={:?}, config_schema={}, ipc={PROTOCOL_VERSION}",
        config.selection.mode, config.schema_version
    );
    println!("配置：{}", path.display());
    println!(
        "Office={} DPI，CS2={} DPI",
        config.profiles.office.dpi, config.profiles.cs2.dpi
    );

    if arguments.inspect_foreground || arguments.apply_current_environment {
        inspect_or_apply(&config, &arguments)
    } else {
        println!("未请求设备操作；使用 --help 查看一次性前台识别与应用参数。");
        ExitCode::SUCCESS
    }
}

fn inspect_or_apply(config: &ConfigDocument, arguments: &Arguments) -> ExitCode {
    let foreground = match foreground::current() {
        Ok(foreground) => foreground,
        Err(error) => {
            eprintln!("前台进程识别失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    let environment = selected_environment(config, Some(&foreground.executable_name));
    let dpi = match environment {
        Environment::Office => config.profiles.office.dpi,
        Environment::Cs2 => config.profiles.cs2.dpi,
    };
    println!(
        "前台进程：{} (PID {})；目标环境={environment:?}，目标 DPI={dpi}",
        foreground.executable_name, foreground.process_id
    );

    if !arguments.apply_current_environment {
        println!("只读识别完成，未写入设备。");
        return ExitCode::SUCCESS;
    }
    match set_first_g102_dpi(dpi, false) {
        Ok(result) if result.changed => {
            println!(
                "运行态 DPI 已从 {} 设置为 {}，回读通过。",
                result.before, result.after
            );
            ExitCode::SUCCESS
        }
        Ok(result) => {
            println!("运行态 DPI 已是 {}，跳过重复写入。", result.after);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("运行态 DPI 应用失败：{error}");
            ExitCode::FAILURE
        }
    }
}

fn selected_environment(config: &ConfigDocument, executable_name: Option<&str>) -> Environment {
    let policy = match config.selection.mode {
        SelectionMode::Auto => SelectionPolicy::Auto,
        SelectionMode::Office => SelectionPolicy::Fixed(Environment::Office),
        SelectionMode::Cs2 => SelectionPolicy::Fixed(Environment::Cs2),
    };
    let rules = config
        .selection
        .rules
        .iter()
        .map(|rule| ProcessRule {
            environment: match rule.environment {
                ProfileName::Office => Environment::Office,
                ProfileName::Cs2 => Environment::Cs2,
            },
            process_names: rule.process_names.clone(),
        })
        .collect::<Vec<_>>();
    select_environment_with_rules(policy, executable_name, &rules)
}

fn parse_arguments() -> Result<Arguments, String> {
    let mut parsed = Arguments::default();
    for argument in env::args().skip(1) {
        match argument.as_str() {
            "--inspect-foreground" => parsed.inspect_foreground = true,
            "--apply-current-environment" => parsed.apply_current_environment = true,
            "--confirm-device-write" => parsed.confirm_device_write = true,
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            _ => return Err(format!("未知参数：{argument}")),
        }
    }
    if parsed.apply_current_environment != parsed.confirm_device_write {
        return Err(
            "--apply-current-environment 必须与 --confirm-device-write 同时提供".to_owned(),
        );
    }
    Ok(parsed)
}

fn print_help() {
    println!("用法：pulsehub-agent [--inspect-foreground]");
    println!("      pulsehub-agent --apply-current-environment --confirm-device-write");
    println!("  --inspect-foreground  只读显示前台进程、目标环境和 DPI");
    println!("  --apply-current-environment  按当前前台进程应用运行态 DPI");
    println!("  --confirm-device-write  显式确认本次 DPI 设备写入");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_cs2_rule_selects_cs2() {
        let config = ConfigDocument::default();
        assert_eq!(
            selected_environment(&config, Some("CS2.EXE")),
            Environment::Cs2
        );
        assert_eq!(
            selected_environment(&config, Some("explorer.exe")),
            Environment::Office
        );
    }
}
