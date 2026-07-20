#![forbid(unsafe_code)]

use std::fmt;
use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(windows)]
pub mod windows;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_REQUEST_ID_BYTES: usize = 128;
pub const MAX_CLIENT_NAME_BYTES: usize = 64;
pub const MAX_SUPPORTED_VERSIONS: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Hello {
        version: u32,
        request_id: String,
        supported_versions: Vec<u32>,
        client: String,
    },
    GetSnapshot {
        version: u32,
        request_id: String,
    },
    ValidateDraft {
        version: u32,
        request_id: String,
        draft: Value,
    },
    CommitConfig {
        version: u32,
        request_id: String,
        base_revision: u64,
        draft: Value,
    },
    ApplyNow {
        version: u32,
        request_id: String,
    },
    SetSelectionMode {
        version: u32,
        request_id: String,
        mode: SelectionMode,
    },
    AttachUi {
        version: u32,
        request_id: String,
    },
}

impl Request {
    pub fn version(&self) -> u32 {
        match self {
            Self::Hello { version, .. }
            | Self::GetSnapshot { version, .. }
            | Self::ValidateDraft { version, .. }
            | Self::CommitConfig { version, .. }
            | Self::ApplyNow { version, .. }
            | Self::SetSelectionMode { version, .. }
            | Self::AttachUi { version, .. } => *version,
        }
    }

    pub fn request_id(&self) -> &str {
        match self {
            Self::Hello { request_id, .. }
            | Self::GetSnapshot { request_id, .. }
            | Self::ValidateDraft { request_id, .. }
            | Self::CommitConfig { request_id, .. }
            | Self::ApplyNow { request_id, .. }
            | Self::SetSelectionMode { request_id, .. }
            | Self::AttachUi { request_id, .. } => request_id,
        }
    }

    pub fn wire_name(&self) -> &'static str {
        match self {
            Self::Hello { .. } => "hello",
            Self::GetSnapshot { .. } => "get_snapshot",
            Self::ValidateDraft { .. } => "validate_draft",
            Self::CommitConfig { .. } => "commit_config",
            Self::ApplyNow { .. } => "apply_now",
            Self::SetSelectionMode { .. } => "set_selection_mode",
            Self::AttachUi { .. } => "attach_ui",
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version() != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion {
                found: self.version(),
            });
        }
        validate_text("request_id", self.request_id(), MAX_REQUEST_ID_BYTES)?;
        if let Self::Hello {
            supported_versions,
            client,
            ..
        } = self
        {
            if supported_versions.is_empty()
                || supported_versions.len() > MAX_SUPPORTED_VERSIONS
                || !supported_versions.contains(&PROTOCOL_VERSION)
            {
                return Err(ProtocolError::InvalidField(
                    "supported_versions 必须包含当前版本且数量不超过 16".to_owned(),
                ));
            }
            validate_text("client", client, MAX_CLIENT_NAME_BYTES)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    Auto,
    Office,
    Cs2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    Office,
    Cs2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
    Unknown,
    Disconnected,
    Ready,
    Busy,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSnapshot {
    pub device_status: DeviceStatus,
    pub active_environment: Environment,
    pub config_revision: u64,
    pub current_dpi: Option<u16>,
    pub desired_dpi: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub request_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl Response {
    pub fn success(request_id: impl Into<String>, data: Value) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn failure(request_id: impl Into<String>, error: ErrorBody) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            ok: false,
            data: None,
            error: Some(error),
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion {
                found: self.version,
            });
        }
        validate_text("request_id", &self.request_id, MAX_REQUEST_ID_BYTES)?;
        if self.ok == self.error.is_some() || self.ok != self.data.is_some() {
            return Err(ProtocolError::InvalidEnvelope);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    #[serde(rename = "PH-IPC-INVALID-REQUEST")]
    InvalidRequest,
    #[serde(rename = "PH-IPC-VERSION")]
    Version,
    #[serde(rename = "PH-IPC-CONFLICT")]
    Conflict,
    #[serde(rename = "PH-IPC-BUSY")]
    Busy,
    #[serde(rename = "PH-IPC-INTERNAL")]
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    SnapshotChanged {
        version: u32,
        snapshot: AgentSnapshot,
    },
    ApplyStarted {
        version: u32,
        environment: Environment,
    },
    ApplyFinished {
        version: u32,
        snapshot: AgentSnapshot,
    },
    DeviceChanged {
        version: u32,
        snapshot: AgentSnapshot,
    },
    ActivateUi {
        version: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    Io(io::ErrorKind),
    EmptyPayload,
    PayloadTooLarge { length: usize, maximum: usize },
    Json(String),
    Protocol(ProtocolError),
}

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(kind) => write!(formatter, "IPC I/O 错误：{kind:?}"),
            Self::EmptyPayload => formatter.write_str("IPC payload 不能为空"),
            Self::PayloadTooLarge { length, maximum } => {
                write!(formatter, "IPC payload {length} 字节超过上限 {maximum}")
            }
            Self::Json(message) => write!(formatter, "IPC JSON 错误：{message}"),
            Self::Protocol(error) => write!(formatter, "IPC 协议错误：{error}"),
        }
    }
}

