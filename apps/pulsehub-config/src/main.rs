#![forbid(unsafe_code)]

use std::env;
use std::process::ExitCode;
#[cfg(windows)]
use std::time::Duration;

use pulsehub_ipc::{AgentSnapshot, PROTOCOL_VERSION, Request, Response, read_frame, write_frame};

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
        None => {
            println!("PulseHub config skeleton: IPC protocol v{PROTOCOL_VERSION}");
            println!("使用 --inspect-agent 读取代理脱敏快照。");
            ExitCode::SUCCESS
        }
    }
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
