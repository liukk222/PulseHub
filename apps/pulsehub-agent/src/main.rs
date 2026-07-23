#![cfg_attr(windows, windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod foreground;
mod local_log;
mod watcher;

#[cfg(windows)]
use std::cell::RefCell;
use std::env;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use pulsehub_config_store::{
    ButtonActionConfig, ConfigDocument, ConfigError, ConfigRepository, ProfileConfig, ProfileName,
    SelectionMode, default_config_path, load_or_create_default,
};
use pulsehub_core::Environment;
use pulsehub_device::hidpp::{
    DpiWriteResult, HidppError, OnboardButtonAction, activate_first_g102_onboard_mode,
    apply_first_g102_profile, ensure_first_g102_lighting_off, probe_first_g102,
    read_first_g102_dpi, set_first_g102_dpi,
};
use pulsehub_ipc::{
    AgentSnapshot, DeviceStatus, DpiCapability, Environment as IpcEnvironment, IntegrationStatus,
    PROTOCOL_VERSION,
};
use pulsehub_profile::{
    EnvironmentTracker, ProcessRule, RetryBackoff, SelectionPolicy, select_environment_with_rules,
};

#[derive(Debug, Default)]
struct Arguments {
    inspect_foreground: bool,
    apply_current_environment: bool,
    watch_foreground: bool,
    serve_ipc_once: bool,
    serve_ipc_apply_once: bool,
    serve_ipc: bool,
    run_agent: bool,
    confirm_device_write: bool,
    exit_after_seconds: Option<u64>,
}

#[derive(Debug)]
struct EnvironmentTarget {
    executable_name: String,
    process_id: u64,
    environment: Environment,
    profile_key: String,
    profile_name: String,
    dpi: u16,
    report_rate_hz: u16,
    button_actions: [OnboardButtonAction; 6],
    dpi_levels: [u16; 4],
}

#[derive(Debug)]
struct ApplyFailure {
    message: String,
    retryable: bool,
    device_disconnected: bool,
}

const ENVIRONMENT_STABLE_FOR: Duration = Duration::from_secs(1);
const PROFILE_APPLY_COOLDOWN: Duration = Duration::from_secs(5);
const SHUTDOWN_DEVICE_WAIT: Duration = Duration::from_secs(10);
const SHUTDOWN_DEVICE_RETRY: Duration = Duration::from_millis(250);
const SHUTDOWN_MAX_ATTEMPTS: u8 = 3;

fn shutdown_marker_path() -> std::path::PathBuf {
    std::env::temp_dir().join("PulseHub-ShuttingDown-v1")
}

fn create_shutdown_marker() {
    let _ = std::fs::write(shutdown_marker_path(), b"shutdown");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwitchDecision {
    AlreadyActive,
    Wait(Duration),
    Apply,
}

#[derive(Debug, Default)]
struct EnvironmentSwitchGuard {
    pending: Option<(String, Instant)>,
    last_applied_at: Option<Instant>,
}

const DEVICE_FAILURE_THRESHOLD: u8 = 3;

#[derive(Debug, Default)]
struct DeviceHealthMonitor {
    consecutive_failures: u8,
    unhealthy: bool,
}

impl DeviceHealthMonitor {
    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.unhealthy = false;
    }

    fn record_failure(&mut self) -> bool {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if !self.unhealthy && self.consecutive_failures >= DEVICE_FAILURE_THRESHOLD {
            self.unhealthy = true;
            return true;
        }
        false
    }
}

impl EnvironmentSwitchGuard {
    fn decide(&mut self, target: &str, active: Option<&str>, now: Instant) -> SwitchDecision {
        if active == Some(target) {
            self.pending = None;
            return SwitchDecision::AlreadyActive;
        }
        if self
            .pending
            .as_ref()
            .is_none_or(|(environment, _)| environment != target)
        {
            self.pending = Some((target.to_owned(), now));
            return SwitchDecision::Wait(ENVIRONMENT_STABLE_FOR);
        }
        let pending_since = self.pending.as_ref().expect("待应用环境必须存在").1;
        let stable_elapsed = now.saturating_duration_since(pending_since);
        if stable_elapsed < ENVIRONMENT_STABLE_FOR {
            return SwitchDecision::Wait(ENVIRONMENT_STABLE_FOR - stable_elapsed);
        }
        if let Some(last_applied_at) = self.last_applied_at {
            let cooldown_elapsed = now.saturating_duration_since(last_applied_at);
            if cooldown_elapsed < PROFILE_APPLY_COOLDOWN {
                return SwitchDecision::Wait(PROFILE_APPLY_COOLDOWN - cooldown_elapsed);
            }
        }
        SwitchDecision::Apply
    }

    fn record_applied(&mut self, now: Instant) {
        self.pending = None;
        self.last_applied_at = Some(now);
    }
}

#[cfg(windows)]
struct CommandFailure {
    code: pulsehub_ipc::ErrorCode,
    message: String,
    retryable: bool,
}

#[cfg(windows)]
enum DeviceCommand {
    ApplyNow {
        reply: std::sync::mpsc::SyncSender<Result<serde_json::Value, CommandFailure>>,
    },
    Shutdown {
        force: bool,
        reply: std::sync::mpsc::SyncSender<Result<serde_json::Value, CommandFailure>>,
    },
    ValidateDraft {
        draft: serde_json::Value,
        reply: std::sync::mpsc::SyncSender<Result<serde_json::Value, CommandFailure>>,
    },
    CommitConfig {
        base_revision: u64,
        draft: serde_json::Value,
        reply: std::sync::mpsc::SyncSender<Result<serde_json::Value, CommandFailure>>,
    },
    SetSelectionMode {
        mode: pulsehub_ipc::SelectionMode,
        reply: std::sync::mpsc::SyncSender<Result<serde_json::Value, CommandFailure>>,
    },
}

fn main() -> ExitCode {
    let _ = std::fs::remove_file(shutdown_marker_path());
    if let Err(error) = local_log::initialize() {
        eprintln!("本地日志初始化失败：{error}");
    }
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
    local_log::set_enabled(config.agent.developer_logging);

    println!(
        "PulseHub agent: selection={:?}, config_schema={}, ipc={PROTOCOL_VERSION}",
        config.selection.mode, config.schema_version
    );
    println!("配置：{}", path.display());
    println!(
        "Office={} DPI，CS2={} DPI",
        config.profiles.office.dpi, config.profiles.cs2.dpi
    );

    if arguments.run_agent {
        run_agent(&path, &config, arguments.exit_after_seconds)
    } else if arguments.watch_foreground {
        run_watcher(&config, arguments.exit_after_seconds)
    } else if arguments.serve_ipc {
        serve_ipc(&config, arguments.exit_after_seconds)
    } else if arguments.serve_ipc_once || arguments.serve_ipc_apply_once {
        serve_ipc_once(&config, arguments.serve_ipc_apply_once)
    } else if arguments.inspect_foreground || arguments.apply_current_environment {
        inspect_or_apply(&config, arguments.apply_current_environment)
    } else {
        println!("未请求设备操作；使用 --help 查看前台识别与自动切换参数。");
        ExitCode::SUCCESS
    }
}

