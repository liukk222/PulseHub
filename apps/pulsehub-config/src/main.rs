#![forbid(unsafe_code)]

use std::env;
use std::process::ExitCode;

use pulsehub_ipc::{AgentSnapshot, PROTOCOL_VERSION, Request, Response, read_frame, write_frame};

fn main() -> ExitCode {
    match env::args().nth(1).as_deref() {
        Some("--inspect-agent") => inspect_agent(),
        Some("-h" | "--help") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some(argument) => {
            eprintln!("未知参数：{argument}");
            print_help();
            ExitCode::from(2)
        }
        None => {
            println!("PulseHub config skeleton: IPC protocol v{PROTOCOL_VERSION}");
            println!("使用 --inspect-agent 读取代理脱敏快照。");
            ExitCode::SUCCESS
        }
    }
}

#[cfg(windows)]
fn inspect_agent() -> ExitCode {
    use pulsehub_ipc::windows::{DEFAULT_PIPE_PATH, connect};

    let mut stream = match connect(DEFAULT_PIPE_PATH) {
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
    let request = Request::GetSnapshot {
        version: PROTOCOL_VERSION,
        request_id: "config-snapshot-1".to_owned(),
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
    let snapshot: AgentSnapshot = match serde_json::from_value(data) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("IPC 快照格式无效：{error}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "代理快照：设备={:?}，环境={:?}，当前 DPI={}，目标 DPI={}，配置修订={}",
        snapshot.device_status,
        snapshot.active_environment,
        snapshot
            .current_dpi
            .map_or_else(|| "未知".to_owned(), |dpi| dpi.to_string()),
        snapshot.desired_dpi,
        snapshot.config_revision
    );
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
fn inspect_agent() -> ExitCode {
    eprintln!("IPC Named Pipe 客户端仅支持 Windows。");
    ExitCode::FAILURE
}

fn print_help() {
    println!("用法：pulsehub-config [--inspect-agent]");
    println!("  --inspect-agent  连接代理并读取只读脱敏快照");
}
