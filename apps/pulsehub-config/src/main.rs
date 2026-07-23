#![cfg_attr(windows, windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use std::env;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::path::{Path, PathBuf};
use std::process::ExitCode;
#[cfg(windows)]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(windows)]
use std::time::Duration;

use pulsehub_ipc::{AgentSnapshot, PROTOCOL_VERSION, Request, Response, read_frame, write_frame};
use pulsehub_ui::{AppTray, AppWindow, ApplicationProfileItem, MappingItem};
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
    Shutdown(bool),
}

fn main() -> ExitCode {
    match env::args().nth(1).as_deref() {
        Some("--set-ui-language") => set_ui_language(env::args().nth(2).as_deref()),
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
        Some("--shutdown-agent") => run_agent_action(AgentAction::Shutdown(false)),
        Some("--force-shutdown-agent") => run_agent_action(AgentAction::Shutdown(true)),
        Some("--tray-only") => run_tray_only(),
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

fn set_ui_language(value: Option<&str>) -> ExitCode {
    let language = match value {
        Some("zh_cn") => pulsehub_config_store::UiLanguage::ZhCn,
        Some("en") => pulsehub_config_store::UiLanguage::En,
        Some(value) => {
            eprintln!("不支持的界面语言：{value}（可选值：zh_cn、en）");
            return ExitCode::from(2);
        }
        None => {
            eprintln!("--set-ui-language 需要 zh_cn 或 en 参数");
            return ExitCode::from(2);
        }
    };
    let path = match pulsehub_config_store::default_config_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("无法确定配置路径：{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut document = match pulsehub_config_store::load_or_create_default(&path) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("无法读取配置：{error}");
            return ExitCode::FAILURE;
        }
    };
    document.agent.language = language;
    match pulsehub_config_store::save_atomic(&path, &document) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("无法保存界面语言：{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(windows)]
fn run_gui() -> ExitCode {
    let _single_instance = match single_instance::SingleInstance::new("Local\\PulseHub.Config.v1") {
        Ok(instance) if instance.is_single() => instance,
        Ok(_) => return ExitCode::SUCCESS,
        Err(_) => return ExitCode::FAILURE,
    };
    ensure_agent_running();
    let ui = match AppWindow::new() {
        Ok(ui) => ui,
        Err(error) => {
            eprintln!("PulseHub 窗口创建失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    // 系统托盘只由 pulsehub-agent.exe 承载，GUI 进程绝不创建托盘图标。
    let tray: slint::Weak<AppTray> = Default::default();

    let close_ui = ui.as_weak();
    let close_tray = tray.clone();
    ui.window().on_close_requested(move || {
        let Some(ui) = close_ui.upgrade() else {
            return slint::CloseRequestResponse::HideWindow;
        };
        if should_prompt_before_close(ui.get_draft_dirty()) {
            ui.set_quit_after_close(false);
            ui.set_close_dialog_visible(true);
            slint::CloseRequestResponse::KeepWindowShown
        } else {
            handoff_to_tray(&close_tray);
            slint::quit_event_loop().ok();
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
    let save_tray = tray.clone();
    ui.on_save_requested(
        move |office_dpi,
              cs2_dpi,
              base_revision,
              selection_mode,
              start_with_windows,
              developer_logging,
              language| {
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
            let shutdown_mappings = save_ui
                .upgrade()
                .map(|ui| mapping_selections(&ui.get_shutdown_mappings()))
                .unwrap_or_default();
            let office_dpi_levels = save_ui
                .upgrade()
                .map(|ui| dpi_levels_from_ui(&ui, true))
                .unwrap_or([800, 1600, 2400, 3200]);
            let cs2_dpi_levels = save_ui
                .upgrade()
                .map(|ui| dpi_levels_from_ui(&ui, false))
                .unwrap_or([800, 1600, 2400, 3200]);
            let shutdown_dpi = save_ui
                .upgrade()
                .map(|ui| ui.get_shutdown_dpi() as u16)
                .unwrap_or(1600);
            let shutdown_dpi_levels = save_ui
                .upgrade()
                .map(|ui| dpi_levels_from_kind(&ui, 3))
                .unwrap_or([800, 1600, 2400, 3200]);
            let office_report_rate = save_ui
                .upgrade()
                .map(|ui| ui.get_office_report_rate() as u16)
                .unwrap_or(1000);
            let cs2_report_rate = save_ui
                .upgrade()
                .map(|ui| ui.get_cs2_report_rate() as u16)
                .unwrap_or(1000);
            let shutdown_report_rate = save_ui
                .upgrade()
                .map(|ui| ui.get_shutdown_report_rate() as u16)
                .unwrap_or(1000);
            let applications = match save_ui
                .upgrade()
                .map(|ui| applications_from_ui(&ui))
                .transpose()
            {
                Ok(Some(applications)) => applications,
                Ok(None) => return,
                Err(error) => {
                    show_gui_error(&save_ui, "应用环境草稿无效", &error);
                    return;
                }
            };
            let worker_ui = save_ui.clone();
            let worker_tray = save_tray.clone();
            std::thread::spawn(move || {
                let result = save_gui_config(
                    office_dpi as u16,
                    cs2_dpi as u16,
                    shutdown_dpi,
                    base_revision as u64,
                    &office_mappings,
                    &cs2_mappings,
                    &shutdown_mappings,
                    office_dpi_levels,
                    cs2_dpi_levels,
                    shutdown_dpi_levels,
                    office_report_rate,
                    cs2_report_rate,
                    shutdown_report_rate,
                    applications,
                    selection_mode.as_str(),
                    start_with_windows,
                    developer_logging,
                    language.as_str(),
                );
                let _ = slint::invoke_from_event_loop(move || match result {
                    Ok((snapshot, config)) => {
                        if let Some(tray) = worker_tray.upgrade() {
                            tray.set_english(
                                config.agent.language == pulsehub_config_store::UiLanguage::En,
                            );
                        }
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
                                if ui.get_quit_after_close() {
                                    ui.set_quit_after_close(false);
                                    begin_safe_shutdown(
                                        worker_ui.clone(),
                                        worker_tray.clone(),
                                        false,
                                    );
                                } else {
                                    handoff_to_tray(&worker_tray);
                                    slint::quit_event_loop().ok();
                                }
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
    ui.on_open_project_requested(move || {
        let mut command = std::process::Command::new("explorer.exe");
        command.arg("https://github.com/liukk222/PulseHub");
        command.creation_flags(0x08000000);
        let _ = command.spawn();
    });
    let import_ui = ui.as_weak();
    ui.on_import_application_requested(move |path| {
        let path = if path.trim().is_empty() {
            rfd::FileDialog::new()
                .set_title("选择要导入的程序")
                .add_filter("Windows 程序", &["exe"])
                .pick_file()
                .map(|path| path.to_string_lossy().into_owned())
        } else {
            Some(path.to_string())
        };
        if let Some(path) = path {
            import_application(&import_ui, &path);
        }
    });
    let select_ui = ui.as_weak();
    ui.on_application_selected(move |index| select_application(&select_ui, index));

    let export_ui = ui.as_weak();
    ui.on_config_export_requested(move || export_configuration(&export_ui));
    let pending_import = Arc::new(std::sync::Mutex::new(None::<PathBuf>));
    let select_import_ui = ui.as_weak();
    let select_import_path = Arc::clone(&pending_import);
    ui.on_config_import_requested(move || {
        let Some(path) = rfd::FileDialog::new()
            .set_title("选择 PulseHub 配置文件")
            .add_filter("PulseHub 配置", &["toml"])
            .pick_file()
        else {
            return;
        };
        if let Ok(mut pending) = select_import_path.lock() {
            *pending = Some(path);
        }
        if let Some(window) = select_import_ui.upgrade() {
            window.set_config_import_dialog_visible(true);
        }
    });
    let confirm_import_ui = ui.as_weak();
    let confirm_import_path = Arc::clone(&pending_import);
    ui.on_config_import_confirmed(move || {
        let path = confirm_import_path
            .lock()
            .ok()
            .and_then(|mut pending| pending.take());
        if let Some(path) = path {
            import_configuration(&confirm_import_ui, path);
        }
    });
    let cancel_import_path = Arc::clone(&pending_import);
    ui.on_config_import_cancelled(move || {
        if let Ok(mut pending) = cancel_import_path.lock() {
            *pending = None;
        }
    });

    let discard_ui = ui.as_weak();
    ui.on_discard_requested(move || refresh_gui(discard_ui.clone()));
    let close_discard_ui = ui.as_weak();
    let close_discard_tray = tray.clone();
    ui.on_discard_and_close_requested(move || {
        if let Some(ui) = close_discard_ui.upgrade() {
            ui.set_draft_dirty(false);
            ui.set_close_after_save(false);
            if ui.get_quit_after_close() {
                ui.set_quit_after_close(false);
                begin_safe_shutdown(close_discard_ui.clone(), close_discard_tray.clone(), false);
            } else {
                handoff_to_tray(&close_discard_tray);
                slint::quit_event_loop().ok();
            }
        }
    });
    let retry_shutdown_ui = ui.as_weak();
    let retry_shutdown_tray = tray.clone();
    ui.on_retry_shutdown_requested(move || {
        begin_safe_shutdown(
            retry_shutdown_ui.clone(),
            retry_shutdown_tray.clone(),
            false,
        )
    });
    let force_shutdown_ui = ui.as_weak();
    let force_shutdown_tray = tray.clone();
    ui.on_force_shutdown_requested(move || {
        begin_safe_shutdown(force_shutdown_ui.clone(), force_shutdown_tray.clone(), true)
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
    let agent_was_connected = Arc::new(AtomicBool::new(false));
    let live_poll_flag = Arc::clone(&live_poll_running);
    let live_poll_ui = ui.as_weak();
    let live_poll_timer = Timer::default();
    let poll_connected = Arc::clone(&agent_was_connected);
    live_poll_timer.start(TimerMode::Repeated, Duration::from_millis(750), move || {
        if live_poll_flag.swap(true, Ordering::AcqRel) {
            return;
        }
        let worker_ui = live_poll_ui.clone();
        let worker_flag = Arc::clone(&live_poll_flag);
        let worker_connected = Arc::clone(&poll_connected);
        std::thread::spawn(move || {
            let snapshot = request_snapshot();
            let should_close = if agent_is_shutting_down() {
                true
            } else if snapshot.is_ok() {
                worker_connected.store(true, Ordering::Release);
                false
            } else {
                worker_connected.load(Ordering::Acquire) && agent_is_not_running()
            };
            let _ = slint::invoke_from_event_loop(move || {
                if should_close {
                    if let Some(window) = worker_ui.upgrade() {
                        let _ = window.hide();
                    }
                    slint::quit_event_loop().ok();
                    worker_flag.store(false, Ordering::Release);
                    return;
                }
                if let (Ok(snapshot), Some(window)) = (snapshot, worker_ui.upgrade()) {
                    update_overview_state(
                        &window,
                        &snapshot,
                        !window.get_draft_dirty() && !window.get_busy(),
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
fn agent_is_not_running() -> bool {
    single_instance::SingleInstance::new("Local\\PulseHub.Agent.v1")
        .is_ok_and(|instance| instance.is_single())
}

#[cfg(windows)]
fn agent_is_shutting_down() -> bool {
    std::env::temp_dir()
        .join("PulseHub-ShuttingDown-v1")
        .is_file()
}

#[cfg(windows)]
fn run_tray_only() -> ExitCode {
    ensure_agent_running();
    let tray = match AppTray::new() {
        Ok(tray) => tray,
        Err(_) => return ExitCode::FAILURE,
    };
    if let Ok(path) = pulsehub_config_store::default_config_path()
        && let Ok(config) = pulsehub_config_store::load_or_create_default(&path)
    {
        tray.set_english(config.agent.language == pulsehub_config_store::UiLanguage::En);
    }
    let open_tray = tray.as_weak();
    tray.on_open_requested(move || {
        spawn_config_process(&[]);
        if let Some(tray) = open_tray.upgrade() {
            let _ = tray.hide();
        }
        slint::quit_event_loop().ok();
    });
    let quit_tray = tray.as_weak();
    tray.on_quit_requested(move || {
        let _ = request_agent_shutdown(false);
        if let Some(tray) = quit_tray.upgrade() {
            let _ = tray.hide();
        }
        slint::quit_event_loop().ok();
    });
    if tray.show().is_err() {
        return ExitCode::FAILURE;
    }
    let trim_timer = Timer::default();
    trim_timer.start(TimerMode::SingleShot, Duration::from_secs(2), || {
        pulsehub_ui::trim_current_process_working_set();
    });
    match slint::run_event_loop() {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

#[cfg(windows)]
fn spawn_config_process(arguments: &[&str]) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    if let Ok(executable) = std::env::current_exe() {
        let _ = std::process::Command::new(executable)
            .args(arguments)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

#[cfg(windows)]
fn handoff_to_tray(tray: &slint::Weak<AppTray>) {
    if let Some(tray) = tray.upgrade() {
        let _ = tray.hide();
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
        window.set_connection_text("正在检测设备".into());
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
fn export_configuration(ui: &slint::Weak<AppWindow>) {
    let result = (|| -> Result<PathBuf, String> {
        let config_path =
            pulsehub_config_store::default_config_path().map_err(|error| error.to_string())?;
        let config = pulsehub_config_store::load_or_create_default(&config_path)
            .map_err(|error| error.to_string())?;
        let Some(path) = rfd::FileDialog::new()
            .set_title("导出 PulseHub 配置")
            .set_file_name("pulsehub-config-export.toml")
            .add_filter("PulseHub 配置", &["toml"])
            .save_file()
        else {
            return Err("已取消导出。".to_owned());
        };
        std::fs::write(
            &path,
            config
                .export_transfer()
                .to_toml()
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("无法写入 {}：{error}", path.display()))?;
        Ok(path)
    })();
    if let Some(window) = ui.upgrade() {
        match result {
            Ok(path) => {
                window.set_save_title("配置已导出".into());
                window.set_save_detail(
                    format!(
                        "已导出到 {}。未保存的窗口草稿不会包含在导出文件中。",
                        path.display()
                    )
                    .into(),
                );
            }
            Err(error) if error == "已取消导出。" => {}
            Err(error) => {
                window.set_save_title("配置导出失败".into());
                window.set_save_detail(error.into());
            }
        }
    }
}

#[cfg(windows)]
fn import_configuration(ui: &slint::Weak<AppWindow>, import_path: PathBuf) {
    let base_revision = ui
        .upgrade()
        .map(|window| window.get_base_revision() as u64)
        .unwrap_or(0);
    if let Some(window) = ui.upgrade() {
        window.set_busy(true);
        window.set_save_title("正在导入配置".into());
        window.set_save_detail("正在校验导入文件并提交给代理。".into());
    }
    let worker_ui = ui.clone();
    std::thread::spawn(move || {
        let result = import_gui_config(&import_path, base_revision);
        let _ = slint::invoke_from_event_loop(move || match result {
            Ok((snapshot, config)) => {
                if let Some(window) = worker_ui.upgrade() {
                    apply_gui_state(&window, &snapshot, &config);
                    window.set_draft_dirty(false);
                    window.set_busy(false);
                    window.set_save_title("配置已导入并应用".into());
                    window.set_save_detail("Office、CS2、退出配置、应用环境和切换规则已更新。自动模式仅按 EXE 文件名匹配。".into());
                }
            }
            Err(error) => show_gui_error(&worker_ui, "配置导入失败", &error),
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
    let english = config.agent.language == pulsehub_config_store::UiLanguage::En;
    ui.set_language(if english { "English" } else { "简体中文" }.into());
    ui.set_selection_mode(selection_mode_label_for_config(config, english).into());
    ui.set_connection_text(connection_label(snapshot.device_status, english).into());
    ui.set_device_status(
        match snapshot.device_status {
            pulsehub_ipc::DeviceStatus::Ready => {
                if english {
                    "Device connected"
                } else {
                    "设备已连接"
                }
            }
            pulsehub_ipc::DeviceStatus::Disconnected => {
                if english {
                    "Device disconnected"
                } else {
                    "未检测到设备"
                }
            }
            pulsehub_ipc::DeviceStatus::Busy => {
                if english {
                    "Device is busy"
                } else {
                    "设备正被其他程序占用"
                }
            }
            pulsehub_ipc::DeviceStatus::Degraded => {
                if english {
                    "Device status is incomplete"
                } else {
                    "设备状态不完整"
                }
            }
            pulsehub_ipc::DeviceStatus::Unknown => {
                if english {
                    "Reading device status"
                } else {
                    "正在读取设备状态"
                }
            }
        }
        .into(),
    );
    update_overview_state(ui, snapshot, true);
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
    ui.set_shutdown_dpi(config.shutdown_profile.dpi.into());
    ui.set_office_report_rate(config.profiles.office.report_rate_hz.into());
    ui.set_cs2_report_rate(config.profiles.cs2.report_rate_hz.into());
    ui.set_shutdown_report_rate(config.shutdown_profile.report_rate_hz.into());
    set_dpi_levels_on_ui(ui, true, &config.profiles.office.dpi_levels);
    set_dpi_levels_on_ui(ui, false, &config.profiles.cs2.dpi_levels);
    let shutdown_levels = <[u16; 4]>::try_from(config.shutdown_profile.dpi_levels.as_slice())
        .unwrap_or([800, 1600, 2400, 3200]);
    set_dpi_levels_i32_for_kind(ui, 3, shutdown_levels.map(i32::from));
    ui.set_office_mappings(mapping_model(
        &config.profiles.office.button_mappings,
        english,
    ));
    ui.set_cs2_mappings(mapping_model(&config.profiles.cs2.button_mappings, english));
    ui.set_shutdown_mappings(mapping_model(
        &config.shutdown_profile.button_mappings,
        english,
    ));
    ui.set_office_dpi_cycle_enabled(profile_uses_dpi_cycle(&config.profiles.office));
    ui.set_cs2_dpi_cycle_enabled(profile_uses_dpi_cycle(&config.profiles.cs2));
    ui.set_shutdown_dpi_cycle_enabled(profile_uses_dpi_cycle(&config.shutdown_profile));
    set_applications_on_ui(ui, &config.applications);
    ui.set_start_with_windows(config.agent.start_with_windows);
    ui.set_developer_logging(config.agent.developer_logging);
    ui.set_integration_status(
        match snapshot.integration_status {
            pulsehub_ipc::IntegrationStatus::Unknown => {
                if english {
                    "Unknown"
                } else {
                    "未知"
                }
            }
            pulsehub_ipc::IntegrationStatus::Synced => {
                if english {
                    "Synced"
                } else {
                    "已同步"
                }
            }
            pulsehub_ipc::IntegrationStatus::Failed => {
                if english {
                    "Sync failed"
                } else {
                    "同步失败"
                }
            }
        }
        .into(),
    );
}

#[cfg(windows)]
fn update_overview_state(ui: &AppWindow, snapshot: &AgentSnapshot, update_banner: bool) {
    let english = ui.get_language().as_str() == "English";
    ui.set_connection_text(connection_label(snapshot.device_status, english).into());
    let environment = snapshot
        .active_profile_name
        .as_deref()
        .map(|name| localize_profile_name(name, english))
        .unwrap_or_else(|| match snapshot.active_environment {
            pulsehub_ipc::Environment::Office => {
                if english {
                    "Office".to_owned()
                } else {
                    "办公环境".to_owned()
                }
            }
            pulsehub_ipc::Environment::Cs2 => {
                if english {
                    "CS2".to_owned()
                } else {
                    "CS 环境".to_owned()
                }
            }
            pulsehub_ipc::Environment::Custom => {
                if english {
                    "App profile".to_owned()
                } else {
                    "应用环境".to_owned()
                }
            }
        });
    ui.set_environment_name(environment.clone().into());
    ui.set_current_dpi(
        snapshot
            .current_dpi
            .map_or_else(|| "—".to_owned(), |dpi| dpi.to_string())
            .into(),
    );
    ui.set_desired_dpi(snapshot.desired_dpi.to_string().into());
    if !update_banner {
        return;
    }
    let mode = ui.get_selection_mode();
    if snapshot.current_dpi == Some(snapshot.desired_dpi) {
        ui.set_save_title(
            if english {
                format!("Current mode: {mode}")
            } else {
                format!("当前模式：{mode}")
            }
            .into(),
        );
        ui.set_save_detail(
            if english {
                format!(
                    "Active profile: {environment}; applied. Current DPI: {}.",
                    snapshot.desired_dpi
                )
            } else {
                format!(
                    "当前环境：{environment}；配置已应用，当前 DPI 为 {}。",
                    snapshot.desired_dpi
                )
            }
            .into(),
        );
    } else {
        ui.set_save_title(
            if english {
                format!("Current mode: {mode} (waiting to apply)")
            } else {
                format!("当前模式：{mode}（等待应用）")
            }
            .into(),
        );
        let current = snapshot.current_dpi.map_or_else(
            || {
                if english {
                    "Unknown".to_owned()
                } else {
                    "未知".to_owned()
                }
            },
            |dpi| dpi.to_string(),
        );
        ui.set_save_detail(if english {
            format!("Active profile: {environment}; saved. Device DPI: {current}; target DPI: {}.", snapshot.desired_dpi)
        } else {
            format!("当前环境：{environment}；配置已保存，设备当前 DPI 为 {current}，目标 DPI 为 {}。", snapshot.desired_dpi)
        }.into());
    }
}

fn localize_profile_name(name: &str, english: bool) -> String {
    if !english {
        return name.to_owned();
    }
    match name {
        "办公环境" => "Office".to_owned(),
        "CS 环境" | "CS2 环境" => "CS2".to_owned(),
        "应用环境" => "App profile".to_owned(),
        _ => name.strip_suffix(" 环境").unwrap_or(name).to_owned(),
    }
}

fn connection_label(status: pulsehub_ipc::DeviceStatus, english: bool) -> &'static str {
    match status {
        pulsehub_ipc::DeviceStatus::Ready => {
            if english {
                "Device connected"
            } else {
                "设备已连接"
            }
        }
        pulsehub_ipc::DeviceStatus::Disconnected => {
            if english {
                "Device disconnected"
            } else {
                "设备已断开"
            }
        }
        pulsehub_ipc::DeviceStatus::Busy => {
            if english {
                "Device busy"
            } else {
                "设备忙"
            }
        }
        pulsehub_ipc::DeviceStatus::Degraded => {
            if english {
                "Device status error"
            } else {
                "设备状态异常"
            }
        }
        pulsehub_ipc::DeviceStatus::Unknown => {
            if english {
                "Detecting device"
            } else {
                "正在检测设备"
            }
        }
    }
}

#[cfg(windows)]
fn ensure_agent_running() {
    if request_snapshot().is_ok() {
        return;
    }
    let Ok(config_executable) = std::env::current_exe() else {
        return;
    };
    let Some(directory) = config_executable.parent() else {
        return;
    };
    let agent = directory.join("pulsehub-agent.exe");
    if !agent.is_file() {
        return;
    }
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new(agent)
        .args(["--run-agent", "--confirm-device-write"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    std::thread::sleep(Duration::from_millis(600));
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
    shutdown_dpi: u16,
    base_revision: u64,
    office_mappings: &[(String, String)],
    cs2_mappings: &[(String, String)],
    shutdown_mappings: &[(String, String)],
    office_dpi_levels: [u16; 4],
    cs2_dpi_levels: [u16; 4],
    shutdown_dpi_levels: [u16; 4],
    office_report_rate: u16,
    cs2_report_rate: u16,
    shutdown_report_rate: u16,
    applications: Vec<pulsehub_config_store::ApplicationProfileConfig>,
    selection_mode: &str,
    start_with_windows: bool,
    developer_logging: bool,
    language: &str,
) -> Result<(AgentSnapshot, pulsehub_config_store::ConfigDocument), String> {
    use pulsehub_ipc::windows::{connect_with_retry, default_pipe_path};

    let path = pulsehub_config_store::default_config_path().map_err(|error| error.to_string())?;
    let mut config =
        pulsehub_config_store::load_or_create_default(&path).map_err(|error| error.to_string())?;
    config.profiles.office.dpi = office_dpi;
    config.profiles.cs2.dpi = cs2_dpi;
    config.shutdown_profile.dpi = shutdown_dpi;
    config.profiles.office.report_rate_hz = office_report_rate;
    config.profiles.cs2.report_rate_hz = cs2_report_rate;
    config.shutdown_profile.report_rate_hz = shutdown_report_rate;
    config.profiles.office.dpi_levels = office_dpi_levels.to_vec();
    config.profiles.cs2.dpi_levels = cs2_dpi_levels.to_vec();
    config.shutdown_profile.dpi_levels = shutdown_dpi_levels.to_vec();
    config.applications = applications;
    apply_mapping_selections(&mut config.profiles.office.button_mappings, office_mappings)?;
    apply_mapping_selections(&mut config.profiles.cs2.button_mappings, cs2_mappings)?;
    apply_mapping_selections(
        &mut config.shutdown_profile.button_mappings,
        shutdown_mappings,
    )?;
    let (mode, fixed_application_id) =
        selection_mode_from_gui(selection_mode, &config.applications)?;
    config.selection.mode = mode;
    config.selection.fixed_application_id = fixed_application_id;
    config.agent.start_with_windows = start_with_windows;
    config.agent.developer_logging = developer_logging;
    config.agent.language = match language {
        "简体中文" => pulsehub_config_store::UiLanguage::ZhCn,
        "English" => pulsehub_config_store::UiLanguage::En,
        _ => return Err(format!("不支持的界面语言：{language}")),
    };
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
fn import_gui_config(
    import_path: &Path,
    base_revision: u64,
) -> Result<(AgentSnapshot, pulsehub_config_store::ConfigDocument), String> {
    use pulsehub_ipc::windows::{connect_with_retry, default_pipe_path};

    let text = std::fs::read_to_string(import_path)
        .map_err(|error| format!("无法读取 {}：{error}", import_path.display()))?;
    let transfer = pulsehub_config_store::ConfigTransfer::from_toml(import_path, &text)
        .map_err(|error| error.to_string())?;
    let config_path =
        pulsehub_config_store::default_config_path().map_err(|error| error.to_string())?;
    let mut config = pulsehub_config_store::load_or_create_default(&config_path)
        .map_err(|error| error.to_string())?;
    config
        .apply_transfer(transfer)
        .map_err(|error| error.to_string())?;
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
            request_id: "gui-import-validate".into(),
            draft: draft.clone(),
        },
    )?;
    let response = exchange(
        &mut stream,
        &Request::CommitConfig {
            version: PROTOCOL_VERSION,
            request_id: "gui-import-commit".into(),
            base_revision,
            draft,
        },
    )?;
    let snapshot = response
        .data
        .and_then(|data| serde_json::from_value(data).ok())
        .ok_or_else(|| "代理返回的导入快照无效".to_owned())?;
    Ok((snapshot, config))
}

#[cfg(windows)]
fn mapping_model(
    mappings: &[pulsehub_config_store::ButtonMappingConfig],
    english: bool,
) -> ModelRc<MappingItem> {
    ModelRc::new(VecModel::from(
        mappings
            .iter()
            .map(|mapping| mapping_item(mapping, english))
            .collect::<Vec<_>>(),
    ))
}

#[cfg(windows)]
fn mapping_item(
    mapping: &pulsehub_config_store::ButtonMappingConfig,
    english: bool,
) -> MappingItem {
    let (action_id, action_label, known) = action_identity(&mapping.action, english);
    let protected = matches!(
        mapping.physical_control.as_str(),
        "g102:left" | "g102:right"
    );
    MappingItem {
        control_id: mapping.physical_control.clone().into(),
        label: control_label(&mapping.physical_control, english).into(),
        action_id: action_id.into(),
        action_label: action_label.into(),
        locked: protected || !known,
    }
}

#[cfg(windows)]
fn action_identity(
    action: &pulsehub_config_store::ButtonActionConfig,
    english: bool,
) -> (String, String, bool) {
    use pulsehub_config_store::ButtonActionConfig;
    match action {
        ButtonActionConfig::LogicalControl { value } if value == "mouse:left" => (
            "left".into(),
            if english {
                "Primary click"
            } else {
                "左键点击"
            }
            .into(),
            true,
        ),
        ButtonActionConfig::LogicalControl { value } if value == "mouse:right" => (
            "right".into(),
            if english {
                "Secondary click"
            } else {
                "右键点击"
            }
            .into(),
            true,
        ),
        ButtonActionConfig::LogicalControl { value } if value == "mouse:middle" => (
            "middle".into(),
            if english {
                "Middle click"
            } else {
                "鼠标中键"
            }
            .into(),
            true,
        ),
        ButtonActionConfig::LogicalControl { value } if value == "mouse:back" => (
            "side_back".into(),
            if english {
                "Mouse back"
            } else {
                "鼠标侧键（后）"
            }
            .into(),
            true,
        ),
        ButtonActionConfig::LogicalControl { value } if value == "mouse:forward" => (
            "side_forward".into(),
            if english {
                "Mouse forward"
            } else {
                "鼠标侧键（前）"
            }
            .into(),
            true,
        ),
        ButtonActionConfig::LogicalControl { value } if value == "mouse:dpi_cycle" => (
            "dpi_cycle".into(),
            if english {
                "Native DPI cycle"
            } else {
                "原本 DPI 切换"
            }
            .into(),
            true,
        ),
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
        ButtonActionConfig::Disabled => (
            "disabled".into(),
            if english { "Disabled" } else { "禁用" }.into(),
            true,
        ),
        _ => (
            "custom".into(),
            if english {
                "Configured action"
            } else {
                "已配置动作"
            }
            .into(),
            false,
        ),
    }
}

#[cfg(windows)]
fn control_label(control: &str, english: bool) -> &'static str {
    match (control, english) {
        ("g102:left", true) => "Primary button",
        ("g102:right", true) => "Secondary button",
        ("g102:middle", true) => "Wheel button (middle)",
        ("g102:side_back", true) => "Back button (G4)",
        ("g102:side_forward", true) => "Forward button (G5)",
        ("g102:dpi", true) => "DPI button (G6)",
        (_, true) => "Other button",
        ("g102:left", false) => "左键",
        ("g102:right", false) => "右键",
        ("g102:middle", false) => "滚轮键（中键）",
        ("g102:side_back", false) => "侧后键（G4）",
        ("g102:side_forward", false) => "侧前键（G5）",
        ("g102:dpi", false) => "DPI 键（G6）",
        (_, false) => "其他按键",
    }
}

#[cfg(windows)]
fn restore_mapping(ui: &slint::Weak<AppWindow>, kind: i32, index: i32) {
    let Some(ui) = ui.upgrade() else { return };
    let model = mappings_for_kind(&ui, kind);
    let Ok(index) = usize::try_from(index) else {
        return;
    };
    let Some(row) = model.row_data(index) else {
        return;
    };
    let (id, label) = original_action(
        row.control_id.as_str(),
        ui.get_language().as_str() == "English",
    );
    update_mapping_row(&ui, &model, kind, index, id, label);
}

#[cfg(windows)]
fn set_custom_dpi(ui: &slint::Weak<AppWindow>, kind: i32, text: &str) {
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
    if kind == 0 {
        ui.set_office_dpi(value);
    } else if kind == 1 {
        ui.set_cs2_dpi(value);
    } else if kind == 2 {
        ui.set_custom_dpi(value);
    } else {
        ui.set_shutdown_dpi(value);
    }
    ui.set_draft_dirty(true);
    ui.set_save_title("自定义 DPI 已加入草稿".into());
    ui.set_save_detail(format!("{value} DPI 将在保存配置后生效。").into());
}

#[cfg(windows)]
fn set_dpi_level(ui: &slint::Weak<AppWindow>, kind: i32, index: i32, text: &str) {
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
    let mut levels = dpi_levels_from_kind(&ui, kind).map(i32::from);
    levels[index] = value;
    if !levels.windows(2).all(|pair| pair[0] < pair[1]) {
        ui.set_save_title("DPI 档位顺序无效".into());
        ui.set_save_detail("四个 DPI 档位必须从低到高排列且不能重复。".into());
        return;
    }
    set_dpi_levels_i32_for_kind(&ui, kind, levels);
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
fn mappings_for_kind(ui: &AppWindow, kind: i32) -> ModelRc<MappingItem> {
    match kind {
        0 => ui.get_office_mappings(),
        1 => ui.get_cs2_mappings(),
        2 => ui.get_custom_mappings(),
        _ => ui.get_shutdown_mappings(),
    }
}

#[cfg(windows)]
fn dpi_levels_from_kind(ui: &AppWindow, kind: i32) -> [u16; 4] {
    if kind < 2 {
        return dpi_levels_from_ui(ui, kind == 0);
    }
    if kind == 3 {
        return [
            ui.get_shutdown_dpi_level_1(),
            ui.get_shutdown_dpi_level_2(),
            ui.get_shutdown_dpi_level_3(),
            ui.get_shutdown_dpi_level_4(),
        ]
        .map(|value| u16::try_from(value).unwrap_or_default());
    }
    [
        ui.get_custom_dpi_level_1(),
        ui.get_custom_dpi_level_2(),
        ui.get_custom_dpi_level_3(),
        ui.get_custom_dpi_level_4(),
    ]
    .map(|value| u16::try_from(value).unwrap_or_default())
}

#[cfg(windows)]
fn set_dpi_levels_i32_for_kind(ui: &AppWindow, kind: i32, levels: [i32; 4]) {
    if kind < 2 {
        set_dpi_levels_i32_on_ui(ui, kind == 0, levels);
    } else if kind == 2 {
        ui.set_custom_dpi_level_1(levels[0]);
        ui.set_custom_dpi_level_2(levels[1]);
        ui.set_custom_dpi_level_3(levels[2]);
        ui.set_custom_dpi_level_4(levels[3]);
    } else {
        ui.set_shutdown_dpi_level_1(levels[0]);
        ui.set_shutdown_dpi_level_2(levels[1]);
        ui.set_shutdown_dpi_level_3(levels[2]);
        ui.set_shutdown_dpi_level_4(levels[3]);
    }
}

#[cfg(windows)]
fn set_applications_on_ui(
    ui: &AppWindow,
    applications: &[pulsehub_config_store::ApplicationProfileConfig],
) {
    set_mode_options_on_ui(ui, applications);
    ui.set_application_profiles_json(
        serde_json::to_string(applications)
            .unwrap_or_else(|_| "[]".to_owned())
            .into(),
    );
    ui.set_application_profile_items(ModelRc::new(VecModel::from(
        applications
            .iter()
            .map(|application| ApplicationProfileItem {
                name: localize_profile_name(
                    &application.name,
                    ui.get_language().as_str() == "English",
                )
                .into(),
                process_name: application.process_name.clone().into(),
                executable_path: application.executable_path.clone().into(),
            })
            .collect::<Vec<_>>(),
    )));
    let selected = if applications.is_empty() { -1 } else { 0 };
    ui.set_selected_application_index(selected);
    if selected >= 0 {
        load_application_editor(ui, &applications[0]);
    } else {
        ui.set_custom_mappings(ModelRc::new(VecModel::from(Vec::<MappingItem>::new())));
    }
}

#[cfg(windows)]
fn load_application_editor(
    ui: &AppWindow,
    application: &pulsehub_config_store::ApplicationProfileConfig,
) {
    ui.set_custom_profile_name(
        localize_profile_name(&application.name, ui.get_language().as_str() == "English").into(),
    );
    ui.set_custom_process_rule(application.process_name.clone().into());
    ui.set_custom_dpi(application.profile.dpi.into());
    ui.set_custom_report_rate(application.profile.report_rate_hz.into());
    if let Ok(levels) = <[u16; 4]>::try_from(application.profile.dpi_levels.as_slice()) {
        set_dpi_levels_i32_for_kind(ui, 2, levels.map(i32::from));
    }
    ui.set_custom_mappings(mapping_model(
        &application.profile.button_mappings,
        ui.get_language().as_str() == "English",
    ));
    ui.set_custom_dpi_cycle_enabled(profile_uses_dpi_cycle(&application.profile));
}

#[cfg(windows)]
fn applications_from_ui(
    ui: &AppWindow,
) -> Result<Vec<pulsehub_config_store::ApplicationProfileConfig>, String> {
    let mut applications: Vec<pulsehub_config_store::ApplicationProfileConfig> =
        serde_json::from_str(ui.get_application_profiles_json().as_str())
            .map_err(|error| format!("应用环境草稿无效：{error}"))?;
    if let Ok(index) = usize::try_from(ui.get_selected_application_index())
        && let Some(application) = applications.get_mut(index)
    {
        application.profile.dpi =
            u16::try_from(ui.get_custom_dpi()).map_err(|_| "应用环境 DPI 超出范围".to_owned())?;
        application.profile.dpi_levels = dpi_levels_from_kind(ui, 2).to_vec();
        application.profile.report_rate_hz = u16::try_from(ui.get_custom_report_rate())
            .map_err(|_| "应用环境回报率超出范围".to_owned())?;
        let selections = mapping_selections(&ui.get_custom_mappings());
        apply_mapping_selections(&mut application.profile.button_mappings, &selections)?;
    }
    Ok(applications)
}

#[cfg(windows)]
fn update_application_draft(
    ui: &AppWindow,
    applications: &[pulsehub_config_store::ApplicationProfileConfig],
) {
    set_mode_options_on_ui(ui, applications);
    ui.set_application_profiles_json(
        serde_json::to_string(applications)
            .unwrap_or_else(|_| "[]".to_owned())
            .into(),
    );
    ui.set_application_profile_items(ModelRc::new(VecModel::from(
        applications
            .iter()
            .map(|application| ApplicationProfileItem {
                name: localize_profile_name(
                    &application.name,
                    ui.get_language().as_str() == "English",
                )
                .into(),
                process_name: application.process_name.clone().into(),
                executable_path: application.executable_path.clone().into(),
            })
            .collect::<Vec<_>>(),
    )));
}

#[cfg(windows)]
fn set_mode_options_on_ui(
    ui: &AppWindow,
    applications: &[pulsehub_config_store::ApplicationProfileConfig],
) {
    let english = ui.get_language().as_str() == "English";
    let mut options = if english {
        vec![
            "Auto mode".to_owned(),
            "Fixed Office".to_owned(),
            "Fixed CS2".to_owned(),
        ]
    } else {
        vec![
            "自动模式".to_owned(),
            "固定 Office".to_owned(),
            "固定 CS2".to_owned(),
        ]
    };
    options.extend(applications.iter().map(|application| {
        if english {
            format!("Fixed {}", localize_profile_name(&application.name, true))
        } else {
            format!("固定 {}", application.name)
        }
    }));
    ui.set_mode_options(ModelRc::new(VecModel::from(
        options
            .into_iter()
            .map(Into::into)
            .collect::<Vec<slint::SharedString>>(),
    )));
}

#[cfg(windows)]
fn select_application(ui: &slint::Weak<AppWindow>, index: i32) {
    let Some(ui) = ui.upgrade() else { return };
    let Ok(applications) = applications_from_ui(&ui) else {
        return;
    };
    let Ok(index_usize) = usize::try_from(index) else {
        return;
    };
    let Some(application) = applications.get(index_usize) else {
        return;
    };
    update_application_draft(&ui, &applications);
    ui.set_selected_application_index(index);
    load_application_editor(&ui, application);
}

#[cfg(windows)]
fn import_application(ui: &slint::Weak<AppWindow>, path: &str) {
    use std::path::Path;
    let Some(ui) = ui.upgrade() else { return };
    let input = Path::new(path.trim());
    if !input.is_file()
        || !input
            .extension()
            .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
    {
        ui.set_save_title("无法导入程序".into());
        ui.set_save_detail("请输入存在的 .exe 程序完整路径。".into());
        return;
    }
    let process_name = input
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_owned();
    let executable_stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("应用")
        .to_owned();
    let display_name = if executable_stem.eq_ignore_ascii_case("winword") {
        "Word 环境".to_owned()
    } else {
        format!("{executable_stem} 环境")
    };
    let mut applications = match applications_from_ui(&ui) {
        Ok(value) => value,
        Err(error) => {
            ui.set_save_title("无法导入程序".into());
            ui.set_save_detail(error.into());
            return;
        }
    };
    if applications
        .iter()
        .any(|value| value.process_name.eq_ignore_ascii_case(&process_name))
    {
        ui.set_save_title("程序已导入".into());
        ui.set_save_detail(format!("{process_name} 已经有独立环境配置。 ").into());
        return;
    }
    let base_id = executable_stem
        .to_ascii_lowercase()
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() {
                value
            } else {
                '_'
            }
        })
        .collect::<String>();
    let mut id = base_id.clone();
    let mut suffix = 2;
    while applications.iter().any(|value| value.id == id) {
        id = format!("{base_id}_{suffix}");
        suffix += 1;
    }
    let mut profile = pulsehub_config_store::ConfigDocument::default()
        .profiles
        .cs2;
    profile.dpi = u16::try_from(ui.get_cs2_dpi()).unwrap_or(800);
    profile.dpi_levels = dpi_levels_from_ui(&ui, false).to_vec();
    let _ = apply_mapping_selections(
        &mut profile.button_mappings,
        &mapping_selections(&ui.get_cs2_mappings()),
    );
    applications.push(pulsehub_config_store::ApplicationProfileConfig {
        id,
        name: display_name,
        executable_path: input.to_string_lossy().into_owned(),
        process_name,
        profile,
    });
    let index = applications.len() - 1;
    update_application_draft(&ui, &applications);
    ui.set_selected_application_index(index as i32);
    load_application_editor(&ui, &applications[index]);
    ui.set_import_executable_path("".into());
    ui.set_draft_dirty(true);
    ui.set_save_title("应用环境已加入草稿".into());
    ui.set_save_detail("可继续调整独立配置，保存后自动模式即可识别该程序。".into());
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
    kind: i32,
    index: i32,
    key: &str,
    control: bool,
    shift: bool,
    alt: bool,
    meta: bool,
) {
    let Some(ui) = ui.upgrade() else { return };
    let model = mappings_for_kind(&ui, kind);
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
                kind,
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
    kind: i32,
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
        if kind == 0 {
            ui.set_office_dpi_cycle_enabled(enabled);
        } else if kind == 1 {
            ui.set_cs2_dpi_cycle_enabled(enabled);
        } else if kind == 2 {
            ui.set_custom_dpi_cycle_enabled(enabled);
        } else {
            ui.set_shutdown_dpi_cycle_enabled(enabled);
        }
    }
    ui.set_draft_dirty(true);
}

fn original_action(control: &str, english: bool) -> (&'static str, &'static str) {
    match (control, english) {
        ("g102:middle", true) => ("middle", "Middle click"),
        ("g102:side_back", true) => ("side_back", "Mouse back"),
        ("g102:side_forward", true) => ("side_forward", "Mouse forward"),
        ("g102:dpi", true) => ("dpi_cycle", "Native DPI cycle"),
        (_, true) => ("disabled", "Disabled"),
        ("g102:middle", false) => ("middle", "鼠标中键"),
        ("g102:side_back", false) => ("side_back", "鼠标侧键（后）"),
        ("g102:side_forward", false) => ("side_forward", "鼠标侧键（前）"),
        ("g102:dpi", false) => ("dpi_cycle", "原本 DPI 切换"),
        (_, false) => ("disabled", "禁用"),
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
        pulsehub_config_store::SelectionMode::Auto => "自动模式",
        pulsehub_config_store::SelectionMode::Office => "固定 Office",
        pulsehub_config_store::SelectionMode::Cs2 => "固定 CS2",
        pulsehub_config_store::SelectionMode::Application => "固定应用环境",
    }
}

fn selection_mode_label_for_config(
    config: &pulsehub_config_store::ConfigDocument,
    english: bool,
) -> String {
    if config.selection.mode != pulsehub_config_store::SelectionMode::Application {
        return if english {
            match config.selection.mode {
                pulsehub_config_store::SelectionMode::Auto => "Auto mode",
                pulsehub_config_store::SelectionMode::Office => "Fixed Office",
                pulsehub_config_store::SelectionMode::Cs2 => "Fixed CS2",
                pulsehub_config_store::SelectionMode::Application => "Fixed app profile",
            }
        } else {
            selection_mode_label(config.selection.mode)
        }
        .to_owned();
    }
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
        .map(|application| {
            if english {
                format!("Fixed {}", localize_profile_name(&application.name, true))
            } else {
                format!("固定 {}", application.name)
            }
        })
        .unwrap_or_else(|| {
            if english {
                "Fixed app profile".to_owned()
            } else {
                "固定应用环境".to_owned()
            }
        })
}

fn selection_mode_from_label(label: &str) -> Result<pulsehub_config_store::SelectionMode, String> {
    match label {
        "自动模式" | "Auto mode" => Ok(pulsehub_config_store::SelectionMode::Auto),
        "固定 Office" | "Fixed Office" => Ok(pulsehub_config_store::SelectionMode::Office),
        "固定 CS2" | "Fixed CS2" => Ok(pulsehub_config_store::SelectionMode::Cs2),
        _ => Err(format!("不支持的环境选择模式：{label}")),
    }
}

fn selection_mode_from_gui(
    label: &str,
    applications: &[pulsehub_config_store::ApplicationProfileConfig],
) -> Result<(pulsehub_config_store::SelectionMode, Option<String>), String> {
    if let Ok(mode) = selection_mode_from_label(label) {
        return Ok((mode, None));
    }
    let application = applications
        .iter()
        .find(|application| {
            label == format!("固定 {}", application.name)
                || label == format!("Fixed {}", application.name)
        })
        .ok_or_else(|| format!("不支持的模式：{label}"))?;
    Ok((
        pulsehub_config_store::SelectionMode::Application,
        Some(application.id.clone()),
    ))
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
fn begin_safe_shutdown(ui: slint::Weak<AppWindow>, tray: slint::Weak<AppTray>, force: bool) {
    if let Some(ui) = ui.upgrade() {
        ui.set_busy(true);
        ui.set_shutdown_dialog_visible(true);
        ui.set_shutdown_title(
            if force {
                "正在退出 PulseHub"
            } else {
                "正在安全还原鼠标"
            }
            .into(),
        );
        ui.set_shutdown_detail(
            if force {
                "正在请求代理立即退出。"
            } else {
                "代理正在设置 DPI 1600，并将六个按键恢复为原生功能；完成回读后自动退出。"
            }
            .into(),
        );
        let _ = ui.show();
    }
    std::thread::spawn(move || {
        let result = request_agent_shutdown(force);
        let _ = slint::invoke_from_event_loop(move || {
            if force || result.is_ok() {
                if let Some(tray) = tray.upgrade() {
                    let _ = tray.hide();
                }
                let _ = slint::quit_event_loop();
                return;
            }
            if let Some(ui) = ui.upgrade() {
                ui.set_busy(false);
                ui.set_shutdown_title("鼠标安全还原失败".into());
                ui.set_shutdown_detail(
                    format!(
                        "{}\n可以重试恢复，或确认仍然退出 PulseHub。",
                        result.expect_err("失败分支必须包含错误")
                    )
                    .into(),
                );
                let _ = ui.show();
            }
        });
    });
}

#[cfg(windows)]
fn request_agent_shutdown(force: bool) -> Result<serde_json::Value, String> {
    use pulsehub_ipc::windows::{connect_with_retry, default_pipe_path};
    let mut stream = connect_with_retry(
        default_pipe_path().map_err(|error| error.to_string())?,
        Duration::from_secs(5),
        Duration::from_millis(100),
    )
    .map_err(|error| error.to_string())?;
    negotiate(&mut stream)?;
    exchange(
        &mut stream,
        &Request::Shutdown {
            version: PROTOCOL_VERSION,
            request_id: if force {
                "gui-force-shutdown"
            } else {
                "gui-safe-shutdown"
            }
            .into(),
            force,
        },
    )?
    .data
    .ok_or_else(|| "代理没有返回退出结果".to_owned())
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
        AgentAction::Shutdown(force) => Request::Shutdown {
            version: PROTOCOL_VERSION,
            request_id: if force {
                "config-force-shutdown-1"
            } else {
                "config-safe-shutdown-1"
            }
            .to_owned(),
            force,
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
    if matches!(action, AgentAction::Shutdown(_)) {
        println!("代理退出请求完成：{data}");
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
    println!("  --set-ui-language <zh_cn|en>  设置 PulseHub 界面语言");
    println!("  --shutdown-agent  还原 DPI 1600 和六个原生按键后安全退出代理");
    println!("  --force-shutdown-agent  跳过鼠标还原并立即退出代理");
}

#[cfg(test)]
mod tests {
    use super::{
        apply_mapping_selections, captured_keyboard_action, is_revision_conflict,
        keyboard_usage_from_key, selection_mode_from_gui, selection_mode_from_label,
        selection_mode_label, should_prompt_before_close, validate_custom_dpi,
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
        let mut config = pulsehub_config_store::ConfigDocument::default();
        config
            .applications
            .push(pulsehub_config_store::ApplicationProfileConfig {
                id: "winword".to_owned(),
                name: "Word 环境".to_owned(),
                executable_path: r"C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE"
                    .to_owned(),
                process_name: "WINWORD.EXE".to_owned(),
                profile: config.profiles.cs2.clone(),
            });
        assert_eq!(
            selection_mode_from_gui("固定 Word 环境", &config.applications),
            Ok((
                pulsehub_config_store::SelectionMode::Application,
                Some("winword".to_owned())
            ))
        );
    }
}
