#![forbid(unsafe_code)]

use std::env;
use std::process::ExitCode;
#[cfg(windows)]
use std::time::Duration;

use pulsehub_ipc::{AgentSnapshot, PROTOCOL_VERSION, Request, Response, read_frame, write_frame};
use pulsehub_ui::AppWindow;
use slint::ComponentHandle;
#[cfg(windows)]
use slint::{ModelRc, VecModel};

#[derive(Clone, Copy)]
enum AgentAction {
    Inspect,
    Apply,
    ValidateCurrentConfig,
    CommitCurrentConfig,
}

fn main() -> ExitCode {
    match env::args().nth(1).as_deref() {
        Some("--inspect-agent") => run_agent_action(AgentAction::Inspect),
        Some("--apply-agent") => run_agent_action(AgentAction::Apply),
        Some("--validate-current-config") => run_agent_action(AgentAction::ValidateCurrentConfig),
        Some("--commit-current-config") => run_agent_action(AgentAction::CommitCurrentConfig),
        Some("-h" | "--help") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some(argument) => {
            eprintln!("未知参数：{argument}");
            print_help();
            ExitCode::from(2)
        }
        None => run_gui(),
    }
}

#[cfg(windows)]
fn run_gui() -> ExitCode {
    let ui = match AppWindow::new() {
        Ok(ui) => ui,
        Err(error) => {
            eprintln!("PulseHub 窗口创建失败：{error}");
            return ExitCode::FAILURE;
        }
    };

    let refresh_ui = ui.as_weak();
    ui.on_refresh_requested(move || refresh_gui(refresh_ui.clone()));

    let save_ui = ui.as_weak();
    ui.on_save_requested(move |office_dpi, cs2_dpi| {
        if let Some(ui) = save_ui.upgrade() {
            ui.set_busy(true);
            ui.set_save_title("正在检查配置".into());
            ui.set_save_detail("代理正在校验草稿，保存按钮暂时不可用。".into());
        }
        let worker_ui = save_ui.clone();
        std::thread::spawn(move || {
            let result = save_gui_config(office_dpi as u16, cs2_dpi as u16);
            let _ = slint::invoke_from_event_loop(move || match result {
                Ok((snapshot, config)) => {
                    if let Some(ui) = worker_ui.upgrade() {
                        apply_gui_state(&ui, &snapshot, &config);
                        ui.set_draft_dirty(false);
                        ui.set_busy(false);
                        ui.set_save_title("配置已保存".into());
                        ui.set_save_detail("配置文件已经更新；设备应用状态由代理独立管理。".into());
                    }
                }
                Err(error) => show_gui_error(&worker_ui, "未能保存配置", &error),
            });
        });
    });

    let discard_ui = ui.as_weak();
    ui.on_discard_requested(move || refresh_gui(discard_ui.clone()));
    refresh_gui(ui.as_weak());

    match ui.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("PulseHub UI 运行失败：{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn run_gui() -> ExitCode {
    eprintln!("PulseHub Slint GUI 目前仅支持 Windows。");
    ExitCode::FAILURE
}

#[cfg(windows)]
fn refresh_gui(ui: slint::Weak<AppWindow>) {
    if let Some(window) = ui.upgrade() {
        window.set_busy(true);
        window.set_connection_text("正在连接代理".into());
    }
    std::thread::spawn(move || {
        let result = load_gui_state();
        let _ = slint::invoke_from_event_loop(move || match result {
            Ok((snapshot, config)) => {
                if let Some(window) = ui.upgrade() {
                    apply_gui_state(&window, &snapshot, &config);
                    window.set_draft_dirty(false);
                    window.set_busy(false);
                }
            }
            Err(error) => show_gui_error(&ui, "代理未连接", &error),
        });
    });
}

#[cfg(windows)]
fn show_gui_error(ui: &slint::Weak<AppWindow>, title: &str, error: &str) {
    if let Some(window) = ui.upgrade() {
        window.set_busy(false);
        window.set_connection_text("代理离线".into());
        window.set_device_status("无法读取设备状态".into());
        window.set_save_title(title.into());
        window.set_save_detail(format!("{error} 配置草稿仍保留在窗口中。").into());
    }
}

#[cfg(windows)]
fn apply_gui_state(
    ui: &AppWindow,
    snapshot: &AgentSnapshot,
    config: &pulsehub_config_store::ConfigDocument,
) {
    ui.set_connection_text("代理已连接".into());
    ui.set_device_status(
        match snapshot.device_status {
            pulsehub_ipc::DeviceStatus::Ready => "设备已连接",
            pulsehub_ipc::DeviceStatus::Disconnected => "未检测到设备",
            pulsehub_ipc::DeviceStatus::Busy => "设备正被其他程序占用",
            pulsehub_ipc::DeviceStatus::Degraded => "设备状态不完整",
            pulsehub_ipc::DeviceStatus::Unknown => "正在读取设备状态",
        }
        .into(),
    );
    ui.set_environment_name(
        match snapshot.active_environment {
            pulsehub_ipc::Environment::Office => "Office",
            pulsehub_ipc::Environment::Cs2 => "CS2",
        }
        .into(),
    );
    ui.set_current_dpi(
        snapshot
            .current_dpi
            .map_or_else(|| "—".to_owned(), |dpi| dpi.to_string())
            .into(),
    );
    ui.set_desired_dpi(snapshot.desired_dpi.to_string().into());
    ui.set_revision(snapshot.config_revision.to_string().into());
    let dpi_values = snapshot
        .dpi_capability
        .as_ref()
        .map(|capability| {
            capability
                .selectable_values
                .iter()
                .copied()
                .map(i32::from)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    ui.set_dpi_options(ModelRc::new(VecModel::from(dpi_values)));
    ui.set_office_dpi(config.profiles.office.dpi.into());
    ui.set_cs2_dpi(config.profiles.cs2.dpi.into());
    ui.set_save_title(
        if snapshot.current_dpi == Some(snapshot.desired_dpi) {
            "已应用"
        } else {
            "已保存，正在等待应用"
        }
        .into(),
    );
    ui.set_save_detail(
        if snapshot.current_dpi == Some(snapshot.desired_dpi) {
            "配置已保存并应用到设备。"
        } else {
            "配置文件已保存；设备状态与保存状态分开显示。"
        }
        .into(),
    );
}

#[cfg(windows)]
fn load_gui_state() -> Result<(AgentSnapshot, pulsehub_config_store::ConfigDocument), String> {
    let config_path =
        pulsehub_config_store::default_config_path().map_err(|error| error.to_string())?;
    let config = pulsehub_config_store::load_or_create_default(&config_path)
        .map_err(|error| error.to_string())?;
    let snapshot = request_snapshot()?;
    Ok((snapshot, config))
}

#[cfg(windows)]
fn save_gui_config(
    office_dpi: u16,
    cs2_dpi: u16,
) -> Result<(AgentSnapshot, pulsehub_config_store::ConfigDocument), String> {
    use pulsehub_ipc::windows::{connect_with_retry, default_pipe_path};

    let path = pulsehub_config_store::default_config_path().map_err(|error| error.to_string())?;
    let mut config =
        pulsehub_config_store::load_or_create_default(&path).map_err(|error| error.to_string())?;
    config.profiles.office.dpi = office_dpi;
    config.profiles.cs2.dpi = cs2_dpi;
    let draft = serde_json::to_value(&config).map_err(|error| error.to_string())?;
    let mut stream = connect_with_retry(
        default_pipe_path().map_err(|error| error.to_string())?,
        Duration::from_secs(5),
        Duration::from_millis(100),
    )
    .map_err(|error| error.to_string())?;
    negotiate(&mut stream)?;
    let before = request_snapshot_on(&mut stream, "gui-before-save")?;
    exchange(
        &mut stream,
        &Request::ValidateDraft {
            version: PROTOCOL_VERSION,
            request_id: "gui-validate".into(),
            draft: draft.clone(),
        },
    )?;
    let response = exchange(
        &mut stream,
        &Request::CommitConfig {
            version: PROTOCOL_VERSION,
            request_id: "gui-commit".into(),
            base_revision: before.config_revision,
            draft,
        },
    )?;
    let snapshot = response
        .data
        .and_then(|data| serde_json::from_value(data).ok())
        .ok_or_else(|| "代理返回的提交快照无效".to_owned())?;
    Ok((snapshot, config))
}

#[cfg(windows)]
fn request_snapshot() -> Result<AgentSnapshot, String> {
    use pulsehub_ipc::windows::{connect_with_retry, default_pipe_path};
    let mut stream = connect_with_retry(
        default_pipe_path().map_err(|error| error.to_string())?,
        Duration::from_secs(5),
        Duration::from_millis(100),
    )
    .map_err(|error| error.to_string())?;
    negotiate(&mut stream)?;
    request_snapshot_on(&mut stream, "gui-refresh")
}

#[cfg(windows)]
fn negotiate(stream: &mut pulsehub_ipc::windows::ByteStream) -> Result<(), String> {
    exchange(
        stream,
        &Request::Hello {
            version: PROTOCOL_VERSION,
            request_id: "gui-hello".into(),
            supported_versions: vec![PROTOCOL_VERSION],
            client: "pulsehub-config".into(),
        },
    )
    .map(|_| ())
}

#[cfg(windows)]
fn request_snapshot_on(
    stream: &mut pulsehub_ipc::windows::ByteStream,
    request_id: &str,
) -> Result<AgentSnapshot, String> {
    let response = exchange(
        stream,
        &Request::GetSnapshot {
            version: PROTOCOL_VERSION,
            request_id: request_id.into(),
        },
    )?;
    response
        .data
        .and_then(|data| serde_json::from_value(data).ok())
        .ok_or_else(|| "代理返回的快照无效".to_owned())
}

#[cfg(windows)]
fn run_agent_action(action: AgentAction) -> ExitCode {
    use pulsehub_ipc::windows::{connect_with_retry, default_pipe_path};

    let draft = if matches!(
        action,
        AgentAction::ValidateCurrentConfig | AgentAction::CommitCurrentConfig
    ) {
        let path = match pulsehub_config_store::default_config_path() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("配置路径错误：{error}");
                return ExitCode::FAILURE;
            }
        };
        let config = match pulsehub_config_store::load_or_create_default(&path) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("配置读取失败：{error}");
                return ExitCode::FAILURE;
            }
        };
        match serde_json::to_value(config) {
            Ok(value) => Some(value),
            Err(error) => {
                eprintln!("配置序列化失败：{error}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };

    let pipe_path = match default_pipe_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("IPC 管道名构造失败：{error}");
            return ExitCode::FAILURE;
        }
    };

    let mut stream = match connect_with_retry(
        pipe_path,
        Duration::from_secs(5),
        Duration::from_millis(100),
    ) {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("无法连接 PulseHub agent：{error}");
            return ExitCode::FAILURE;
        }
    };
    let hello = Request::Hello {
        version: PROTOCOL_VERSION,
        request_id: "config-hello-1".to_owned(),
        supported_versions: vec![PROTOCOL_VERSION],
        client: "pulsehub-config".to_owned(),
    };
    if let Err(error) = exchange(&mut stream, &hello) {
        eprintln!("IPC 版本协商失败：{error}");
        return ExitCode::FAILURE;
    }
    let base_revision = if matches!(action, AgentAction::CommitCurrentConfig) {
        let snapshot_request = Request::GetSnapshot {
            version: PROTOCOL_VERSION,
            request_id: "config-before-commit-1".to_owned(),
        };
        let response = match exchange(&mut stream, &snapshot_request) {
            Ok(response) => response,
            Err(error) => {
                eprintln!("提交前读取配置修订失败：{error}");
                return ExitCode::FAILURE;
            }
        };
        let snapshot = match response
            .data
            .and_then(|data| serde_json::from_value::<AgentSnapshot>(data).ok())
        {
            Some(snapshot) => snapshot,
            None => {
                eprintln!("提交前的代理快照格式无效");
                return ExitCode::FAILURE;
            }
        };
        snapshot.config_revision
    } else {
        0
    };
    let request = match action {
        AgentAction::Apply => Request::ApplyNow {
            version: PROTOCOL_VERSION,
            request_id: "config-apply-1".to_owned(),
        },
        AgentAction::Inspect => Request::GetSnapshot {
            version: PROTOCOL_VERSION,
            request_id: "config-snapshot-1".to_owned(),
        },
        AgentAction::ValidateCurrentConfig => Request::ValidateDraft {
            version: PROTOCOL_VERSION,
            request_id: "config-validate-1".to_owned(),
            draft: draft.expect("验证操作必须加载草稿"),
        },
        AgentAction::CommitCurrentConfig => Request::CommitConfig {
            version: PROTOCOL_VERSION,
            request_id: "config-commit-1".to_owned(),
            base_revision,
            draft: draft.expect("提交操作必须加载草稿"),
        },
    };
    let response = match exchange(&mut stream, &request) {
        Ok(response) => response,
        Err(error) => {
            eprintln!("IPC 快照读取失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    let Some(data) = response.data else {
        eprintln!("IPC 快照响应缺少 data");
        return ExitCode::FAILURE;
    };
    if matches!(action, AgentAction::ValidateCurrentConfig) {
        println!("配置草稿验证通过：{data}");
        return ExitCode::SUCCESS;
    }
    let snapshot: AgentSnapshot = match serde_json::from_value(data) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("IPC 快照格式无效：{error}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "{}：设备={:?}，环境={:?}，当前 DPI={}，目标 DPI={}，配置修订={}",
        match action {
            AgentAction::Apply => "应用完成",
            AgentAction::CommitCurrentConfig => "配置提交完成",
            _ => "代理快照",
        },
        snapshot.device_status,
        snapshot.active_environment,
        snapshot
            .current_dpi
            .map_or_else(|| "未知".to_owned(), |dpi| dpi.to_string()),
        snapshot.desired_dpi,
        snapshot.config_revision
    );
    if let Some(capability) = &snapshot.dpi_capability {
        println!(
            "DPI 能力：{}–{}，步进={}，界面选项={:?}",
            capability.minimum,
            capability.maximum,
            capability
                .step
                .map_or_else(|| "离散值".to_owned(), |step| step.to_string()),
            capability.selectable_values
        );
    }
    ExitCode::SUCCESS
}

#[cfg(windows)]
fn exchange(
    stream: &mut pulsehub_ipc::windows::ByteStream,
    request: &Request,
) -> Result<Response, String> {
    write_frame(stream, request).map_err(|error| error.to_string())?;
    let response: Response = read_frame(stream).map_err(|error| error.to_string())?;
    response.validate().map_err(|error| error.to_string())?;
    if let Some(error) = &response.error {
        return Err(format!(
            "{}：{}",
            serde_json::to_string(&error.code).unwrap(),
            error.message
        ));
    }
    Ok(response)
}

#[cfg(not(windows))]
fn run_agent_action(_: AgentAction) -> ExitCode {
    eprintln!("IPC Named Pipe 客户端仅支持 Windows。");
    ExitCode::FAILURE
}

fn print_help() {
    println!(
        "用法：pulsehub-config [--inspect-agent | --apply-agent | --validate-current-config | --commit-current-config]"
    );
    println!("  --inspect-agent  连接代理并读取只读脱敏快照");
    println!("  --apply-agent  请求代理应用当前前台环境并返回回读快照");
    println!("  --validate-current-config  请求代理验证磁盘上的当前配置，不保存");
    println!("  --commit-current-config  按代理当前修订提交磁盘上的配置，不自动写 HID");
}
