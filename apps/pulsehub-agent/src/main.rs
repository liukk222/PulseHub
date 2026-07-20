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
use pulsehub_device::hidpp::{HidppError, probe_first_g102, set_first_g102_dpi};
use pulsehub_ipc::{AgentSnapshot, DeviceStatus, Environment as IpcEnvironment, PROTOCOL_VERSION};
use pulsehub_profile::{
    EnvironmentTracker, ProcessRule, RetryBackoff, SelectionPolicy, select_environment_with_rules,
};

#[derive(Debug, Default)]
struct Arguments {
    inspect_foreground: bool,
    apply_current_environment: bool,
    watch_foreground: bool,
    serve_ipc_once: bool,
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

#[derive(Debug)]
struct ApplyFailure {
    message: String,
    retryable: bool,
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
    #[cfg(windows)]
    let _single_instance = match single_instance::SingleInstance::new("Local\\PulseHub.Agent.v1") {
        Ok(instance) if instance.is_single() => instance,
        Ok(_) => {
            eprintln!("PulseHub agent 已在当前用户会话中运行，拒绝启动第二个实例。");
            return ExitCode::from(3);
        }
        Err(error) => {
            eprintln!("PulseHub 单实例锁创建失败：{error}");
            return ExitCode::FAILURE;
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
    } else if arguments.serve_ipc_once {
        serve_ipc_once(&config)
    } else if arguments.inspect_foreground || arguments.apply_current_environment {
        inspect_or_apply(&config, arguments.apply_current_environment)
    } else {
        println!("未请求设备操作；使用 --help 查看前台识别与自动切换参数。");
        ExitCode::SUCCESS
    }
}

#[cfg(windows)]
fn serve_ipc_once(config: &ConfigDocument) -> ExitCode {
    use pulsehub_ipc::windows::{DEFAULT_PIPE_PATH, Server};

    let target = match resolve_target(config) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("IPC 快照构造失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    println!("正在执行 HID++ 只读状态查询……");
    let probe = probe_first_g102(false);
    let (device_status, current_dpi) = match &probe {
        Ok(result) => match result.dpi_sensors.first() {
            Some(sensor) => (DeviceStatus::Ready, Some(sensor.current)),
            None => (DeviceStatus::Degraded, None),
        },
        Err(HidppError::InterfaceNotFound) => (DeviceStatus::Disconnected, None),
        Err(HidppError::Timeout) => (DeviceStatus::Busy, None),
        Err(_) => (DeviceStatus::Degraded, None),
    };
    if let Err(error) = &probe {
        eprintln!("HID++ 状态查询未完成：{error}；仍提供降级快照。");
    }
    let snapshot = snapshot_for_target(&target, 0, device_status, current_dpi);
    let server = match Server::bind(DEFAULT_PIPE_PATH) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("IPC 管道创建失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    println!("IPC 单连接验证服务已启动：{DEFAULT_PIPE_PATH}");
    println!("提供包含 HID 只读状态的脱敏快照；等待 pulsehub-config 客户端……");
    let mut stream = match server.accept() {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("IPC 客户端连接失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    match server.serve_connection(&mut stream, &snapshot) {
        Ok(()) => {
            println!("IPC 客户端已断开，单连接验证服务停止。");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("IPC 会话失败：{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn serve_ipc_once(_: &ConfigDocument) -> ExitCode {
    eprintln!("IPC Named Pipe 验证模式仅支持 Windows。");
    ExitCode::FAILURE
}

fn inspect_or_apply(config: &ConfigDocument, apply: bool) -> ExitCode {
    let target = match resolve_target(config) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("前台进程识别失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    let snapshot = snapshot_for_target(&target, 0, DeviceStatus::Unknown, None);
    print_target(&target, &snapshot);
    if !apply {
        println!("只读识别完成，未写入设备。");
        return ExitCode::SUCCESS;
    }
    match apply_target(&target) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", error.message);
            ExitCode::FAILURE
        }
    }
}

fn snapshot_for_target(
    target: &EnvironmentTarget,
    config_revision: u64,
    device_status: DeviceStatus,
    current_dpi: Option<u16>,
) -> AgentSnapshot {
    AgentSnapshot {
        device_status,
        active_environment: match target.environment {
            Environment::Office => IpcEnvironment::Office,
            Environment::Cs2 => IpcEnvironment::Cs2,
        },
        config_revision,
        current_dpi,
        desired_dpi: target.dpi,
    }
}

fn run_watcher(config: &ConfigDocument, exit_after_seconds: Option<u64>) -> ExitCode {
    let mut tracker = EnvironmentTracker::default();
    let mut backoff = RetryBackoff::default();
    println!("自动切换监听已启动；按 Ctrl+C 退出。仅切换运行态 DPI，不写入板载闪存。");
    let result = watcher::run(exit_after_seconds.map(Duration::from_secs), || {
        let target = match resolve_target(config) {
            Ok(target) => target,
            Err(error) => {
                let delay = backoff.record_failure();
                eprintln!(
                    "前台进程识别暂时失败：{error}；{} ms 后重试。",
                    delay.as_millis()
                );
                return Some(delay);
            }
        };
        if tracker.current() == Some(target.environment) {
            println!(
                "前台进程：{}；环境仍为 {:?}，跳过重复应用。",
                target.executable_name, target.environment
            );
            return None;
        }
        let snapshot = snapshot_for_target(&target, 0, DeviceStatus::Unknown, None);
        print_target(&target, &snapshot);
        match apply_target(&target) {
            Ok(()) => {
                tracker.observe(target.environment);
                backoff.record_success();
                None
            }
            Err(error) if error.retryable => {
                let delay = backoff.record_failure();
                eprintln!("{}；{} ms 后重试。", error.message, delay.as_millis());
                Some(delay)
            }
            Err(error) => {
                eprintln!("{}；该错误不会自动重试。", error.message);
                None
            }
        }
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

fn print_target(target: &EnvironmentTarget, snapshot: &AgentSnapshot) {
    println!(
        "前台进程：{} (PID {})；目标环境={:?}，目标 DPI={}",
        target.executable_name,
        target.process_id,
        snapshot.active_environment,
        snapshot.desired_dpi
    );
}

fn apply_target(target: &EnvironmentTarget) -> Result<(), ApplyFailure> {
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
        Err(error) => {
            let retryable = !matches!(
                error,
                HidppError::PlatformUnsupported | HidppError::InvalidDpi { .. }
            );
            Err(ApplyFailure {
                message: format!("运行态 DPI 应用失败：{error}"),
                retryable,
            })
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
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--inspect-foreground" => parsed.inspect_foreground = true,
            "--apply-current-environment" => parsed.apply_current_environment = true,
            "--watch-foreground" => parsed.watch_foreground = true,
            "--serve-ipc-once" => parsed.serve_ipc_once = true,
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
        + u8::from(parsed.watch_foreground)
        + u8::from(parsed.serve_ipc_once);
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
    println!("      pulsehub-agent --serve-ipc-once");
    println!("  --inspect-foreground  只读显示前台进程、目标环境和 DPI");
    println!("  --apply-current-environment  按当前前台进程应用一次运行态 DPI");
    println!("  --watch-foreground  监听 Windows 前台事件并自动切换运行态 DPI");
    println!("  --serve-ipc-once  只读服务一个 IPC 客户端，断开后退出");
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

    #[test]
    fn target_maps_to_sanitized_ipc_snapshot() {
        let target = EnvironmentTarget {
            executable_name: "private-process.exe".to_owned(),
            process_id: 42,
            environment: Environment::Cs2,
            dpi: 800,
        };

        let snapshot = snapshot_for_target(&target, 7, DeviceStatus::Ready, Some(800));

        assert_eq!(snapshot.device_status, DeviceStatus::Ready);
        assert_eq!(snapshot.active_environment, IpcEnvironment::Cs2);
        assert_eq!(snapshot.config_revision, 7);
        assert_eq!(snapshot.current_dpi, Some(800));
        assert_eq!(snapshot.desired_dpi, 800);
    }
}
