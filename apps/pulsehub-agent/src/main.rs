#![forbid(unsafe_code)]

mod foreground;
mod watcher;

use std::env;
use std::process::ExitCode;
use std::time::Duration;

use pulsehub_config_store::{
    ConfigDocument, ProfileName, SelectionMode, default_config_path, load_or_create_default,
};
use pulsehub_core::Environment;
use pulsehub_device::hidpp::set_first_g102_dpi;
use pulsehub_ipc::PROTOCOL_VERSION;
use pulsehub_profile::{
    EnvironmentTracker, ProcessRule, SelectionPolicy, select_environment_with_rules,
};

#[derive(Debug, Default)]
struct Arguments {
    inspect_foreground: bool,
    apply_current_environment: bool,
    watch_foreground: bool,
    confirm_device_write: bool,
    exit_after_seconds: Option<u64>,
}

#[derive(Debug)]
struct EnvironmentTarget {
    executable_name: String,
    process_id: u64,
    environment: Environment,
    dpi: u16,
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

    if arguments.watch_foreground {
        run_watcher(&config, arguments.exit_after_seconds)
    } else if arguments.inspect_foreground || arguments.apply_current_environment {
        inspect_or_apply(&config, arguments.apply_current_environment)
    } else {
        println!("未请求设备操作；使用 --help 查看前台识别与自动切换参数。");
        ExitCode::SUCCESS
    }
}

fn inspect_or_apply(config: &ConfigDocument, apply: bool) -> ExitCode {
    let target = match resolve_target(config) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("前台进程识别失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    print_target(&target);
    if !apply {
        println!("只读识别完成，未写入设备。");
        return ExitCode::SUCCESS;
    }
    match apply_target(&target) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run_watcher(config: &ConfigDocument, exit_after_seconds: Option<u64>) -> ExitCode {
    let mut tracker = EnvironmentTracker::default();
    println!("自动切换监听已启动；按 Ctrl+C 退出。仅切换运行态 DPI，不写入板载闪存。");
    let result = watcher::run(exit_after_seconds.map(Duration::from_secs), || {
        let target = resolve_target(config)?;
        if tracker.observe(target.environment).is_none() {
            println!(
                "前台进程：{}；环境仍为 {:?}，跳过重复应用。",
                target.executable_name, target.environment
            );
            return Ok(());
        }
        print_target(&target);
        apply_target(&target)
    });
    match result {
        Ok(()) => {
            println!("自动切换监听已停止，Windows event hook 已卸载。");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("自动切换监听失败：{error}");
            ExitCode::FAILURE
        }
    }
}

fn resolve_target(config: &ConfigDocument) -> Result<EnvironmentTarget, String> {
    let foreground = foreground::current()?;
    let environment = selected_environment(config, Some(&foreground.executable_name));
    let dpi = match environment {
        Environment::Office => config.profiles.office.dpi,
        Environment::Cs2 => config.profiles.cs2.dpi,
    };
    Ok(EnvironmentTarget {
        executable_name: foreground.executable_name,
        process_id: foreground.process_id,
        environment,
        dpi,
    })
}

fn print_target(target: &EnvironmentTarget) {
    println!(
        "前台进程：{} (PID {})；目标环境={:?}，目标 DPI={}",
        target.executable_name, target.process_id, target.environment, target.dpi
    );
}

fn apply_target(target: &EnvironmentTarget) -> Result<(), String> {
    match set_first_g102_dpi(target.dpi, false) {
        Ok(result) if result.changed => {
            println!(
                "运行态 DPI 已从 {} 设置为 {}，回读通过。",
                result.before, result.after
            );
            Ok(())
        }
        Ok(result) => {
            println!("运行态 DPI 已是 {}，跳过重复写入。", result.after);
            Ok(())
        }
        Err(error) => Err(format!("运行态 DPI 应用失败：{error}")),
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
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--inspect-foreground" => parsed.inspect_foreground = true,
            "--apply-current-environment" => parsed.apply_current_environment = true,
            "--watch-foreground" => parsed.watch_foreground = true,
            "--confirm-device-write" => parsed.confirm_device_write = true,
            "--exit-after-seconds" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--exit-after-seconds 后必须提供秒数".to_owned())?;
                let seconds = value
                    .parse::<u64>()
                    .map_err(|_| format!("无效秒数：{value}"))?;
                if !(1..=3600).contains(&seconds) {
                    return Err("--exit-after-seconds 必须为 1–3600".to_owned());
                }
                parsed.exit_after_seconds = Some(seconds);
            }
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            _ => return Err(format!("未知参数：{argument}")),
        }
    }
    let modes = u8::from(parsed.inspect_foreground)
        + u8::from(parsed.apply_current_environment)
        + u8::from(parsed.watch_foreground);
    if modes > 1 {
        return Err("前台检查、单次应用和自动监听模式不能同时使用".to_owned());
    }
    let writing = parsed.apply_current_environment || parsed.watch_foreground;
    if writing != parsed.confirm_device_write {
        return Err("设备应用或自动监听必须与 --confirm-device-write 同时提供".to_owned());
    }
    if parsed.exit_after_seconds.is_some() && !parsed.watch_foreground {
        return Err("--exit-after-seconds 只能与 --watch-foreground 一起使用".to_owned());
    }
    Ok(parsed)
}

fn print_help() {
    println!("用法：pulsehub-agent [--inspect-foreground]");
    println!("      pulsehub-agent --apply-current-environment --confirm-device-write");
    println!(
        "      pulsehub-agent --watch-foreground --confirm-device-write [--exit-after-seconds <1-3600>]"
    );
    println!("  --inspect-foreground  只读显示前台进程、目标环境和 DPI");
    println!("  --apply-current-environment  按当前前台进程应用一次运行态 DPI");
    println!("  --watch-foreground  监听 Windows 前台事件并自动切换运行态 DPI");
    println!("  --confirm-device-write  显式确认本次进程中的 DPI 设备写入");
    println!("  --exit-after-seconds  验证用的自动退出时间；省略时按 Ctrl+C 退出");
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
