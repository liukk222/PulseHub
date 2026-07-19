#![forbid(unsafe_code)]

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundProcess {
    pub executable_name: String,
    pub process_id: u64,
}

#[cfg(windows)]
pub fn current() -> Result<ForegroundProcess, String> {
    let window = active_win_pos_rs::get_active_window()
        .map_err(|()| "无法读取 Windows 前台窗口".to_owned())?;
    let executable_name = window
        .process_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "前台窗口没有可用的进程文件名".to_owned())?;
    Ok(ForegroundProcess {
        executable_name: executable_name.to_owned(),
        process_id: window.process_id,
    })
}

#[cfg(not(windows))]
pub fn current() -> Result<ForegroundProcess, String> {
    Err("前台窗口识别当前仅支持 Windows".to_owned())
}
