#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const CLEANUP_INTERVAL: Duration = Duration::from_secs(10 * 24 * 60 * 60);
const LOG_PREFIX: &str = "pulsehub-agent-";
const LOG_SUFFIX: &str = ".log";

struct LoggerState {
    directory: PathBuf,
    last_cleanup: Mutex<Instant>,
}

static LOGGER: OnceLock<LoggerState> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn initialize() -> Result<(), String> {
    let app_data = std::env::var_os("APPDATA").ok_or("APPDATA 环境变量不存在")?;
    let directory = PathBuf::from(app_data).join("PulseHub").join("logs");
    fs::create_dir_all(&directory).map_err(|error| format!("创建日志目录失败：{error}"))?;
    let removed = cleanup_expired_at(&directory, SystemTime::now())?;
    let _ = LOGGER.set(LoggerState {
        directory,
        last_cleanup: Mutex::new(Instant::now()),
    });
    if removed != 0 {
        eprintln!("本次启动已永久删除 {removed} 个过期 PulseHub 日志");
    }
    Ok(())
}

pub fn set_enabled(enabled: bool) {
    let changed = ENABLED.swap(enabled, Ordering::AcqRel) != enabled;
    if enabled && changed {
        info(format_args!("开发者日志已启用"));
    }
}

pub fn info(arguments: std::fmt::Arguments<'_>) {
    write_entry("INFO", arguments);
}

pub fn error(arguments: std::fmt::Arguments<'_>) {
    write_entry("ERROR", arguments);
}

pub fn run_periodic_maintenance() {
    let Some(logger) = LOGGER.get() else {
        return;
    };
    let mut last_cleanup = logger
        .last_cleanup
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if last_cleanup.elapsed() < CLEANUP_INTERVAL {
        return;
    }
    *last_cleanup = Instant::now();
    drop(last_cleanup);
    match cleanup_expired_at(&logger.directory, SystemTime::now()) {
        Ok(removed) => info(format_args!(
            "十天周期日志清理完成；删除 {removed} 个过期日志"
        )),
        Err(cleanup_error) => error(format_args!("十天周期日志清理失败：{cleanup_error}")),
    }
}

fn write_entry(level: &str, arguments: std::fmt::Arguments<'_>) {
    if !ENABLED.load(Ordering::Acquire) {
        return;
    }
    let Some(logger) = LOGGER.get() else {
        return;
    };
    let now = SystemTime::now();
    let timestamp = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let day = timestamp / (24 * 60 * 60);
    let path = logger
        .directory
        .join(format!("{LOG_PREFIX}{day}{LOG_SUFFIX}"));
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "[{timestamp}] [{level}] {arguments}");
    }
}

fn cleanup_expired_at(directory: &Path, now: SystemTime) -> Result<usize, String> {
    let entries = fs::read_dir(directory).map_err(|error| format!("读取日志目录失败：{error}"))?;
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|error| format!("读取日志目录项失败：{error}"))?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !is_managed_log_name(file_name) {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| format!("读取日志元数据失败：{error}"))?;
        if !metadata.file_type().is_file() {
            continue;
        }
        let modified = metadata
            .modified()
            .map_err(|error| format!("读取日志修改时间失败：{error}"))?;
        if now.duration_since(modified).unwrap_or_default() > RETENTION {
            fs::remove_file(entry.path())
                .map_err(|error| format!("硬删除过期日志失败：{error}"))?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn is_managed_log_name(file_name: &str) -> bool {
    file_name
        .strip_prefix(LOG_PREFIX)
        .and_then(|value| value.strip_suffix(LOG_SUFFIX))
        .is_some_and(|day| !day.is_empty() && day.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_directory(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("pulsehub-log-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn recognizes_only_managed_daily_logs() {
        assert!(is_managed_log_name("pulsehub-agent-20654.log"));
        assert!(!is_managed_log_name("pulsehub-agent-current.log"));
        assert!(!is_managed_log_name("config.toml"));
        assert!(!is_managed_log_name("other-20654.log"));
    }

    #[test]
    fn cleanup_hard_deletes_only_expired_managed_files() {
        let directory = temporary_directory("retention");
        let expired = directory.join("pulsehub-agent-1.log");
        let unmanaged = directory.join("notes.log");
        fs::write(&expired, b"expired").unwrap();
        fs::write(&unmanaged, b"keep").unwrap();
        let future = SystemTime::now() + RETENTION + Duration::from_secs(1);

        assert_eq!(cleanup_expired_at(&directory, future).unwrap(), 1);
        assert!(!expired.exists());
        assert!(unmanaged.exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