impl std::error::Error for FrameError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    NotNegotiated,
    AlreadyNegotiated,
    UnsupportedVersion { found: u32 },
    InvalidField(String),
    InvalidEnvelope,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotNegotiated => formatter.write_str("尚未完成 hello 版本协商"),
            Self::AlreadyNegotiated => formatter.write_str("hello 版本协商已经完成"),
            Self::UnsupportedVersion { found } => {
                write!(formatter, "不支持 IPC 协议版本 {found}")
            }
            Self::InvalidField(message) => write!(formatter, "字段无效：{message}"),
            Self::InvalidEnvelope => formatter.write_str("响应 data/error 与 ok 状态不一致"),
        }
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Debug, Default)]
pub struct Session {
    negotiated: bool,
}

impl Session {
    pub fn accept(&mut self, request: &Request) -> Result<(), ProtocolError> {
        request.validate()?;
        match (self.negotiated, request) {
            (false, Request::Hello { .. }) => {
                self.negotiated = true;
                Ok(())
            }
            (false, _) => Err(ProtocolError::NotNegotiated),
            (true, Request::Hello { .. }) => Err(ProtocolError::AlreadyNegotiated),
            (true, _) => Ok(()),
        }
    }

    pub fn is_negotiated(&self) -> bool {
        self.negotiated
    }
}

pub fn dispatch_request(
    session: &mut Session,
    request: &Request,
    snapshot: &AgentSnapshot,
) -> Response {
    let request_id = request.request_id().to_owned();
    if let Err(error) = session.accept(request) {
        return Response::failure(
            request_id,
            ErrorBody {
                code: match error {
                    ProtocolError::UnsupportedVersion { .. } => ErrorCode::Version,
                    _ => ErrorCode::InvalidRequest,
                },
                message: error.to_string(),
                retryable: false,
            },
        );
    }
    dispatch_accepted_request(request, snapshot)
}

pub fn dispatch_accepted_request(request: &Request, snapshot: &AgentSnapshot) -> Response {
    let request_id = request.request_id().to_owned();
    match request {
        Request::Hello { .. } => Response::success(
            request_id,
            serde_json::json!({"selected_version": PROTOCOL_VERSION}),
        ),
        Request::GetSnapshot { .. } => match serde_json::to_value(snapshot) {
            Ok(data) => Response::success(request_id, data),
            Err(error) => Response::failure(
                request_id,
                ErrorBody {
                    code: ErrorCode::Internal,
                    message: error.to_string(),
                    retryable: false,
                },
            ),
        },
        _ => Response::failure(
            request_id,
            ErrorBody {
                code: ErrorCode::InvalidRequest,
                message: format!("{} 尚未由代理实现", request.wire_name()),
                retryable: false,
            },
        ),
    }
}

