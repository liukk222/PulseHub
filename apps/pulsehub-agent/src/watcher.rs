#![forbid(unsafe_code)]

use std::time::Duration;

#[cfg(windows)]
pub fn run(
    exit_after: Option<Duration>,
    mut on_foreground_changed: impl FnMut() -> Option<Duration>,
) -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::Instant;

    use win_event_hook::events::{Event, NamedEvent};

    let (sender, receiver) = mpsc::sync_channel(1);
    let config = win_event_hook::Config::builder()
        .skip_own_process()
        .with_dedicated_thread_name("PulseHubForegroundHook")
        .with_event(Event::Named(NamedEvent::SystemForeground))
        .finish();
    let mut hook = win_event_hook::WinEventHook::install(config, move |_, _, _, _, _, _| {
        let _ = sender.try_send(());
    })
    .map_err(|error| format!("安装 Windows 前台事件 hook 失败：{error}"))?;

    let stopping = Arc::new(AtomicBool::new(false));
    let stopping_for_handler = Arc::clone(&stopping);
    ctrlc::set_handler(move || stopping_for_handler.store(true, Ordering::Release))
        .map_err(|error| format!("安装 Ctrl+C 处理器失败：{error}"))?;

    let started = Instant::now();
    let mut retry_at = on_foreground_changed().map(|delay| Instant::now() + delay);
    while !stopping.load(Ordering::Acquire)
        && exit_after.is_none_or(|duration| started.elapsed() < duration)
    {
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(()) => {
                std::thread::sleep(Duration::from_millis(75));
                while receiver.try_recv().is_ok() {}
                retry_at = on_foreground_changed().map(|delay| Instant::now() + delay);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if retry_at.is_some_and(|deadline| Instant::now() >= deadline) {
                    retry_at = on_foreground_changed().map(|delay| Instant::now() + delay);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Windows 前台事件通道意外断开".to_owned());
            }
        }
    }
    hook.uninstall()
        .map_err(|error| format!("卸载 Windows 前台事件 hook 失败：{error}"))?;
    Ok(())
}

#[cfg(not(windows))]
pub fn run(
    _exit_after: Option<Duration>,
    _on_foreground_changed: impl FnMut() -> Option<Duration>,
) -> Result<(), String> {
    Err("前台事件监听当前仅支持 Windows".to_owned())
}
