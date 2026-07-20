#![forbid(unsafe_code)]

use std::env;
use std::process::ExitCode;
#[cfg(windows)]
use std::time::Duration;

use pulsehub_ipc::{AgentSnapshot, PROTOCOL_VERSION, Request, Response, read_frame, write_frame};
use pulsehub_ui::{AppWindow, MappingItem};
use slint::ComponentHandle;
#[cfg(windows)]
use slint::{Model, ModelRc, VecModel};

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

    let close_ui = ui.as_weak();
    ui.window().on_close_requested(move || {
        let Some(ui) = close_ui.upgrade() else {
            return slint::CloseRequestResponse::HideWindow;
        };
        if should_prompt_before_close(ui.get_draft_dirty()) {
            ui.set_close_dialog_visible(true);
            slint::CloseRequestResponse::KeepWindowShown
        } else {
            slint::CloseRequestResponse::HideWindow
        }
    });

    let refresh_ui = ui.as_weak();
    ui.on_refresh_requested(move || {
        if let Some(ui) = refresh_ui.upgrade()
            && ui.get_draft_dirty()
        {
            ui.set_save_title("有未保存的更改".into());
            ui.set_save_detail("请先保存或放弃更改，再刷新代理状态。".into());
            return;
        }
        refresh_gui(refresh_ui.clone());
    });

    let save_ui = ui.as_weak();
    ui.on_save_requested(move |office_dpi, cs2_dpi, base_revision| {
        if let Some(ui) = save_ui.upgrade() {
            ui.set_busy(true);
            ui.set_save_title("正在检查配置".into());
            ui.set_save_detail("代理正在校验草稿，保存按钮暂时不可用。".into());
        }
        let office_mappings = save_ui
            .upgrade()
            .map(|ui| mapping_selections(&ui.get_office_mappings()))
            .unwrap_or_default();
        let cs2_mappings = save_ui
            .upgrade()
            .map(|ui| mapping_selections(&ui.get_cs2_mappings()))
            .unwrap_or_default();
        let worker_ui = save_ui.clone();
        std::thread::spawn(move || {
            let result = save_gui_config(
                office_dpi as u16,
                cs2_dpi as u16,
                base_revision as u64,
                &office_mappings,
                &cs2_mappings,
            );
            let _ = slint::invoke_from_event_loop(move || match result {
                Ok((snapshot, config)) => {
                    if let Some(ui) = worker_ui.upgrade() {
                        apply_gui_state(&ui, &snapshot, &config);
                        ui.set_draft_dirty(false);
                        ui.set_busy(false);
                        ui.set_save_title("配置已保存".into());
                        ui.set_save_detail("配置文件已经更新；设备应用状态由代理独立管理。".into());
                        if ui.get_close_after_save() {
                            ui.set_close_after_save(false);
                            let _ = ui.hide();
                        }
                    }
                }
                Err(error) if is_revision_conflict(&error) => show_gui_conflict(&worker_ui),
                Err(error) => show_gui_error(&worker_ui, "未能保存配置", &error),
            });
        });
    });

    let office_ui = ui.as_weak();
    ui.on_office_mapping_cycle(move |index| cycle_mapping(&office_ui, true, index));
    let cs2_ui = ui.as_weak();
    ui.on_cs2_mapping_cycle(move |index| cycle_mapping(&cs2_ui, false, index));

    let discard_ui = ui.as_weak();
    ui.on_discard_requested(move || refresh_gui(discard_ui.clone()));
    let close_discard_ui = ui.as_weak();
    ui.on_discard_and_close_requested(move || {
        if let Some(ui) = close_discard_ui.upgrade() {
            ui.set_draft_dirty(false);
            ui.set_close_after_save(false);
            let _ = ui.hide();
        }
    });
    refresh_gui(ui.as_weak());

    match ui.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("PulseHub UI 运行失败：{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(windows)]
fn show_gui_conflict(ui: &slint::Weak<AppWindow>) {
    if let Some(window) = ui.upgrade() {
        window.set_busy(false);
        window.set_close_after_save(false);
        window.set_save_title("配置已在其他窗口更新".into());
        window
            .set_save_detail("当前草稿基于旧版本。为避免覆盖新配置，请放弃更改并重新载入。".into());
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
        window.set_close_after_save(false);
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
    ui.set_base_revision(snapshot.config_revision.try_into().unwrap_or(i32::MAX));
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
    ui.set_office_mappings(mapping_model(&config.profiles.office.button_mappings));
    ui.set_cs2_mappings(mapping_model(&config.profiles.cs2.button_mappings));
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
    base_revision: u64,
    office_mappings: &[(String, String)],
    cs2_mappings: &[(String, String)],
) -> Result<(AgentSnapshot, pulsehub_config_store::ConfigDocument), String> {
    use pulsehub_ipc::windows::{connect_with_retry, default_pipe_path};

    let path = pulsehub_config_store::default_config_path().map_err(|error| error.to_string())?;
    let mut config =
        pulsehub_config_store::load_or_create_default(&path).map_err(|error| error.to_string())?;
    config.profiles.office.dpi = office_dpi;
    config.profiles.cs2.dpi = cs2_dpi;
    apply_mapping_selections(&mut config.profiles.office.button_mappings, office_mappings)?;
    apply_mapping_selections(&mut config.profiles.cs2.button_mappings, cs2_mappings)?;
    let draft = serde_json::to_value(&config).map_err(|error| error.to_string())?;
    let mut stream = connect_with_retry(
        default_pipe_path().map_err(|error| error.to_string())?,
        Duration::from_secs(5),
        Duration::from_millis(100),
    )
    .map_err(|error| error.to_string())?;
    negotiate(&mut stream)?;
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
            base_revision,
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
fn mapping_model(mappings: &[pulsehub_config_store::ButtonMappingConfig]) -> ModelRc<MappingItem> {
    ModelRc::new(VecModel::from(
        mappings.iter().map(mapping_item).collect::<Vec<_>>(),
    ))
}

#[cfg(windows)]
fn mapping_item(mapping: &pulsehub_config_store::ButtonMappingConfig) -> MappingItem {
    let (action_id, action_label, known) = action_identity(&mapping.action);
    let protected = matches!(
        mapping.physical_control.as_str(),
        "g102:left" | "g102:right"
    );
    MappingItem {
        control_id: mapping.physical_control.clone().into(),
        label: control_label(&mapping.physical_control).into(),
        action_id: action_id.into(),
        action_label: action_label.into(),
        locked: protected || !known,
    }
}

#[cfg(windows)]
fn action_identity(
    action: &pulsehub_config_store::ButtonActionConfig,
) -> (&'static str, &'static str, bool) {
    use pulsehub_config_store::ButtonActionConfig;
    match action {
        ButtonActionConfig::LogicalControl { value } if value == "mouse:left" => {
            ("left", "左键点击", true)
        }
        ButtonActionConfig::LogicalControl { value } if value == "mouse:right" => {
            ("right", "右键点击", true)
        }
        ButtonActionConfig::OnboardKeyboard {
            usage_page: 7,
            usage: 0x2a,
            modifiers: 0,
        } => ("backspace", "Backspace", true),
        ButtonActionConfig::OnboardKeyboard {
            usage_page: 7,
            usage: 0x19,
            modifiers: 1,
        } => ("paste", "Ctrl + V", true),
        ButtonActionConfig::OnboardKeyboard {
            usage_page: 7,
            usage: 0x06,
            modifiers: 1,
        } => ("copy", "Ctrl + C", true),
        ButtonActionConfig::OnboardKeyboard {
            usage_page: 7,
            usage: 0x04,
            modifiers: 1,
        } => ("select_all", "Ctrl + A", true),
        ButtonActionConfig::Disabled => ("disabled", "禁用", true),
        _ => ("custom", "已配置动作", false),
    }
}

#[cfg(windows)]
fn control_label(control: &str) -> &'static str {
    match control {
        "g102:left" => "左键",
        "g102:right" => "右键",
        "g102:middle" => "滚轮键（中键）",
        "g102:side_back" => "侧后键（G4）",
        "g102:side_forward" => "侧前键（G5）",
        "g102:dpi" => "DPI 键（G6）",
        _ => "其他按键",
    }
}

#[cfg(windows)]
fn cycle_mapping(ui: &slint::Weak<AppWindow>, office: bool, index: i32) {
    let Some(ui) = ui.upgrade() else { return };
    let model = if office {
        ui.get_office_mappings()
    } else {
        ui.get_cs2_mappings()
    };
    let Ok(index) = usize::try_from(index) else {
        return;
    };
    let Some(mut row) = model.row_data(index) else {
        return;
    };
    if row.locked {
        return;
    }
    let (id, label) = next_action(row.action_id.as_str());
    row.action_id = id.into();
    row.action_label = label.into();
    model.set_row_data(index, row);
    ui.set_draft_dirty(true);
}

fn next_action(current: &str) -> (&'static str, &'static str) {
    match current {
        "backspace" => ("paste", "Ctrl + V"),
        "paste" => ("copy", "Ctrl + C"),
        "copy" => ("select_all", "Ctrl + A"),
        "select_all" => ("disabled", "禁用"),
        _ => ("backspace", "Backspace"),
    }
}

#[cfg(windows)]
fn mapping_selections(model: &ModelRc<MappingItem>) -> Vec<(String, String)> {
    (0..model.row_count())
        .filter_map(|index| model.row_data(index))
        .map(|row| (row.control_id.to_string(), row.action_id.to_string()))
        .collect()
}

fn apply_mapping_selections(
    mappings: &mut [pulsehub_config_store::ButtonMappingConfig],
    selections: &[(String, String)],
) -> Result<(), String> {
    for (control, action_id) in selections {
        let mapping = mappings
            .iter_mut()
            .find(|mapping| mapping.physical_control == *control)
            .ok_or_else(|| format!("配置缺少按键 {control}"))?;
        mapping.action = action_from_id(action_id)?;
    }
    Ok(())
}

fn action_from_id(id: &str) -> Result<pulsehub_config_store::ButtonActionConfig, String> {
    use pulsehub_config_store::ButtonActionConfig;
    let keyboard = |usage, modifiers| ButtonActionConfig::OnboardKeyboard {
        usage_page: 7,
        usage,
        modifiers,
    };
    match id {
        "left" => Ok(ButtonActionConfig::LogicalControl {
            value: "mouse:left".to_owned(),
        }),
        "right" => Ok(ButtonActionConfig::LogicalControl {
            value: "mouse:right".to_owned(),
        }),
        "backspace" => Ok(keyboard(0x2a, 0)),
        "paste" => Ok(keyboard(0x19, 1)),
        "copy" => Ok(keyboard(0x06, 1)),
        "select_all" => Ok(keyboard(0x04, 1)),
        "disabled" => Ok(ButtonActionConfig::Disabled),
        _ => Err(format!("不支持的按键动作：{id}")),
    }
}

fn is_revision_conflict(error: &str) -> bool {
    error.contains("PH-IPC-CONFLICT")
}

fn should_prompt_before_close(draft_dirty: bool) -> bool {
    draft_dirty
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

#[cfg(test)]
mod tests {
    use super::{
        apply_mapping_selections, is_revision_conflict, next_action, should_prompt_before_close,
    };

    #[test]
    fn recognizes_revision_conflict_without_matching_other_errors() {
        assert!(is_revision_conflict("\"PH-IPC-CONFLICT\"：配置修订冲突"));
        assert!(!is_revision_conflict("\"PH-IPC-BUSY\"：设备协调者响应超时"));
    }

    #[test]
    fn mapping_editor_cycles_only_through_supported_actions() {
        assert_eq!(next_action("paste"), ("copy", "Ctrl + C"));
        assert_eq!(next_action("select_all"), ("disabled", "禁用"));
        assert_eq!(next_action("disabled"), ("backspace", "Backspace"));
    }

    #[test]
    fn mapping_selection_updates_draft_and_keeps_primary_clicks_valid() {
        let mut config = pulsehub_config_store::ConfigDocument::default();
        apply_mapping_selections(
            &mut config.profiles.office.button_mappings,
            &[("g102:side_back".to_owned(), "copy".to_owned())],
        )
        .unwrap();
        config.validate().unwrap();
        let side_back = config
            .profiles
            .office
            .button_mappings
            .iter()
            .find(|mapping| mapping.physical_control == "g102:side_back")
            .unwrap();
        assert!(matches!(
            side_back.action,
            pulsehub_config_store::ButtonActionConfig::OnboardKeyboard {
                usage: 0x06,
                modifiers: 1,
                ..
            }
        ));
    }

    #[test]
    fn close_only_prompts_for_dirty_drafts() {
        assert!(!should_prompt_before_close(false));
        assert!(should_prompt_before_close(true));
    }
}