pub fn serve_next<T: Read + Write>(
    stream: &mut T,
    session: &mut Session,
    snapshot: &AgentSnapshot,
) -> Result<(), FrameError> {
    let request = read_request(stream)?;
    let response = dispatch_request(session, &request, snapshot);
    response.validate().map_err(FrameError::Protocol)?;
    write_frame(stream, &response)
}

pub fn serve_next_with<T: Read + Write>(
    stream: &mut T,
    session: &mut Session,
    snapshot: impl FnOnce() -> AgentSnapshot,
) -> Result<(), FrameError> {
    let request = read_request(stream)?;
    let snapshot = snapshot();
    let response = dispatch_request(session, &request, &snapshot);
    response.validate().map_err(FrameError::Protocol)?;
    write_frame(stream, &response)
}

pub fn write_frame(writer: &mut impl Write, message: &impl Serialize) -> Result<(), FrameError> {
    let payload =
        serde_json::to_vec(message).map_err(|error| FrameError::Json(error.to_string()))?;
    if payload.is_empty() {
        return Err(FrameError::EmptyPayload);
    }
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(FrameError::PayloadTooLarge {
            length: payload.len(),
            maximum: MAX_PAYLOAD_BYTES,
        });
    }
    let length = u32::try_from(payload.len()).map_err(|_| FrameError::PayloadTooLarge {
        length: payload.len(),
        maximum: MAX_PAYLOAD_BYTES,
    })?;
    writer
        .write_all(&length.to_le_bytes())
        .and_then(|()| writer.write_all(&payload))
        .map_err(|error| FrameError::Io(error.kind()))
}

pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T, FrameError> {
    let mut prefix = [0_u8; 4];
    reader
        .read_exact(&mut prefix)
        .map_err(|error| FrameError::Io(error.kind()))?;
    let length = usize::try_from(u32::from_le_bytes(prefix)).unwrap_or(usize::MAX);
    if length == 0 {
        return Err(FrameError::EmptyPayload);
    }
    if length > MAX_PAYLOAD_BYTES {
        return Err(FrameError::PayloadTooLarge {
            length,
            maximum: MAX_PAYLOAD_BYTES,
        });
    }
    let mut payload = vec![0_u8; length];
    reader
        .read_exact(&mut payload)
        .map_err(|error| FrameError::Io(error.kind()))?;
    serde_json::from_slice(&payload).map_err(|error| FrameError::Json(error.to_string()))
}

pub fn read_request(reader: &mut impl Read) -> Result<Request, FrameError> {
    let request: Request = read_frame(reader)?;
    request.validate().map_err(FrameError::Protocol)?;
    Ok(request)
}

