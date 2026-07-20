#![forbid(unsafe_code)]

use std::env;
use std::process::ExitCode;
#[cfg(windows)]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(windows)]
use std::time::Duration;

use pulsehub_ipc::{AgentSnapshot, PROTOCOL_VERSION, Request, Response, read_frame, write_frame};
use pulsehub_ui::{AppWindow, MappingItem};
use slint::ComponentHandle;
#[cfg(windows)]
use slint::{Model, ModelRc, Timer, TimerMode, VecModel};

#[derive(Clone, Copy)]
enum AgentAction {
    Inspect,
    Apply,
    ValidateCurrentConfig,
    CommitCurrentConfig,
    SetSelectionMode(pulsehub_ipc::SelectionMode),
    SetStartup(bool),
}

fn main() -> ExitCode {
    match env::args().nth(1).as_deref() {
        Some("--inspect-agent") => run_agent_action(AgentAction::Inspect),
        Some("--apply-agent") => run_agent_action(AgentAction::Apply),
        Some("--validate-current-config") => run_agent_action(AgentAction::ValidateCurrentConfig),
        Some("--commit-current-config") => run_agent_action(AgentAction::CommitCurrentConfig),
        Some("--set-selection-auto") => run_agent_action(AgentAction::SetSelectionMode(
            pulsehub_ipc::SelectionMode::Auto,
        )),
        Some("--set-selection-office") => run_agent_action(AgentAction::SetSelectionMode(
            pulsehub_ipc::SelectionMode::Office,
        )),
        Some("--set-selection-cs2") => run_agent_action(AgentAction::SetSelectionMode(
            pulsehub_ipc::SelectionMode::Cs2,
        )),
        Some("--enable-startup") => run_agent_action(AgentAction::SetStartup(true)),
        Some("--disable-startup") => run_agent_action(AgentAction::SetStartup(false)),
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
    ui.on_save_requested(
        move |office_dpi,
              cs2_dpi,
              base_revision,
              selection_mode,
              start_with_windows,
              developer_logging| {
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
            let office_dpi_levels = save_ui
                .upgrade()
                .map(|ui| dpi_levels_from_ui(&ui, true))
                .unwrap_or([800, 1600, 2400, 3200]);
            let cs2_dpi_levels = save_ui
                .upgrade()
                .map(|ui| dpi_levels_from_ui(&ui, false))
                .unwrap_or([800, 1600, 2400, 3200]);
            let worker_ui = save_ui.clone();
            std::thread::spawn(move || {
                let result = save_gui_config(
                    office_dpi as u16,
                    cs2_dpi as u16,
                    base_revision as u64,
                    &office_mappings,
                    &cs2_mappings,
                    office_dpi_levels,
                    cs2_dpi_levels,
                    selection_mode.as_str(),
                    start_with_windows,
                    developer_logging,
                );
                let _ = slint::invoke_from_event_loop(move || match result {
                    Ok((snapshot, config)) => {
                        if let Some(ui) = worker_ui.upgrade() {
                            apply_gui_state(&ui, &snapshot, &config);
                            ui.set_draft_dirty(false);
                            ui.set_busy(false);
                            ui.set_save_title("配置已保存".into());
                            ui.set_save_detail(
                                "配置文件已经更新；设备应用状态由代理独立管理。".into(),
                            );
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
        },
    );

    let restore_ui = ui.as_weak();
    ui.on_mapping_restore_requested(move |office, index| {
        restore_mapping(&restore_ui, office, index)
    });
    let key_ui = ui.as_weak();
    ui.on_mapping_key_requested(move |office, index, key, control, shift, alt, meta| {
        capture_mapping(
            &key_ui,
            office,
            index,
            key.as_str(),
            control,
            shift,
            alt,
            meta,
        )
    });
    let dpi_ui = ui.as_weak();
    ui.on_custom_dpi_requested(move |office, value| {
        set_custom_dpi(&dpi_ui, office, value.as_str())
    });
    let level_ui = ui.as_weak();
    ui.on_dpi_level_requested(move |office, index, value| {
        set_dpi_level(&level_ui, office, index, value.as_str())
    });

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
    let apply_ui = ui.as_weak();
    ui.on_retry_apply_requested(move || {
        if let Some(ui) = apply_ui.upgrade() {
            ui.set_busy(true);
            ui.set_save_title("正在应用配置".into());
            ui.set_save_detail("代理正在写入运行态 DPI 并执行回读校验。".into());
        }
        let worker_ui = apply_ui.clone();
        std::thread::spawn(move || {
            let result = apply_agent_now();
            let _ = slint::invoke_from_event_loop(move || match result {
                Ok((snapshot, config)) => {
                    if let Some(ui) = worker_ui.upgrade() {
                        apply_gui_state(&ui, &snapshot, &config);
                        ui.set_busy(false);
                    }
                }
                Err(error) => show_apply_error(&worker_ui, &error),
            });
        });
    });
    refresh_gui(ui.as_weak());

    let live_poll_running = Arc::new(AtomicBool::new(false));
    let live_poll_flag = Arc::clone(&live_poll_running);
    let live_poll_ui = ui.as_weak();
    let live_poll_timer = Timer::default();
    live_poll_timer.start(TimerMode::Repeated, Duration::from_millis(750), move || {
        if live_poll_flag.swap(true, Ordering::AcqRel) {
            return;
        }
        let worker_ui = live_poll_ui.clone();
        let worker_flag = Arc::clone(&live_poll_flag);
        std::thread::spawn(move || {
            let snapshot = request_snapshot();
            let _ = slint::invoke_from_event_loop(move || {
                if let (Ok(snapshot), Some(window)) = (snapshot, worker_ui.upgrade()) {
                    window.set_current_dpi(
                        snapshot
                            .current_dpi
                            .map_or_else(|| "—".to_owned(), |dpi| dpi.to_string())
                            .into(),
                    );
                }
                worker_flag.store(false, Ordering::Release);
            });
        });
    });

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
fn show_apply_error(ui: &slint::Weak<AppWindow>, error: &str) {
    if let Some(window) = ui.upgrade() {
        window.set_busy(false);
        window.set_save_title("配置未能应用".into());
        let guidance = if error.contains("PH-IPC-BUSY") {
            "设备暂时忙，请稍后重试；若确有其他鼠标管理程序运行，再将其退出。"
        } else {
            "请按错误内容检查配置值或设备连接后重试。"
        };
        window.set_save_detail(format!("配置仍已安全保存。{guidance} 错误：{error}").into());
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
    if let Some(capability) = &snapshot.dpi_capability {
        ui.set_dpi_minimum(capability.minimum.into());
        ui.set_dpi_maximum(capability.maximum.into());
        ui.set_dpi_step(capability.step.unwrap_or(1).into());
    }
    ui.set_office_dpi(config.profiles.office.dpi.into());
    ui.set_cs2_dpi(config.profiles.cs2.dpi.into());
    set_dpi_levels_on_ui(ui, true, &config.profiles.office.dpi_levels);
    set_dpi_levels_on_ui(ui, false, &config.profiles.cs2.dpi_levels);
    ui.set_office_mappings(mapping_model(&config.profiles.office.button_mappings));
    ui.set_cs2_mappings(mapping_model(&config.profiles.cs2.button_mappings));
    ui.set_office_dpi_cycle_enabled(profile_uses_dpi_cycle(&config.profiles.office));
    ui.set_cs2_dpi_cycle_enabled(profile_uses_dpi_cycle(&config.profiles.cs2));
    ui.set_selection_mode(selection_mode_label(config.selection.mode).into());
    ui.set_start_with_windows(config.agent.start_with_windows);
    ui.set_developer_logging(config.agent.developer_logging);
    ui.set_integration_status(
        match snapshot.integration_status {
            pulsehub_ipc::IntegrationStatus::Unknown => "未知",
            pulsehub_ipc::IntegrationStatus::Synced => "已同步",
            pulsehub_ipc::IntegrationStatus::Failed => "同步失败",
        }
        .into(),
    );
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
#[allow(clippy::too_many_arguments)]
fn save_gui_config(
    office_dpi: u16,
    cs2_dpi: u16,
    base_revision: u64,
    office_mappings: &[(String, String)],
    cs2_mappings: &[(String, String)],
    office_dpi_levels: [u16; 4],
    cs2_dpi_levels: [u16; 4],
    selection_mode: &str,
    start_with_windows: bool,
    developer_logging: bool,
) -> Result<(AgentSnapshot, pulsehub_config_store::ConfigDocument), String> {
    use pulsehub_ipc::windows::{connect_with_retry, default_pipe_path};

    let path = pulsehub_config_store::default_config_path().map_err(|error| error.to_string())?;
    let mut config =
        pulsehub_config_store::load_or_create_default(&path).map_err(|error| error.to_string())?;
    config.profiles.office.dpi = office_dpi;
    config.profiles.cs2.dpi = cs2_dpi;
    config.profiles.office.dpi_levels = office_dpi_levels.to_vec();
    config.profiles.cs2.dpi_levels = cs2_dpi_levels.to_vec();
    apply_mapping_selections(&mut config.profiles.office.button_mappings, office_mappings)?;
    apply_mapping_selections(&mut config.profiles.cs2.button_mappings, cs2_mappings)?;
    config.selection.mode = selection_mode_from_label(selection_mode)?;
    config.agent.start_with_windows = start_with_windows;
    config.agent.developer_logging = developer_logging;
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
fn action_identity(action: &pulsehub_config_store::ButtonActionConfig) -> (String, String, bool) {
    use pulsehub_config_store::ButtonActionConfig;
    match action {
        ButtonActionConfig::LogicalControl { value } if value == "mouse:left" => {
            ("left".into(), "左键点击".into(), true)
        }
        ButtonActionConfig::LogicalControl { value } if value == "mouse:right" => {
            ("right".into(), "右键点击".into(), true)
        }
        ButtonActionConfig::LogicalControl { value } if value == "mouse:middle" => {
            ("middle".into(), "鼠标中键".into(), true)
        }
        ButtonActionConfig::LogicalControl { value } if value == "mouse:back" => {
            ("side_back".into(), "鼠标侧键（后）".into(), true)
        }
        ButtonActionConfig::LogicalControl { value } if value == "mouse:forward" => {
            ("side_forward".into(), "鼠标侧键（前）".into(), true)
        }
        ButtonActionConfig::LogicalControl { value } if value == "mouse:dpi_cycle" => {
            ("dpi_cycle".into(), "原本 DPI 切换".into(), true)
        }
        ButtonActionConfig::OnboardKeyboard {
            usage_page: 7,
            usage: 0x2a,
            modifiers: 0,
        } => ("key:42:0".into(), "Backspace".into(), true),
        ButtonActionConfig::OnboardKeyboard {
            usage_page: 7,
            usage,
            modifiers,
        } if *usage <= u16::from(u8::MAX) => (
            format!("key:{usage}:{modifiers}"),
            keyboard_action_label(*usage as u8, *modifiers),
            true,
        ),
        ButtonActionConfig::Disabled => ("disabled".into(), "禁用".into(), true),
        _ => ("custom".into(), "已配置动作".into(), false),
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
fn restore_mapping(ui: &slint::Weak<AppWindow>, office: bool, index: i32) {
    let Some(ui) = ui.upgrade() else { return };
    let model = if office {
        ui.get_office_mappings()
    } else {
        ui.get_cs2_mappings()
    };
    let Ok(index) = usize::try_from(index) else {
        return;
    };
    let Some(row) = model.row_data(index) else {
        return;
    };
    let (id, label) = original_action(row.control_id.as_str());
    update_mapping_row(&ui, &model, office, index, id, label);
}

#[cfg(windows)]
fn set_custom_dpi(ui: &slint::Weak<AppWindow>, office: bool, text: &str) {
    let Some(ui) = ui.upgrade() else { return };
    let minimum = ui.get_dpi_minimum();
    let maximum = ui.get_dpi_maximum();
    let step = ui.get_dpi_step().max(1);
    let value = match validate_custom_dpi(text, minimum, maximum, step) {
        Ok(value) => value,
        Err(error) => {
            ui.set_save_title("DPI 输入无效".into());
            ui.set_save_detail(error.into());
            return;
        }
    };
    if office {
        ui.set_office_dpi(value);
    } else {
        ui.set_cs2_dpi(value);
    }
    ui.set_draft_dirty(true);
    ui.set_save_title("自定义 DPI 已加入草稿".into());
    ui.set_save_detail(format!("{value} DPI 将在保存配置后生效。").into());
}

#[cfg(windows)]
fn set_dpi_level(ui: &slint::Weak<AppWindow>, office: bool, index: i32, text: &str) {
    let Some(ui) = ui.upgrade() else { return };
    let value = match validate_custom_dpi(
        text,
        ui.get_dpi_minimum(),
        ui.get_dpi_maximum(),
        ui.get_dpi_step(),
    ) {
        Ok(value) => value,
        Err(error) => {
            ui.set_save_title("DPI 档位无效".into());
            ui.set_save_detail(error.into());
            return;
        }
    };
    let Ok(index) = usize::try_from(index) else {
        return;
    };
    if index >= 4 {
        return;
    }
    let mut levels = dpi_levels_from_ui(&ui, office).map(i32::from);
    levels[index] = value;
    if !levels.windows(2).all(|pair| pair[0] < pair[1]) {
        ui.set_save_title("DPI 档位顺序无效".into());
        ui.set_save_detail("四个 DPI 档位必须从低到高排列且不能重复。".into());
        return;
    }
    set_dpi_levels_i32_on_ui(&ui, office, levels);
    ui.set_draft_dirty(true);
    ui.set_save_title("DPI 档位已加入草稿".into());
    ui.set_save_detail(format!("档位 {} 已设置为 {value} DPI。", index + 1).into());
}

#[cfg(windows)]
fn dpi_levels_from_ui(ui: &AppWindow, office: bool) -> [u16; 4] {
    let levels = if office {
        [
            ui.get_office_dpi_level_1(),
            ui.get_office_dpi_level_2(),
            ui.get_office_dpi_level_3(),
            ui.get_office_dpi_level_4(),
        ]
    } else {
        [
            ui.get_cs2_dpi_level_1(),
            ui.get_cs2_dpi_level_2(),
            ui.get_cs2_dpi_level_3(),
            ui.get_cs2_dpi_level_4(),
        ]
    };
    levels.map(|value| u16::try_from(value).unwrap_or_default())
}

#[cfg(windows)]
fn set_dpi_levels_on_ui(ui: &AppWindow, office: bool, levels: &[u16]) {
    if let Ok(levels) = <[u16; 4]>::try_from(levels) {
        set_dpi_levels_i32_on_ui(ui, office, levels.map(i32::from));
    }
}

#[cfg(windows)]
fn set_dpi_levels_i32_on_ui(ui: &AppWindow, office: bool, levels: [i32; 4]) {
    if office {
        ui.set_office_dpi_level_1(levels[0]);
        ui.set_office_dpi_level_2(levels[1]);
        ui.set_office_dpi_level_3(levels[2]);
        ui.set_office_dpi_level_4(levels[3]);
    } else {
        ui.set_cs2_dpi_level_1(levels[0]);
        ui.set_cs2_dpi_level_2(levels[1]);
        ui.set_cs2_dpi_level_3(levels[2]);
        ui.set_cs2_dpi_level_4(levels[3]);
    }
}

#[cfg(windows)]
fn profile_uses_dpi_cycle(profile: &pulsehub_config_store::ProfileConfig) -> bool {
    profile.button_mappings.iter().any(|mapping| {
        mapping.physical_control == "g102:dpi"
            && matches!(
                &mapping.action,
                pulsehub_config_store::ButtonActionConfig::LogicalControl { value }
                    if value == "mouse:dpi_cycle"
            )
    })
}

fn validate_custom_dpi(text: &str, minimum: i32, maximum: i32, step: i32) -> Result<i32, String> {
    let value = text
        .trim()
        .parse::<i32>()
        .map_err(|_| "请输入只包含数字的 DPI 值。".to_owned())?;
    let step = step.max(1);
    if value < minimum || value > maximum || (value - minimum) % step != 0 {
        return Err(format!(
            "请输入 {minimum}–{maximum} 且符合 {step} DPI 步进的值。"
        ));
    }
    Ok(value)
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn capture_mapping(
    ui: &slint::Weak<AppWindow>,
    office: bool,
    index: i32,
    key: &str,
    control: bool,
    shift: bool,
    alt: bool,
    meta: bool,
) {
    let Some(ui) = ui.upgrade() else { return };
    let model = if office {
        ui.get_office_mappings()
    } else {
        ui.get_cs2_mappings()
    };
    let Ok(index) = usize::try_from(index) else {
        return;
    };
    let Some(row) = model.row_data(index) else {
        return;
    };
    if row.locked {
        return;
    }
    match captured_keyboard_action(key, control, shift, alt, meta) {
        Ok((usage, modifiers, label)) => {
            update_mapping_row(
                &ui,
                &model,
                office,
                index,
                &format!("key:{usage}:{modifiers}"),
                &label,
            );
        }
        Err(error) => {
            ui.set_save_title("不支持该按键组合".into());
            ui.set_save_detail(error.into());
        }
    }
}

#[cfg(windows)]
fn update_mapping_row(
    ui: &AppWindow,
    model: &ModelRc<MappingItem>,
    office: bool,
    index: usize,
    id: &str,
    label: &str,
) {
    let Some(mut row) = model.row_data(index) else {
        return;
    };
    if row.locked {
        return;
    }
    row.action_id = id.into();
    row.action_label = label.into();
    model.set_row_data(index, row);
    if model
        .row_data(index)
        .is_some_and(|row| row.control_id.as_str() == "g102:dpi")
    {
        let enabled = id == "dpi_cycle";
        if office {
            ui.set_office_dpi_cycle_enabled(enabled);
        } else {
            ui.set_cs2_dpi_cycle_enabled(enabled);
        }
    }
    ui.set_draft_dirty(true);
}

fn original_action(control: &str) -> (&'static str, &'static str) {
    match control {
        "g102:middle" => ("middle", "鼠标中键"),
        "g102:side_back" => ("side_back", "鼠标侧键（后）"),
        "g102:side_forward" => ("side_forward", "鼠标侧键（前）"),
        "g102:dpi" => ("dpi_cycle", "原本 DPI 切换"),
        _ => ("disabled", "禁用"),
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
        "middle" => Ok(ButtonActionConfig::LogicalControl {
            value: "mouse:middle".to_owned(),
        }),
        "side_back" => Ok(ButtonActionConfig::LogicalControl {
            value: "mouse:back".to_owned(),
        }),
        "side_forward" => Ok(ButtonActionConfig::LogicalControl {
            value: "mouse:forward".to_owned(),
        }),
        "dpi_cycle" => Ok(ButtonActionConfig::LogicalControl {
            value: "mouse:dpi_cycle".to_owned(),
        }),
        "disabled" => Ok(ButtonActionConfig::Disabled),
        _ if id.starts_with("key:") => {
            let mut parts = id.split(':');
            let _ = parts.next();
            let usage = parts.next().and_then(|value| value.parse::<u16>().ok());
            let modifiers = parts.next().and_then(|value| value.parse::<u8>().ok());
            match (usage, modifiers, parts.next()) {
                (Some(usage @ 1..=0xe7), Some(modifiers), None) => Ok(keyboard(usage, modifiers)),
                _ => Err(format!("无效的键盘动作：{id}")),
            }
        }
        _ => Err(format!("不支持的按键动作：{id}")),
    }
}

fn captured_keyboard_action(
    key: &str,
    control: bool,
    shift: bool,
    alt: bool,
    meta: bool,
) -> Result<(u8, u8, String), String> {
    let usage = keyboard_usage_from_key(key)
        .ok_or_else(|| format!("当前 G102 板载格式不支持按键“{key}”。请换用常规键盘按键。"))?;
    let modifiers =
        u8::from(control) | (u8::from(shift) << 1) | (u8::from(alt) << 2) | (u8::from(meta) << 3);
    Ok((usage, modifiers, keyboard_action_label(usage, modifiers)))
}

fn keyboard_usage_from_key(key: &str) -> Option<u8> {
    let normalized = key.to_lowercase();
    let mut chars = normalized.chars();
    let character = chars.next()?;
    if chars.next().is_none() {
        return match character {
            'a'..='z' => Some(0x04 + (character as u8 - b'a')),
            '1'..='9' => Some(0x1e + (character as u8 - b'1')),
            '0' => Some(0x27),
            ' ' => Some(0x2c),
            '-' => Some(0x2d),
            '=' => Some(0x2e),
            '[' => Some(0x2f),
            ']' => Some(0x30),
            '\\' => Some(0x31),
            ';' => Some(0x33),
            '\'' => Some(0x34),
            '`' => Some(0x35),
            ',' => Some(0x36),
            '.' => Some(0x37),
            '/' => Some(0x38),
            '\u{f700}' => Some(0x52),
            '\u{f701}' => Some(0x51),
            '\u{f702}' => Some(0x50),
            '\u{f703}' => Some(0x4f),
            '\u{f704}'..='\u{f70f}' => Some(0x3a + (character as u8 - 0x04)),
            '\u{f727}' => Some(0x49),
            '\u{f729}' => Some(0x4a),
            '\u{f72b}' => Some(0x4d),
            '\u{f72c}' => Some(0x4b),
            '\u{f72d}' => Some(0x4e),
            _ => None,
        };
    }
    match key {
        "Backspace" => Some(0x2a),
        "Tab" => Some(0x2b),
        "Space" => Some(0x2c),
        "Delete" => Some(0x4c),
        "Left" => Some(0x50),
        "Right" => Some(0x4f),
        "Up" => Some(0x52),
        "Down" => Some(0x51),
        _ => None,
    }
}

fn keyboard_action_label(usage: u8, modifiers: u8) -> String {
    let mut parts = Vec::new();
    if modifiers & 0x01 != 0 {
        parts.push("Ctrl".to_owned());
    }
    if modifiers & 0x02 != 0 {
        parts.push("Shift".to_owned());
    }
    if modifiers & 0x04 != 0 {
        parts.push("Alt".to_owned());
    }
    if modifiers & 0x08 != 0 {
        parts.push("Win".to_owned());
    }
    parts.push(keyboard_usage_label(usage));
    parts.join(" + ")
}

fn keyboard_usage_label(usage: u8) -> String {
    match usage {
        0x04..=0x1d => char::from(b'A' + usage - 0x04).to_string(),
        0x1e..=0x26 => char::from(b'1' + usage - 0x1e).to_string(),
        0x27 => "0".to_owned(),
        0x2a => "Backspace".to_owned(),
        0x2b => "Tab".to_owned(),
        0x2c => "Space".to_owned(),
        0x2d => "-".to_owned(),
        0x2e => "=".to_owned(),
        0x2f => "[".to_owned(),
        0x30 => "]".to_owned(),
        0x31 => "\\".to_owned(),
        0x33 => ";".to_owned(),
        0x34 => "'".to_owned(),
        0x35 => "`".to_owned(),
        0x36 => ",".to_owned(),
        0x37 => ".".to_owned(),
        0x38 => "/".to_owned(),
        0x3a..=0x45 => format!("F{}", usage - 0x39),
        0x49 => "Insert".to_owned(),
        0x4a => "Home".to_owned(),
        0x4b => "PageUp".to_owned(),
        0x4c => "Delete".to_owned(),
        0x4d => "End".to_owned(),
        0x4e => "PageDown".to_owned(),
        0x4f => "Right".to_owned(),
        0x50 => "Left".to_owned(),
        0x51 => "Down".to_owned(),
        0x52 => "Up".to_owned(),
        _ => format!("HID 0x{usage:02X}"),
    }
}

fn selection_mode_label(mode: pulsehub_config_store::SelectionMode) -> &'static str {
    match mode {
        pulsehub_config_store::SelectionMode::Auto => "自动",
        pulsehub_config_store::SelectionMode::Office => "固定 Office",
        pulsehub_config_store::SelectionMode::Cs2 => "固定 CS2",
    }
}

fn selection_mode_from_label(label: &str) -> Result<pulsehub_config_store::SelectionMode, String> {
    match label {
        "自动" => Ok(pulsehub_config_store::SelectionMode::Auto),
        "固定 Office" => Ok(pulsehub_config_store::SelectionMode::Office),
        "固定 CS2" => Ok(pulsehub_config_store::SelectionMode::Cs2),
        _ => Err(format!("不支持的环境选择模式：{label}")),
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
fn apply_agent_now() -> Result<(AgentSnapshot, pulsehub_config_store::ConfigDocument), String> {
    use pulsehub_ipc::windows::{connect_with_retry, default_pipe_path};
    let path = pulsehub_config_store::default_config_path().map_err(|error| error.to_string())?;
    let config =
        pulsehub_config_store::load_or_create_default(&path).map_err(|error| error.to_string())?;
    let mut stream = connect_with_retry(
        default_pipe_path().map_err(|error| error.to_string())?,
        Duration::from_secs(5),
        Duration::from_millis(100),
    )
    .map_err(|error| error.to_string())?;
    negotiate(&mut stream)?;
    let response = exchange(
        &mut stream,
        &Request::ApplyNow {
            version: PROTOCOL_VERSION,
            request_id: "gui-apply-now".into(),
        },
    )?;
    let snapshot = response
        .data
        .and_then(|data| serde_json::from_value(data).ok())
        .ok_or_else(|| "代理返回的应用快照无效".to_owned())?;
    Ok((snapshot, config))
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
        AgentAction::ValidateCurrentConfig
            | AgentAction::CommitCurrentConfig
            | AgentAction::SetStartup(_)
    ) {
        let path = match pulsehub_config_store::default_config_path() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("配置路径错误：{error}");
                return ExitCode::FAILURE;
            }
        };
        let mut config = match pulsehub_config_store::load_or_create_default(&path) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("配置读取失败：{error}");
                return ExitCode::FAILURE;
            }
        };
        if let AgentAction::SetStartup(enabled) = action {
            config.agent.start_with_windows = enabled;
        }
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
    let base_revision = if matches!(
        action,
        AgentAction::CommitCurrentConfig | AgentAction::SetStartup(_)
    ) {
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
        AgentAction::SetSelectionMode(mode) => Request::SetSelectionMode {
            version: PROTOCOL_VERSION,
            request_id: "config-set-selection-1".to_owned(),
            mode,
        },
        AgentAction::SetStartup(_) => Request::CommitConfig {
            version: PROTOCOL_VERSION,
            request_id: "config-set-startup-1".to_owned(),
            base_revision,
            draft: draft.expect("登录启动操作必须加载草稿"),
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
            AgentAction::SetSelectionMode(_) => "环境模式更新完成",
            AgentAction::SetStartup(_) => "登录启动更新完成",
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
    println!("Windows 登录启动：{:?}", snapshot.integration_status);
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
    println!("  --set-selection-auto  将环境选择保存为自动");
    println!("  --set-selection-office  将环境选择保存为固定 Office");
    println!("  --set-selection-cs2  将环境选择保存为固定 CS2");
    println!("  --enable-startup  保存并创建当前用户登录启动项");
    println!("  --disable-startup  保存并删除当前用户登录启动项");
}

#[cfg(test)]
mod tests {
    use super::{
        apply_mapping_selections, captured_keyboard_action, is_revision_conflict,
        keyboard_usage_from_key, selection_mode_from_label, selection_mode_label,
        should_prompt_before_close, validate_custom_dpi,
    };

    #[test]
    fn recognizes_revision_conflict_without_matching_other_errors() {
        assert!(is_revision_conflict("\"PH-IPC-CONFLICT\"：配置修订冲突"));
        assert!(!is_revision_conflict("\"PH-IPC-BUSY\"：设备协调者响应超时"));
    }

    #[test]
    fn captures_single_keys_and_modifier_chords() {
        assert_eq!(
            captured_keyboard_action("a", true, false, false, false),
            Ok((0x04, 0x01, "Ctrl + A".to_owned()))
        );
        assert_eq!(
            captured_keyboard_action("Backspace", false, false, false, false),
            Ok((0x2a, 0, "Backspace".to_owned()))
        );
        assert_eq!(keyboard_usage_from_key("中"), None);
    }

    #[test]
    fn custom_dpi_respects_device_range_and_step() {
        assert_eq!(validate_custom_dpi(" 1250 ", 50, 8000, 50), Ok(1250));
        assert!(validate_custom_dpi("1255", 50, 8000, 50).is_err());
        assert!(validate_custom_dpi("9000", 50, 8000, 50).is_err());
        assert!(validate_custom_dpi("fast", 50, 8000, 50).is_err());
    }

    #[test]
    fn mapping_selection_updates_draft_and_keeps_primary_clicks_valid() {
        let mut config = pulsehub_config_store::ConfigDocument::default();
        apply_mapping_selections(
            &mut config.profiles.office.button_mappings,
            &[("g102:side_back".to_owned(), "key:6:1".to_owned())],
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

    #[test]
    fn selection_mode_labels_round_trip() {
        for mode in [
            pulsehub_config_store::SelectionMode::Auto,
            pulsehub_config_store::SelectionMode::Office,
            pulsehub_config_store::SelectionMode::Cs2,
        ] {
            assert_eq!(
                selection_mode_from_label(selection_mode_label(mode)),
                Ok(mode)
            );
        }
        assert!(selection_mode_from_label("未知").is_err());
    }
}
