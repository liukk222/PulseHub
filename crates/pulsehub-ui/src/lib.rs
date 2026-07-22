//! PulseHub 的 Slint 生成代码边界。
//!
//! Slint 生成代码内部包含框架维护的 unsafe 实现，因此仅此 crate 放宽 unsafe lint；
//! `pulsehub-config` 及所有领域、IPC 和设备 crate 仍保持 `forbid(unsafe_code)`。

#![allow(unsafe_code)]

slint::include_modules!();

/// 请求 Windows 回收当前进程不再活跃的工作集页面。
///
/// 托盘态不持有 GUI 窗口；调用后只影响物理工作集，后续需要的页面仍可按需调入。
#[cfg(windows)]
pub fn trim_current_process_working_set() {
    unsafe {
        let process = windows_sys::Win32::System::Threading::GetCurrentProcess();
        windows_sys::Win32::System::ProcessStatus::EmptyWorkingSet(process);
    }
}