#[cfg(windows)]
fn run_agent(path: &Path, config: &ConfigDocument, exit_after_seconds: Option<u64>) -> ExitCode {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, RwLock, mpsc};
    use std::thread;

    use pulsehub_ipc::windows::{
        Server, connect, default_pipe_path, serve_connection_with_handler,
    };
    use pulsehub_ipc::{ErrorBody, ErrorCode, Request, Response};

    let pipe_path = match default_pipe_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("IPC 管道名构造失败：{error}");
            return ExitCode::FAILURE;
        }
    };

    let repository = match ConfigRepository::from_document(path, config.clone()) {
        Ok(repository) => RefCell::new(repository),
        Err(error) => {
            eprintln!("配置仓库初始化失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = sync_start_with_windows(config.agent.start_with_windows) {
        eprintln!("登录启动项同步失败：{error}");
        local_log::error(format_args!("登录启动项同步失败：{error}"));
    }
    let mut initial = match live_snapshot(config) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("代理初始快照构造失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    initial.config_revision = repository.borrow().revision();
    let snapshot = Arc::new(RwLock::new(initial));
    let stopping = Arc::new(AtomicBool::new(false));
    let active_clients = Arc::new(AtomicUsize::new(0));
    let (command_tx, command_rx) = mpsc::sync_channel::<DeviceCommand>(16);
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let ipc_thread = {
        let snapshot = Arc::clone(&snapshot);
        let stopping = Arc::clone(&stopping);
        let active_clients = Arc::clone(&active_clients);
        let command_tx = command_tx.clone();
        let pipe_path = pipe_path.clone();
        thread::spawn(move || {
            let server = match Server::bind(&pipe_path) {
                Ok(server) => server,
                Err(error) => {
                    let _ = ready_tx.send(Err(error.to_string()));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            while !stopping.load(Ordering::Acquire) {
                let mut stream = match server.accept() {
                    Ok(stream) => stream,
                    Err(error) => {
                        eprintln!("IPC accept 失败：{error}");
                        continue;
                    }
                };
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                if active_clients.load(Ordering::Acquire) >= 4 {
                    eprintln!("IPC 客户端已达上限，拒绝新连接。");
                    continue;
                }
                active_clients.fetch_add(1, Ordering::AcqRel);
                let snapshot = Arc::clone(&snapshot);
                let active_clients = Arc::clone(&active_clients);
                let command_tx = command_tx.clone();
                thread::spawn(move || {
                    let result = serve_connection_with_handler(
                        &mut stream,
                        || {
                            snapshot
                                .read()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .clone()
                        },
                        |request, _| {
                            let reply_timeout = match request {
                                Request::Shutdown { .. } => Duration::from_secs(30),
                                Request::ApplyNow { .. } => Duration::from_secs(15),
                                _ => Duration::from_secs(5),
                            };
                            let (reply, receiver) = mpsc::sync_channel(1);
                            let command = match request {
                                Request::ApplyNow { .. } => DeviceCommand::ApplyNow { reply },
                                Request::Shutdown { force, .. } => DeviceCommand::Shutdown {
                                    force: *force,
                                    reply,
                                },
                                Request::ValidateDraft { draft, .. } => {
                                    DeviceCommand::ValidateDraft {
                                        draft: draft.clone(),
                                        reply,
                                    }
                                }
                                Request::CommitConfig {
                                    base_revision,
                                    draft,
                                    ..
                                } => DeviceCommand::CommitConfig {
                                    base_revision: *base_revision,
                                    draft: draft.clone(),
                                    reply,
                                },
                                Request::SetSelectionMode { mode, .. } => {
                                    DeviceCommand::SetSelectionMode { mode: *mode, reply }
                                }
                                _ => return None,
                            };
                            match command_tx.try_send(command) {
                                Ok(()) => match receiver.recv_timeout(reply_timeout) {
                                    Ok(Ok(data)) => {
                                        Some(Response::success(request.request_id(), data))
                                    }
                                    Ok(Err(error)) => Some(Response::failure(
                                        request.request_id(),
                                        ErrorBody {
                                            code: error.code,
                                            message: error.message,
                                            retryable: error.retryable,
                                        },
                                    )),
                                    Err(_) => Some(Response::failure(
                                        request.request_id(),
                                        ErrorBody {
                                            code: ErrorCode::Busy,
                                            message: "设备协调者响应超时".to_owned(),
                                            retryable: true,
                                        },
                                    )),
                                },
                                Err(mpsc::TrySendError::Full(_)) => Some(Response::failure(
                                    request.request_id(),
                                    ErrorBody {
                                        code: ErrorCode::Busy,
                                        message: "设备命令队列已满".to_owned(),
                                        retryable: true,
                                    },
                                )),
                                Err(mpsc::TrySendError::Disconnected(_)) => {
                                    Some(Response::failure(
                                        request.request_id(),
                                        ErrorBody {
                                            code: ErrorCode::Internal,
                                            message: "设备协调者已停止".to_owned(),
                                            retryable: false,
                                        },
                                    ))
                                }
                            }
                        },
                    );
                    if let Err(error) = result {
                        eprintln!("IPC 客户端会话失败：{error}");
                    }
                    active_clients.fetch_sub(1, Ordering::AcqRel);
                });
            }
        })
    };
    match ready_rx.recv() {
        Ok(Ok(())) => {
            println!("统一代理已启动：前台监听 + HID 应用 + IPC 快照。");
            local_log::info(format_args!(
                "统一代理已启动：前台监听 + HID 应用 + IPC 快照"
            ));
        }
        Ok(Err(error)) => {
            eprintln!("IPC 管道创建失败：{error}");
            return ExitCode::FAILURE;
        }
        Err(_) => {
            eprintln!("IPC 启动线程意外退出。");
            return ExitCode::FAILURE;
        }
    }

    let tray_thread = spawn_agent_tray(command_tx.clone(), Arc::clone(&stopping));

    let mut active_profile_key: Option<String> = None;
    let mut backoff = RetryBackoff::default();
    let mut switch_guard = EnvironmentSwitchGuard::default();
    let foreground_snapshot = Arc::clone(&snapshot);
    let command_snapshot = Arc::clone(&snapshot);
    let mut last_dpi_poll = Instant::now();
    let recovery_requested = std::rc::Rc::new(std::cell::Cell::new(false));
    let foreground_recovery = std::rc::Rc::clone(&recovery_requested);
    let command_recovery = std::rc::Rc::clone(&recovery_requested);
    let mut device_health = DeviceHealthMonitor::default();
    let result = watcher::run(
        exit_after_seconds.map(Duration::from_secs),
        Arc::clone(&stopping),
        || {
            if foreground_recovery.replace(false) {
                active_profile_key = None;
                switch_guard.pending = None;
                println!("设备健康检查连续失败，当前环境状态已失效，开始重新连接并恢复配置。");
                local_log::error(format_args!(
                    "设备健康检查连续失败；当前环境状态失效，开始重新连接并恢复配置"
                ));
            }
            let current_config = repository.borrow().document().clone();
            let target = match resolve_target(&current_config) {
                Ok(target) => target,
                Err(error) => {
                    let delay = backoff.record_failure();
                    eprintln!(
                        "前台进程识别暂时失败：{error}；{} ms 后重试。",
                        delay.as_millis()
                    );
                    local_log::error(format_args!(
                        "前台进程识别失败：{error}；{} ms 后重试",
                        delay.as_millis()
                    ));
                    return Some(delay);
                }
            };
            let is_new_pending = switch_guard
                .pending
                .as_ref()
                .is_none_or(|(profile_key, _)| profile_key != &target.profile_key);
            match switch_guard.decide(
                &target.profile_key,
                active_profile_key.as_deref(),
                Instant::now(),
            ) {
                SwitchDecision::AlreadyActive => return None,
                SwitchDecision::Wait(delay) => {
                    if is_new_pending {
                        println!(
                            "检测到目标环境 {:?}，稳定 {} ms 后再应用，以避免瞬时切换写入。",
                            target.environment,
                            ENVIRONMENT_STABLE_FOR.as_millis()
                        );
                        local_log::info(format_args!(
                            "检测到目标环境 {:?}；稳定 {} ms 后应用",
                            target.environment,
                            ENVIRONMENT_STABLE_FOR.as_millis()
                        ));
                    }
                    return Some(delay);
                }
                SwitchDecision::Apply => {}
            }
            print_target(
                &target,
                &snapshot_for_target(
                    &target,
                    repository.borrow().revision(),
                    DeviceStatus::Unknown,
                    None,
                ),
            );
            // G102 只公开一个板载配置。环境变化时必须同时同步按键映射与
            // DPI 模式；apply_target_full 会先比较回读内容，完全一致时不写闪存。
            match apply_target_full(&target) {
                Ok(applied) => {
                    active_profile_key = Some(target.profile_key.clone());
                    switch_guard.record_applied(Instant::now());
                    backoff.record_success();
                    let previous = foreground_snapshot
                        .read()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone();
                    let mut current = snapshot_for_target(
                        &target,
                        repository.borrow().revision(),
                        DeviceStatus::Ready,
                        Some(applied.after),
                    );
                    current.dpi_capability = previous.dpi_capability;
                    current.integration_status = previous.integration_status;
                    *foreground_snapshot
                        .write()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = current;
                    None
                }
                Err(error) if error.retryable => {
                    let delay = backoff.record_failure();
                    eprintln!("{}；{} ms 后重试。", error.message, delay.as_millis());
                    local_log::error(format_args!(
                        "{}；{} ms 后重试",
                        error.message,
                        delay.as_millis()
                    ));
                    Some(delay)
                }
                Err(error) => {
                    eprintln!("{}；该错误不会自动重试。", error.message);
                    None
                }
            }
        },
        || {
            local_log::run_periodic_maintenance();
            let mut request_recovery = false;
            while let Ok(command) = command_rx.try_recv() {
                match command {
                    DeviceCommand::ApplyNow { reply } => {
                        let current_config = repository.borrow().document().clone();
                        let result = resolve_target(&current_config)
                            .map_err(|message| CommandFailure {
                                code: ErrorCode::Internal,
                                message,
                                retryable: true,
                            })
                            .and_then(|target| {
                                apply_target_full(&target)
                                    .map_err(|error| CommandFailure {
                                        code: if error.retryable {
                                            ErrorCode::Busy
                                        } else {
                                            ErrorCode::Internal
                                        },
                                        message: error.message,
                                        retryable: error.retryable,
                                    })
                                    .map(|applied| {
                                        let previous = command_snapshot
                                            .read()
                                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                                            .clone();
                                        let mut current = snapshot_for_target(
                                            &target,
                                            repository.borrow().revision(),
                                            DeviceStatus::Ready,
                                            Some(applied.after),
                                        );
                                        current.dpi_capability = previous.dpi_capability;
                                        current.integration_status = previous.integration_status;
                                        *command_snapshot
                                            .write()
                                            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                                            current.clone();
                                        serde_json::to_value(current)
                                            .expect("AgentSnapshot 必须可序列化")
                                    })
                            });
                        let _ = reply.send(result);
                    }
                    DeviceCommand::Shutdown { force, reply } => {
                        if force {
                            local_log::audit(format_args!("用户确认仍然退出；跳过鼠标安全还原"));
                            create_shutdown_marker();
                            stopping.store(true, Ordering::Release);
                            let _ = reply.send(Ok(serde_json::json!({
                                "shutdown": true,
                                "restored": false,
                                "forced": true
                            })));
                            break;
                        }

                        let shutdown_profile =
                            repository.borrow().document().shutdown_profile.clone();
                        local_log::audit(format_args!(
                            "开始安全退出：目标 DPI={}，回报率={} Hz，应用用户退出配置",
                            shutdown_profile.dpi, shutdown_profile.report_rate_hz
                        ));
                        match apply_safe_shutdown(&shutdown_profile) {
                            Ok(applied) => {
                                local_log::audit(format_args!(
                                    "安全退出配置写入并回读成功：DPI={}，回报率={} Hz",
                                    applied.after, shutdown_profile.report_rate_hz
                                ));
                                create_shutdown_marker();
                                stopping.store(true, Ordering::Release);
                                let _ = reply.send(Ok(serde_json::json!({
                                    "shutdown": true,
                                    "restored": true,
                                    "current_dpi": applied.after
                                })));
                                break;
                            }
                            Err(error) if error.device_disconnected => {
                                local_log::audit(format_args!(
                                    "安全退出时设备未连接，无法恢复；代理将在有界尝试后退出：{}",
                                    error.message
                                ));
                                create_shutdown_marker();
                                stopping.store(true, Ordering::Release);
                                let _ = reply.send(Ok(serde_json::json!({
                                    "shutdown": true,
                                    "restored": false,
                                    "device_connected": false
                                })));
                                break;
                            }
                            Err(error) => {
                                local_log::audit(format_args!(
                                    "安全退出还原失败，等待用户重试或确认仍然退出：{}",
                                    error.message
                                ));
                                let _ = reply.send(Err(CommandFailure {
                                    code: ErrorCode::Internal,
                                    message: error.message,
                                    retryable: true,
                                }));
                            }
                        }
                    }
                    DeviceCommand::ValidateDraft { draft, reply } => {
                        let result = repository
                            .borrow()
                            .validate_draft(draft)
                            .map(|_| {
                                serde_json::json!({
                                    "valid": true,
                                    "revision": repository.borrow().revision()
                                })
                            })
                            .map_err(config_command_failure);
                        let _ = reply.send(result);
                    }
                    DeviceCommand::CommitConfig {
                        base_revision,
                        draft,
                        reply,
                    } => {
                        let committed = {
                            repository
                                .borrow_mut()
                                .commit(base_revision, draft)
                                .map_err(config_command_failure)
                        };
                        let result = committed.map(|revision| {
                            let current_config = repository.borrow().document().clone();
                            local_log::set_enabled(current_config.agent.developer_logging);
                            let previous = command_snapshot
                                .read()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .clone();
                            let desired_dpi = match previous.active_environment {
                                IpcEnvironment::Office => current_config.profiles.office.dpi,
                                IpcEnvironment::Cs2 => current_config.profiles.cs2.dpi,
                                IpcEnvironment::Custom => previous.desired_dpi,
                            };
                            let current = AgentSnapshot {
                                device_status: previous.device_status,
                                active_environment: previous.active_environment,
                                config_revision: revision,
                                current_dpi: previous.current_dpi,
                                desired_dpi,
                                active_profile_name: previous.active_profile_name,
                                dpi_capability: previous.dpi_capability,
                                integration_status: sync_start_with_windows(
                                    current_config.agent.start_with_windows,
                                )
                                .map_or(IntegrationStatus::Failed, |()| IntegrationStatus::Synced),
                            };
                            *command_snapshot
                                .write()
                                .unwrap_or_else(|poisoned| poisoned.into_inner()) = current.clone();
                            serde_json::to_value(current).expect("AgentSnapshot 必须可序列化")
                        });
                        let _ = reply.send(result);
                    }
                    DeviceCommand::SetSelectionMode { mode, reply } => {
                        let mut draft = repository.borrow().document().clone();
                        draft.selection.mode = match mode {
                            pulsehub_ipc::SelectionMode::Auto => SelectionMode::Auto,
                            pulsehub_ipc::SelectionMode::Office => SelectionMode::Office,
                            pulsehub_ipc::SelectionMode::Cs2 => SelectionMode::Cs2,
                        };
                        let revision = repository.borrow().revision();
                        let draft = serde_json::to_value(draft)
                            .map_err(|error| CommandFailure {
                                code: ErrorCode::Internal,
                                message: error.to_string(),
                                retryable: false,
                            })
                            .and_then(|draft| {
                                repository
                                    .borrow_mut()
                                    .commit(revision, draft)
                                    .map_err(config_command_failure)
                            });
                        let result = draft.map(|revision| {
                            let config = repository.borrow().document().clone();
                            let previous = command_snapshot
                                .read()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .clone();
                            let environment = resolve_target(&config)
                                .map(|target| target.environment)
                                .unwrap_or_else(|_| selected_environment(&config, None));
                            let desired_dpi = match environment {
                                Environment::Office => config.profiles.office.dpi,
                                Environment::Cs2 => config.profiles.cs2.dpi,
                                Environment::Custom => previous.desired_dpi,
                            };
                            let current = AgentSnapshot {
                                device_status: previous.device_status,
                                active_environment: match environment {
                                    Environment::Office => IpcEnvironment::Office,
                                    Environment::Cs2 => IpcEnvironment::Cs2,
                                    Environment::Custom => IpcEnvironment::Custom,
                                },
                                config_revision: revision,
                                current_dpi: previous.current_dpi,
                                desired_dpi,
                                active_profile_name: previous.active_profile_name,
                                dpi_capability: previous.dpi_capability,
                                integration_status: previous.integration_status,
                            };
                            *command_snapshot
                                .write()
                                .unwrap_or_else(|poisoned| poisoned.into_inner()) = current.clone();
                            serde_json::to_value(current).expect("AgentSnapshot 必须可序列化")
                        });
                        let _ = reply.send(result);
                    }
                }
            }
            if stopping.load(Ordering::Acquire) {
                return false;
            }
            if last_dpi_poll.elapsed() >= Duration::from_millis(500) {
                last_dpi_poll = Instant::now();
                match read_first_g102_dpi(false) {
                    Ok(current_dpi) => {
                        device_health.record_success();
                        let mut current = command_snapshot
                            .write()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        current.current_dpi = Some(current_dpi);
                        current.device_status = DeviceStatus::Ready;
                    }
                    Err(error) if device_health.record_failure() => {
                        eprintln!("DPI 健康检查连续 {DEVICE_FAILURE_THRESHOLD} 次失败：{error}");
                        local_log::error(format_args!(
                            "DPI 健康检查连续 {DEVICE_FAILURE_THRESHOLD} 次失败：{error}"
                        ));
                        let mut current = command_snapshot
                            .write()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        current.current_dpi = None;
                        current.device_status = DeviceStatus::Degraded;
                        command_recovery.set(true);
                        request_recovery = true;
                    }
                    Err(_) => {}
                }
            }
            request_recovery
        },
    );
    stopping.store(true, Ordering::Release);
    let _ = connect(&pipe_path);
    let _ = ipc_thread.join();
    let _ = tray_thread.join();
    match result {
        Ok(()) => {
            println!("统一代理已停止，前台 hook 与 IPC listener 均已释放。");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("统一代理运行失败：{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(windows)]
fn spawn_agent_tray(
    command_tx: std::sync::mpsc::SyncSender<DeviceCommand>,
    stopping: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        use slint::{Timer, TimerMode};
        use std::os::windows::process::CommandExt;
        use std::sync::atomic::Ordering;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let Ok(tray) = pulsehub_ui::AppTray::new() else {
            return;
        };
        update_agent_tray_language(&tray);

        let gui_running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let open_gui_running = std::sync::Arc::clone(&gui_running);
        tray.on_open_requested(move || {
            if open_gui_running.swap(true, Ordering::AcqRel) {
                return;
            }
            if let Ok(agent) = std::env::current_exe()
                && let Some(directory) = agent.parent()
            {
                let child = std::process::Command::new(directory.join("pulsehub-config.exe"))
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn();
                if let Ok(mut child) = child {
                    let child_gui_running = std::sync::Arc::clone(&open_gui_running);
                    std::thread::spawn(move || {
                        let _ = child.wait();
                        child_gui_running.store(false, Ordering::Release);
                    });
                    return;
                }
            }
            open_gui_running.store(false, Ordering::Release);
        });

        let shutdown_signal = std::sync::Arc::new(std::sync::Mutex::new(None));
        let quit_shutdown_signal = std::sync::Arc::clone(&shutdown_signal);
        let quit_tx = command_tx.clone();
        tray.on_quit_requested(move || {
            let (reply, receiver) = std::sync::mpsc::sync_channel(1);
            if quit_tx
                .try_send(DeviceCommand::Shutdown {
                    force: false,
                    reply,
                })
                .is_ok()
                && receiver
                    .recv_timeout(Duration::from_secs(30))
                    .is_ok_and(|result| result.is_ok())
            {
                if let Ok(signal) =
                    single_instance::SingleInstance::new("Local\\PulseHub.ShuttingDown.v1")
                    && let Ok(mut slot) = quit_shutdown_signal.lock()
                {
                    *slot = Some(signal);
                }
                slint::quit_event_loop().ok();
            }
        });

        if tray.show().is_err() {
            return;
        }
        let language_tray = tray.as_weak();
        let monitor = Timer::default();
        monitor.start(TimerMode::Repeated, Duration::from_millis(500), move || {
            if stopping.load(Ordering::Acquire) {
                if let Some(tray) = language_tray.upgrade() {
                    let _ = tray.hide();
                }
                slint::quit_event_loop().ok();
                return;
            }
            if let Some(tray) = language_tray.upgrade() {
                update_agent_tray_language(&tray);
            }
        });
        let trim = Timer::default();
        trim.start(TimerMode::SingleShot, Duration::from_secs(2), || {
            pulsehub_ui::trim_current_process_working_set();
        });
        let _ = slint::run_event_loop();
        // 让已打开的 GUI 至少有数次轮询机会观察到退出信号。
        std::thread::sleep(Duration::from_secs(3));
        drop(shutdown_signal);
    })
}

#[cfg(windows)]
fn update_agent_tray_language(tray: &pulsehub_ui::AppTray) {
    if let Ok(path) = pulsehub_config_store::default_config_path()
        && let Ok(config) = pulsehub_config_store::load_or_create_default(&path)
    {
        tray.set_english(config.agent.language == pulsehub_config_store::UiLanguage::En);
    }
}

#[cfg(not(windows))]
fn run_agent(_: &Path, _: &ConfigDocument, _: Option<u64>) -> ExitCode {
    eprintln!("统一代理模式仅支持 Windows。");
    ExitCode::FAILURE
}

#[cfg(windows)]
fn config_command_failure(error: ConfigError) -> CommandFailure {
    let code = match error {
        ConfigError::RevisionConflict { .. } => pulsehub_ipc::ErrorCode::Conflict,
        ConfigError::Validation(_)
        | ConfigError::UnsupportedSchema { .. }
        | ConfigError::Parse { .. } => pulsehub_ipc::ErrorCode::InvalidRequest,
        _ => pulsehub_ipc::ErrorCode::Internal,
    };
    CommandFailure {
        code,
        message: error.to_string(),
        retryable: false,
    }
}

#[cfg(windows)]
fn serve_ipc_once(config: &ConfigDocument, allow_apply: bool) -> ExitCode {
    use pulsehub_ipc::windows::{Server, default_pipe_path};

    let pipe_path = match default_pipe_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("IPC 管道名构造失败：{error}");
            return ExitCode::FAILURE;
        }
    };

    let snapshot = match live_snapshot(config) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("IPC 快照构造失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    let server = match Server::bind(&pipe_path) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("IPC 管道创建失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    println!("IPC 单连接验证服务已启动：{pipe_path}");
    println!("提供包含 HID 只读状态的脱敏快照；等待 pulsehub-config 客户端……");
    let mut stream = match server.accept() {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("IPC 客户端连接失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    if allow_apply {
        return serve_ipc_apply_session(config, &mut stream, snapshot);
    }
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

#[cfg(windows)]
fn serve_ipc_apply_session(
    config: &ConfigDocument,
    stream: &mut pulsehub_ipc::windows::ByteStream,
    mut snapshot: AgentSnapshot,
) -> ExitCode {
    use pulsehub_ipc::{
        ErrorBody, ErrorCode, Request, Response, Session, dispatch_accepted_request, read_request,
        write_frame,
    };

    let mut session = Session::default();
    loop {
        let request = match read_request(stream) {
            Ok(request) => request,
            Err(pulsehub_ipc::FrameError::Io(std::io::ErrorKind::UnexpectedEof)) => {
                return ExitCode::SUCCESS;
            }
            Err(error) => {
                eprintln!("IPC 请求读取失败：{error}");
                return ExitCode::FAILURE;
            }
        };
        let response = if let Err(error) = session.accept(&request) {
            Response::failure(
                request.request_id(),
                ErrorBody {
                    code: ErrorCode::InvalidRequest,
                    message: error.to_string(),
                    retryable: false,
                },
            )
        } else if matches!(request, Request::ApplyNow { .. }) {
            let target = match resolve_target(config) {
                Ok(target) => target,
                Err(error) => {
                    let response = Response::failure(
                        request.request_id(),
                        ErrorBody {
                            code: ErrorCode::Internal,
                            message: error,
                            retryable: true,
                        },
                    );
                    if write_frame(stream, &response).is_err() {
                        return ExitCode::FAILURE;
                    }
                    continue;
                }
            };
            match apply_target_dpi(&target) {
                Ok(applied) => {
                    let capability = snapshot.dpi_capability.clone();
                    let integration_status = snapshot.integration_status;
                    snapshot = snapshot_for_target(
                        &target,
                        snapshot.config_revision,
                        DeviceStatus::Ready,
                        Some(applied.after),
                    );
                    snapshot.dpi_capability = capability;
                    snapshot.integration_status = integration_status;
                    Response::success(
                        request.request_id(),
                        serde_json::to_value(&snapshot).expect("AgentSnapshot 必须可序列化"),
                    )
                }
                Err(error) => Response::failure(
                    request.request_id(),
                    ErrorBody {
                        code: if error.retryable {
                            ErrorCode::Busy
                        } else {
                            ErrorCode::Internal
                        },
                        message: error.message,
                        retryable: error.retryable,
                    },
                ),
            }
        } else {
            dispatch_accepted_request(&request, &snapshot)
        };
        if let Err(error) = write_frame(stream, &response) {
            eprintln!("IPC 响应写入失败：{error}");
            return ExitCode::FAILURE;
        }
    }
}

#[cfg(windows)]
fn serve_ipc(config: &ConfigDocument, exit_after_seconds: Option<u64>) -> ExitCode {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, RwLock};
    use std::thread;
    use std::time::Instant;

    use pulsehub_ipc::windows::{Server, connect, default_pipe_path, serve_connection_with};

    const MAX_CLIENTS: usize = 4;
    let pipe_path = match default_pipe_path() {
        Ok(path) => Arc::new(path),
        Err(error) => {
            eprintln!("IPC 管道名构造失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    let snapshot = match live_snapshot(config) {
        Ok(snapshot) => Arc::new(RwLock::new(snapshot)),
        Err(error) => {
            eprintln!("IPC 快照构造失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    let server = match Server::bind(pipe_path.as_str()) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("IPC 管道创建失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    let stopping = Arc::new(AtomicBool::new(false));
    let install_stop_handler = |stopping: Arc<AtomicBool>, pipe_path: Arc<String>| {
        ctrlc::set_handler(move || {
            stopping.store(true, Ordering::Release);
            let _ = connect(pipe_path.as_str());
        })
    };
    if let Err(error) = install_stop_handler(Arc::clone(&stopping), Arc::clone(&pipe_path)) {
        eprintln!("IPC Ctrl+C 处理器安装失败：{error}");
        return ExitCode::FAILURE;
    }
    if let Some(seconds) = exit_after_seconds {
        let stopping = Arc::clone(&stopping);
        let pipe_path = Arc::clone(&pipe_path);
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(seconds));
            stopping.store(true, Ordering::Release);
            let _ = connect(pipe_path.as_str());
        });
    }

    let active_clients = Arc::new(AtomicUsize::new(0));
    println!("IPC 常驻服务已启动：{pipe_path}；最多 {MAX_CLIENTS} 个并发客户端。");
    println!("按 Ctrl+C 退出。服务只读取 HID 状态，不写入设备。");
    while !stopping.load(Ordering::Acquire) {
        let mut stream = match server.accept() {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("IPC accept 失败：{error}");
                continue;
            }
        };
        if stopping.load(Ordering::Acquire) {
            break;
        }
        if active_clients.load(Ordering::Acquire) >= MAX_CLIENTS {
            eprintln!("IPC 客户端已达上限，拒绝新连接。");
            continue;
        }
        active_clients.fetch_add(1, Ordering::AcqRel);
        let active_clients_for_thread = Arc::clone(&active_clients);
        let snapshot_for_thread = Arc::clone(&snapshot);
        thread::spawn(move || {
            let result = serve_connection_with(&mut stream, || {
                snapshot_for_thread
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
            });
            if let Err(error) = result {
                eprintln!("IPC 客户端会话失败：{error}");
            }
            active_clients_for_thread.fetch_sub(1, Ordering::AcqRel);
        });
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    while active_clients.load(Ordering::Acquire) != 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let remaining = active_clients.load(Ordering::Acquire);
    if remaining == 0 {
        println!("IPC 常驻服务已停止，所有客户端会话均已释放。");
    } else {
        eprintln!("IPC 常驻服务停止时仍有 {remaining} 个客户端会话，由进程退出统一回收。");
    }
    ExitCode::SUCCESS
}

#[cfg(not(windows))]
fn serve_ipc(_: &ConfigDocument, _: Option<u64>) -> ExitCode {
    eprintln!("IPC Named Pipe 常驻模式仅支持 Windows。");
    ExitCode::FAILURE
}

fn live_snapshot(config: &ConfigDocument) -> Result<AgentSnapshot, String> {
    let target = resolve_target(config)?;
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
    let mut snapshot = snapshot_for_target(&target, 0, device_status, current_dpi);
    snapshot.dpi_capability = probe.as_ref().ok().and_then(|result| {
        result.dpi_sensors.first().map(|sensor| DpiCapability {
            minimum: sensor.minimum,
            maximum: sensor.maximum,
            step: sensor.step,
            selectable_values: suggested_dpi_values(
                sensor.minimum,
                sensor.maximum,
                sensor.step,
                &sensor.discrete_values,
            ),
        })
    });
    snapshot.integration_status = startup_integration_status(config.agent.start_with_windows);
    Ok(snapshot)
}

#[cfg(not(windows))]
fn serve_ipc_once(_: &ConfigDocument, _: bool) -> ExitCode {
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
    match apply_target_dpi(&target) {
        Ok(_) => ExitCode::SUCCESS,
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
            Environment::Custom => IpcEnvironment::Custom,
        },
        config_revision,
        current_dpi,
        desired_dpi: target.dpi,
        active_profile_name: Some(target.profile_name.clone()),
        dpi_capability: None,
        integration_status: IntegrationStatus::Unknown,
    }
}

fn suggested_dpi_values(
    minimum: u16,
    maximum: u16,
    step: Option<u16>,
    discrete_values: &[u16],
) -> Vec<u16> {
    [100_u16, 400, 800, 1600, 1800, 3200]
        .into_iter()
        .filter(|value| {
            if !discrete_values.is_empty() && step.is_none() {
                return discrete_values.contains(value);
            }
            (minimum..=maximum).contains(value)
                && step.is_none_or(|step| (*value - minimum).is_multiple_of(step))
        })
        .collect()
}

#[cfg(windows)]
fn startup_integration_status(enabled: bool) -> IntegrationStatus {
    let exists = std::process::Command::new("reg.exe")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "PulseHub",
        ])
        .output()
        .is_ok_and(|output| output.status.success());
    if exists == enabled {
        IntegrationStatus::Synced
    } else {
        IntegrationStatus::Failed
    }
}

#[cfg(not(windows))]
fn startup_integration_status(_: bool) -> IntegrationStatus {
    IntegrationStatus::Unknown
}

#[cfg(windows)]
fn startup_command(executable: &Path) -> String {
    format!(
        "\"{}\" --run-agent --confirm-device-write",
        executable.display()
    )
}

#[cfg(windows)]
fn sync_start_with_windows(enabled: bool) -> Result<(), String> {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let approval_key =
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    if !enabled {
        let status = std::process::Command::new("reg.exe")
            .args(["delete", key, "/v", "PulseHub", "/f"])
            .status()
            .map_err(|error| format!("无法启动 reg.exe：{error}"))?;
        if status.success() || startup_integration_status(false) == IntegrationStatus::Synced {
            let _ = std::process::Command::new("reg.exe")
                .args(["delete", approval_key, "/v", "PulseHub", "/f"])
                .status();
            return Ok(());
        }
        return Err(format!("删除登录启动项失败：{status}"));
    }
    let executable =
        std::env::current_exe().map_err(|error| format!("无法解析代理路径：{error}"))?;
    let command = startup_command(&executable);
    let status = std::process::Command::new("reg.exe")
        .args([
            "add", key, "/v", "PulseHub", "/t", "REG_SZ", "/d", &command, "/f",
        ])
        .status()
        .map_err(|error| format!("无法启动 reg.exe：{error}"))?;
    if !status.success() {
        return Err(format!("写入登录启动项失败：{status}"));
    }
    let approval_status = std::process::Command::new("reg.exe")
        .args(["delete", approval_key, "/v", "PulseHub", "/f"])
        .status()
        .map_err(|error| format!("无法启动 reg.exe：{error}"))?;
    if approval_status.success()
        || !std::process::Command::new("reg.exe")
            .args(["query", approval_key, "/v", "PulseHub"])
            .output()
            .is_ok_and(|output| output.status.success())
    {
        Ok(())
    } else {
        Err(format!(
            "无法解除登录启动项的 Windows 禁用状态：{approval_status}"
        ))
    }
}

fn run_watcher(config: &ConfigDocument, exit_after_seconds: Option<u64>) -> ExitCode {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    let mut tracker = EnvironmentTracker::default();
    let mut backoff = RetryBackoff::default();
    println!("自动切换监听已启动；按 Ctrl+C 退出。仅切换运行态 DPI，不写入板载闪存。");
    let result = watcher::run(
        exit_after_seconds.map(Duration::from_secs),
        Arc::new(AtomicBool::new(false)),
        || {
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
            match apply_target_dpi(&target) {
                Ok(_) => {
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
        },
        || false,
    );
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
    let custom = custom_application_for_process(config, &foreground.executable_name);
    let (environment, profile_key, profile_name, profile) = if let Some(application) = custom {
        (
            Environment::Custom,
            format!("application:{}", application.id),
            application.name.clone(),
            &application.profile,
        )
    } else {
        let environment = selected_environment(config, Some(&foreground.executable_name));
        match environment {
            Environment::Office => (
                environment,
                "office".to_owned(),
                "办公环境".to_owned(),
                &config.profiles.office,
            ),
            Environment::Cs2 => (
                environment,
                "cs2".to_owned(),
                "CS 环境".to_owned(),
                &config.profiles.cs2,
            ),
            Environment::Custom => unreachable!("自定义环境由应用列表解析"),
        }
    };
    let dpi = profile.dpi;
    let button_actions = button_actions_for_profile(profile)?;
    let dpi_levels = profile
        .dpi_levels
        .clone()
        .try_into()
        .map_err(|_| "DPI 档位必须正好包含四项".to_owned())?;
    Ok(EnvironmentTarget {
        executable_name: foreground.executable_name,
        process_id: foreground.process_id,
        environment,
        profile_key,
        profile_name,
        dpi,
        report_rate_hz: profile.report_rate_hz,
        button_actions,
        dpi_levels,
    })
}

fn custom_application_for_process<'a>(
    config: &'a ConfigDocument,
    executable_name: &str,
) -> Option<&'a pulsehub_config_store::ApplicationProfileConfig> {
    match config.selection.mode {
        SelectionMode::Auto => config.applications.iter().find(|application| {
            application
                .process_name
                .eq_ignore_ascii_case(executable_name)
        }),
        SelectionMode::Application => {
            config
                .selection
                .fixed_application_id
                .as_deref()
                .and_then(|id| {
                    config
                        .applications
                        .iter()
                        .find(|application| application.id == id)
                })
        }
        SelectionMode::Office | SelectionMode::Cs2 => None,
    }
}

fn safe_shutdown_target(profile: &ProfileConfig) -> Result<EnvironmentTarget, String> {
    let dpi_levels = profile
        .dpi_levels
        .clone()
        .try_into()
        .map_err(|_| "退出配置的 DPI 档位必须正好包含四项".to_owned())?;
    let button_actions = button_actions_for_profile(profile)?;
    Ok(EnvironmentTarget {
        executable_name: "PulseHub shutdown".to_owned(),
        process_id: 0,
        environment: Environment::Office,
        profile_key: "shutdown".to_owned(),
        profile_name: "安全退出".to_owned(),
        dpi: profile.dpi,
        report_rate_hz: profile.report_rate_hz,
        button_actions,
        dpi_levels,
    })
}

fn apply_safe_shutdown(profile: &ProfileConfig) -> Result<DpiWriteResult, ApplyFailure> {
    let target = safe_shutdown_target(profile).map_err(|message| ApplyFailure {
        message,
        retryable: false,
        device_disconnected: false,
    })?;
    let deadline = Instant::now() + SHUTDOWN_DEVICE_WAIT;
    let mut attempt = 0_u8;
    loop {
        attempt += 1;
        match apply_target_full(&target) {
            Err(error)
                if (error.device_disconnected || error.retryable)
                    && attempt < SHUTDOWN_MAX_ATTEMPTS
                    && Instant::now() < deadline =>
            {
                local_log::audit(format_args!(
                    "安全退出第 {attempt} 次还原暂时失败，将进行有限重试：{}",
                    error.message
                ));
                std::thread::sleep(
                    SHUTDOWN_DEVICE_RETRY.min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            result => return result,
        }
    }
}

fn button_actions_for_profile(profile: &ProfileConfig) -> Result<[OnboardButtonAction; 6], String> {
    const CONTROLS: [&str; 6] = [
        "g102:left",
        "g102:right",
        "g102:middle",
        "g102:side_back",
        "g102:side_forward",
        "g102:dpi",
    ];
    let mut actions = std::array::from_fn(|_| OnboardButtonAction::Disabled);
    let mut assigned = [false; 6];
    for mapping in &profile.button_mappings {
        let index = CONTROLS
            .iter()
            .position(|control| *control == mapping.physical_control)
            .ok_or_else(|| format!("不支持的 G102 物理按键：{}", mapping.physical_control))?;
        actions[index] = onboard_action_from_config(&mapping.action)?;
        assigned[index] = true;
    }
    if let Some(index) = assigned.iter().position(|assigned| !assigned) {
        return Err(format!("配置缺少 G102 按键映射：{}", CONTROLS[index]));
    }
    Ok(actions)
}

fn onboard_action_from_config(action: &ButtonActionConfig) -> Result<OnboardButtonAction, String> {
    match action {
        ButtonActionConfig::LogicalControl { value } => match value.as_str() {
            "mouse:left" => Ok(mouse_button_action(1)),
            "mouse:right" => Ok(mouse_button_action(2)),
            "mouse:middle" => Ok(mouse_button_action(3)),
            "mouse:back" => Ok(mouse_button_action(4)),
            "mouse:forward" => Ok(mouse_button_action(5)),
            "mouse:dpi_cycle" => Ok(OnboardButtonAction::Special {
                code: 0x05,
                profile: 0,
            }),
            value => Err(format!("不支持的板载逻辑动作：{value}")),
        },
        ButtonActionConfig::OnboardKeyboard {
            usage_page,
            usage,
            modifiers,
        } => {
            if *usage_page != 0x07 || *usage > u16::from(u8::MAX) {
                return Err(format!(
                    "G102 板载键盘动作不支持 usage_page=0x{usage_page:04x}, usage=0x{usage:04x}"
                ));
            }
            Ok(OnboardButtonAction::Keyboard {
                modifiers: *modifiers,
                key: *usage as u8,
            })
        }
        ButtonActionConfig::OnboardConsumer { usage } => {
            Ok(OnboardButtonAction::ConsumerControl { usage: *usage })
        }
        ButtonActionConfig::Disabled => Ok(OnboardButtonAction::Disabled),
    }
}

fn mouse_button_action(button: u8) -> OnboardButtonAction {
    OnboardButtonAction::Mouse {
        button: Some(button),
        mask: 1_u16 << (button - 1),
    }
}

fn print_target(target: &EnvironmentTarget, snapshot: &AgentSnapshot) {
    println!(
        "前台进程：{} (PID {})；目标环境={}，目标 DPI={}",
        target.executable_name, target.process_id, target.profile_name, snapshot.desired_dpi
    );
    local_log::info(format_args!(
        "环境切换目标：进程={} PID={} 环境={} 配置键={} DPI={}",
        target.executable_name,
        target.process_id,
        target.profile_name,
        target.profile_key,
        snapshot.desired_dpi
    ));
}

fn apply_target_dpi(target: &EnvironmentTarget) -> Result<DpiWriteResult, ApplyFailure> {
    match set_first_g102_dpi(target.dpi, false) {
        Ok(result) if result.changed => {
            println!(
                "运行态 DPI 已从 {} 设置为 {}，回读通过。",
                result.before, result.after
            );
            local_log::info(format_args!(
                "运行态 DPI 写入并回读成功：{} -> {}",
                result.before, result.after
            ));
            Ok(result)
        }
        Ok(result) => {
            println!("运行态 DPI 已是 {}，跳过重复写入。", result.after);
            local_log::info(format_args!(
                "运行态 DPI 已是 {}；跳过重复写入",
                result.after
            ));
            Ok(result)
        }
        Err(error) => {
            let retryable = !matches!(
                error,
                HidppError::PlatformUnsupported | HidppError::InvalidDpi { .. }
            );
            let message = format!("运行态 DPI 应用失败：{error}");
            local_log::error(format_args!("{message}"));
            let device_disconnected = matches!(error, HidppError::InterfaceNotFound);
            Err(ApplyFailure {
                message,
                retryable,
                device_disconnected,
            })
        }
    }
}

fn apply_target_full(target: &EnvironmentTarget) -> Result<DpiWriteResult, ApplyFailure> {
    // 先校验目标 DPI，避免在无效值下进入板载写入事务。
    apply_target_dpi(target)?;
    let dpi_levels = matches!(
        target.button_actions[5],
        OnboardButtonAction::Special { code: 0x05, .. }
    )
    .then_some(&target.dpi_levels);
    let profile = apply_first_g102_profile(
        &target.button_actions,
        target.dpi,
        dpi_levels,
        target.report_rate_hz,
        false,
    )
    .map_err(|error| hidpp_apply_failure("板载配置写入失败", error))?;
    if profile.changed_buttons.is_empty() && !profile.dpi_changed && !profile.report_rate_changed {
        println!("板载 DPI 与按键映射已和当前环境一致，跳过闪存写入。");
        local_log::info(format_args!("板载配置与当前环境一致；跳过闪存写入"));
    } else {
        println!(
            "板载配置已写入并回读通过：DPI 变更={}，回报率变更={}，按键槽位 {:?}。",
            profile.dpi_changed, profile.report_rate_changed, profile.changed_buttons
        );
        local_log::info(format_args!(
            "板载配置写入并回读成功：DPI变更={} 回报率变更={} 按键槽位={:?}",
            profile.dpi_changed, profile.report_rate_changed, profile.changed_buttons
        ));
    }
    let mode = activate_first_g102_onboard_mode(false)
        .map_err(|error| hidpp_apply_failure("板载模式启用失败", error))?;
    if mode.before != mode.after {
        println!("设备已切换到板载模式，按键映射现已生效。");
        local_log::info(format_args!(
            "设备模式切换：{:?} -> {:?}",
            mode.before, mode.after
        ));
    }
    let lighting = ensure_first_g102_lighting_off(false)
        .map_err(|error| hidpp_apply_failure("固定关闭灯光失败", error))?;
    local_log::info(format_args!(
        "灯光固定关闭并回读通过：clusters={} 已关闭={}",
        lighting.cluster_count, lighting.already_off
    ));
    // G102 切入板载模式时固件总是先激活 slot 0，而不是 default_dpi_index。
    // 在 Onboard 模式下再次选择目标 DPI，并以这次回读作为 IPC 快照的真实值。
    // 2026-07-20 实机确认该写入不会把 mode 切回 Host。
    apply_target_dpi(target)
}

fn hidpp_apply_failure(context: &str, error: HidppError) -> ApplyFailure {
    let device_disconnected = matches!(error, HidppError::InterfaceNotFound);
    let retryable = !matches!(
        error,
        HidppError::PlatformUnsupported
            | HidppError::InvalidDpi { .. }
            | HidppError::InvalidResponse(_)
    );
    let message = format!("{context}：{error}");
    local_log::error(format_args!("{message}"));
    ApplyFailure {
        message,
        retryable,
        device_disconnected,
    }
}

fn selected_environment(config: &ConfigDocument, executable_name: Option<&str>) -> Environment {
    let policy = match config.selection.mode {
        SelectionMode::Auto => SelectionPolicy::Auto,
        SelectionMode::Office => SelectionPolicy::Fixed(Environment::Office),
        SelectionMode::Cs2 => SelectionPolicy::Fixed(Environment::Cs2),
        SelectionMode::Application => SelectionPolicy::Fixed(Environment::Office),
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
            "--serve-ipc-apply-once" => parsed.serve_ipc_apply_once = true,
            "--serve-ipc" => parsed.serve_ipc = true,
            "--run-agent" => parsed.run_agent = true,
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
        + u8::from(parsed.serve_ipc_once)
        + u8::from(parsed.serve_ipc_apply_once)
        + u8::from(parsed.serve_ipc)
        + u8::from(parsed.run_agent);
    if modes > 1 {
        return Err("前台检查、单次应用和自动监听模式不能同时使用".to_owned());
    }
    let writing = parsed.apply_current_environment
        || parsed.watch_foreground
        || parsed.run_agent
        || parsed.serve_ipc_apply_once;
    if writing != parsed.confirm_device_write {
        return Err("设备应用或自动监听必须与 --confirm-device-write 同时提供".to_owned());
    }
    if parsed.exit_after_seconds.is_some()
        && !(parsed.watch_foreground || parsed.serve_ipc || parsed.run_agent)
    {
        return Err(
            "--exit-after-seconds 只能与 --watch-foreground、--serve-ipc 或 --run-agent 一起使用"
                .to_owned(),
        );
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
    println!("      pulsehub-agent --serve-ipc-apply-once --confirm-device-write");
    println!("      pulsehub-agent --serve-ipc [--exit-after-seconds <1-3600>]");
    println!(
        "      pulsehub-agent --run-agent --confirm-device-write [--exit-after-seconds <1-3600>]"
    );
    println!("  --inspect-foreground  只读显示前台进程、目标环境和 DPI");
    println!("  --apply-current-environment  按当前前台进程应用一次运行态 DPI");
    println!("  --watch-foreground  监听 Windows 前台事件并自动切换运行态 DPI");
    println!("  --serve-ipc-once  只读服务一个 IPC 客户端，断开后退出");
    println!("  --serve-ipc-apply-once  允许单个已协商客户端请求 apply_now");
    println!("  --serve-ipc  启动最多 4 个并发客户端的只读 IPC 常驻服务");
    println!("  --run-agent  同时运行前台监听、DPI 自动应用和 IPC 快照服务");
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
    fn converts_profile_mappings_to_verified_g102_slots() {
        let mut profile = ConfigDocument::default().profiles.office;
        profile.button_mappings[2].action = ButtonActionConfig::OnboardKeyboard {
            usage_page: 0x07,
            usage: 0x2a,
            modifiers: 0,
        };
        let actions = button_actions_for_profile(&profile).unwrap();

        assert_eq!(actions[0], mouse_button_action(1));
        assert_eq!(
            actions[2],
            OnboardButtonAction::Keyboard {
                modifiers: 0,
                key: 0x2a
            }
        );
        assert_eq!(actions[3], mouse_button_action(4));
        assert_eq!(
            actions[5],
            OnboardButtonAction::Special {
                code: 0x05,
                profile: 0
            }
        );
    }

    #[test]
    fn safe_shutdown_target_restores_native_controls() {
        let config = ConfigDocument::default();
        let target = safe_shutdown_target(&config.shutdown_profile).unwrap();

        assert_eq!(target.dpi, 1600);
        assert_eq!(target.report_rate_hz, 1000);
        assert_eq!(target.dpi_levels, [800, 1600, 2400, 3200]);
        assert_eq!(target.button_actions[0], mouse_button_action(1));
        assert_eq!(target.button_actions[1], mouse_button_action(2));
        assert_eq!(target.button_actions[2], mouse_button_action(3));
        assert_eq!(target.button_actions[3], mouse_button_action(4));
        assert_eq!(target.button_actions[4], mouse_button_action(5));
        assert_eq!(
            target.button_actions[5],
            OnboardButtonAction::Special {
                code: 0x05,
                profile: 0
            }
        );
    }

    #[test]
    fn safe_shutdown_target_uses_user_profile() {
        let mut profile = ConfigDocument::default().shutdown_profile;
        profile.dpi = 800;
        profile.report_rate_hz = 250;
        let target = safe_shutdown_target(&profile).unwrap();
        assert_eq!(target.dpi, 800);
        assert_eq!(target.report_rate_hz, 250);
    }

    #[test]
    fn rejects_incomplete_profile_before_device_io() {
        let mut profile = ConfigDocument::default().profiles.office;
        profile
            .button_mappings
            .retain(|mapping| mapping.physical_control != "g102:middle");

        assert!(button_actions_for_profile(&profile).is_err());
    }

    #[test]
    fn target_maps_to_sanitized_ipc_snapshot() {
        let target = EnvironmentTarget {
            profile_key: "cs2".to_owned(),
            profile_name: "CS2".to_owned(),
            executable_name: "private-process.exe".to_owned(),
            process_id: 42,
            environment: Environment::Cs2,
            dpi: 800,
            report_rate_hz: 1000,
            button_actions: button_actions_for_profile(&ConfigDocument::default().profiles.cs2)
                .unwrap(),
            dpi_levels: [800, 1600, 2400, 3200],
        };

        let snapshot = snapshot_for_target(&target, 7, DeviceStatus::Ready, Some(800));

        assert_eq!(snapshot.device_status, DeviceStatus::Ready);
        assert_eq!(snapshot.active_environment, IpcEnvironment::Cs2);
        assert_eq!(snapshot.config_revision, 7);
        assert_eq!(snapshot.current_dpi, Some(800));
        assert_eq!(snapshot.desired_dpi, 800);
    }

    #[test]
    fn auto_mode_matches_imported_application_case_insensitively() {
        let mut config = ConfigDocument::default();
        config
            .applications
            .push(pulsehub_config_store::ApplicationProfileConfig {
                id: "winword".to_owned(),
                name: "Word".to_owned(),
                executable_path: r"C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE"
                    .to_owned(),
                process_name: "WINWORD.EXE".to_owned(),
                profile: config.profiles.cs2.clone(),
            });

        let selected = custom_application_for_process(&config, "winword.exe").unwrap();
        assert_eq!(selected.id, "winword");
        config.selection.mode = SelectionMode::Office;
        assert!(custom_application_for_process(&config, "WINWORD.EXE").is_none());
        config.selection.mode = SelectionMode::Application;
        config.selection.fixed_application_id = Some("winword".to_owned());
        let fixed = custom_application_for_process(&config, "explorer.exe").unwrap();
        assert_eq!(fixed.id, "winword");
    }

    #[test]
    fn dpi_shortcuts_are_derived_from_continuous_capability() {
        assert_eq!(
            suggested_dpi_values(50, 8000, Some(50), &[50, 8000]),
            [100, 400, 800, 1600, 1800, 3200]
        );
        assert_eq!(
            suggested_dpi_values(400, 1600, Some(400), &[400, 1600]),
            [400, 800, 1600]
        );
    }

    #[cfg(windows)]
    #[test]
    fn startup_command_quotes_agent_path_and_uses_daemon_arguments() {
        assert_eq!(
            startup_command(Path::new(r"C:\Program Files\PulseHub\pulsehub-agent.exe")),
            r#""C:\Program Files\PulseHub\pulsehub-agent.exe" --run-agent --confirm-device-write"#
        );
    }

    #[test]
    fn switch_guard_requires_stability_and_merges_transient_targets() {
        let started = Instant::now();
        let mut guard = EnvironmentSwitchGuard::default();

        assert_eq!(
            guard.decide("cs2", Some("office"), started),
            SwitchDecision::Wait(ENVIRONMENT_STABLE_FOR)
        );
        assert_eq!(
            guard.decide(
                "office",
                Some("office"),
                started + Duration::from_millis(400)
            ),
            SwitchDecision::AlreadyActive
        );
        assert_eq!(
            guard.decide("cs2", Some("office"), started + Duration::from_millis(500)),
            SwitchDecision::Wait(ENVIRONMENT_STABLE_FOR)
        );
        assert_eq!(
            guard.decide("cs2", Some("office"), started + Duration::from_millis(1500)),
            SwitchDecision::Apply
        );
    }

    #[test]
    fn switch_guard_enforces_profile_apply_cooldown() {
        let started = Instant::now();
        let mut guard = EnvironmentSwitchGuard::default();
        guard.record_applied(started);

        assert_eq!(
            guard.decide("cs2", Some("office"), started + Duration::from_secs(1)),
            SwitchDecision::Wait(ENVIRONMENT_STABLE_FOR)
        );
        assert_eq!(
            guard.decide("cs2", Some("office"), started + Duration::from_secs(2)),
            SwitchDecision::Wait(Duration::from_secs(3))
        );
        assert_eq!(
            guard.decide("cs2", Some("office"), started + PROFILE_APPLY_COOLDOWN),
            SwitchDecision::Apply
        );
    }

    #[test]
    fn device_health_ignores_transient_failures_and_signals_once() {
        let mut health = DeviceHealthMonitor::default();

        assert!(!health.record_failure());
        assert!(!health.record_failure());
        health.record_success();
        assert!(!health.record_failure());
        assert!(!health.record_failure());
        assert!(health.record_failure());
        assert!(!health.record_failure());
    }

    #[test]
    fn device_health_can_signal_again_after_recovery() {
        let mut health = DeviceHealthMonitor::default();
        for _ in 0..DEVICE_FAILURE_THRESHOLD - 1 {
            assert!(!health.record_failure());
        }
        assert!(health.record_failure());
        health.record_success();
        for _ in 0..DEVICE_FAILURE_THRESHOLD - 1 {
            assert!(!health.record_failure());
        }
        assert!(health.record_failure());
    }
}