fn validate_text(field: &str, value: &str, maximum: usize) -> Result<(), ProtocolError> {
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        Err(ProtocolError::InvalidField(format!(
            "{field} 必须为 1–{maximum} 字节且不含控制字符"
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, ErrorKind};

    use serde_json::json;

    use super::*;

    fn hello() -> Request {
        Request::Hello {
            version: 1,
            request_id: "hello-1".to_owned(),
            supported_versions: vec![1],
            client: "pulsehub-config".to_owned(),
        }
    }

    #[test]
    fn request_names_match_the_documented_wire_format() {
        assert_eq!(
            Request::GetSnapshot {
                version: 1,
                request_id: "42".to_owned()
            }
            .wire_name(),
            "get_snapshot"
        );
    }

    #[test]
    fn frame_round_trips_with_little_endian_length() {
        let request = hello();
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &request).unwrap();
        assert_eq!(
            u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize,
            bytes.len() - 4
        );
        assert_eq!(read_request(&mut Cursor::new(bytes)).unwrap(), request);
    }

    #[test]
    fn rejects_truncated_and_oversized_frames_before_json_decode() {
        assert_eq!(
            read_frame::<Request>(&mut Cursor::new([1_u8, 0])),
            Err(FrameError::Io(ErrorKind::UnexpectedEof))
        );
        let oversized = u32::try_from(MAX_PAYLOAD_BYTES + 1).unwrap().to_le_bytes();
        assert_eq!(
            read_frame::<Request>(&mut Cursor::new(oversized)),
            Err(FrameError::PayloadTooLarge {
                length: MAX_PAYLOAD_BYTES + 1,
                maximum: MAX_PAYLOAD_BYTES
            })
        );
        let truncated = [5_u8, 0, 0, 0, b'{'];
        assert_eq!(
            read_frame::<Request>(&mut Cursor::new(truncated)),
            Err(FrameError::Io(ErrorKind::UnexpectedEof))
        );
    }

    #[test]
    fn rejects_unknown_fields_invalid_utf8_and_unknown_types() {
        for payload in [
            br#"{"version":1,"request_id":"x","type":"get_snapshot","extra":true}"#.as_slice(),
            br#"{"version":1,"request_id":"x","type":"not_real"}"#.as_slice(),
            &[0xff, 0xfe],
        ] {
            let mut frame = u32::try_from(payload.len()).unwrap().to_le_bytes().to_vec();
            frame.extend_from_slice(payload);
            assert!(matches!(
                read_frame::<Request>(&mut Cursor::new(frame)),
                Err(FrameError::Json(_))
            ));
        }
    }

    #[test]
    fn session_requires_hello_before_other_requests() {
        let snapshot = Request::GetSnapshot {
            version: 1,
            request_id: "42".to_owned(),
        };
        let mut session = Session::default();
        assert_eq!(session.accept(&snapshot), Err(ProtocolError::NotNegotiated));
        assert_eq!(session.accept(&hello()), Ok(()));
        assert!(session.is_negotiated());
        assert_eq!(session.accept(&snapshot), Ok(()));
        assert_eq!(
            session.accept(&hello()),
            Err(ProtocolError::AlreadyNegotiated)
        );
    }

    #[test]
    fn response_enforces_ok_data_error_invariant() {
        let success = Response::success("42", json!({"selected_version": 1}));
        assert_eq!(success.validate(), Ok(()));
        let invalid = Response {
            error: Some(ErrorBody {
                code: ErrorCode::Internal,
                message: "bad".to_owned(),
                retryable: false,
            }),
            ..success
        };
        assert_eq!(invalid.validate(), Err(ProtocolError::InvalidEnvelope));
    }

    #[test]
    fn writer_rejects_payload_above_limit() {
        let huge = json!({"value": "x".repeat(MAX_PAYLOAD_BYTES)});
        assert!(matches!(
            write_frame(&mut Vec::new(), &huge),
            Err(FrameError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn dispatcher_negotiates_and_returns_snapshot() {
        let snapshot = AgentSnapshot {
            device_status: DeviceStatus::Ready,
            active_environment: Environment::Office,
            config_revision: 3,
            current_dpi: Some(1800),
            desired_dpi: 1800,
        };
        let mut session = Session::default();
        let hello_response = dispatch_request(&mut session, &hello(), &snapshot);
        assert!(hello_response.ok);
        let request = Request::GetSnapshot {
            version: 1,
            request_id: "snapshot-1".to_owned(),
        };
        let response = dispatch_request(&mut session, &request, &snapshot);
        assert_eq!(response.data, Some(serde_json::to_value(snapshot).unwrap()));
    }

    #[test]
    fn dispatcher_rejects_snapshot_before_hello() {
        let snapshot = AgentSnapshot {
            device_status: DeviceStatus::Unknown,
            active_environment: Environment::Office,
            config_revision: 0,
            current_dpi: None,
            desired_dpi: 3200,
        };
        let request = Request::GetSnapshot {
            version: 1,
            request_id: "too-early".to_owned(),
        };
        let response = dispatch_request(&mut Session::default(), &request, &snapshot);
        assert!(!response.ok);
        assert_eq!(response.error.unwrap().code, ErrorCode::InvalidRequest);
    }
}
