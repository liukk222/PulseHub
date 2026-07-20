use std::io;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use interprocess::os::windows::named_pipe::{
    DuplexPipeStream, PipeListener, PipeListenerOptions, pipe_mode,
};
use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use widestring::U16CString;

use crate::{
    AgentSnapshot, ErrorBody, ErrorCode, FrameError, Request, Response, Session,
    dispatch_accepted_request, read_request, serve_next, write_frame,
};

pub const PIPE_PATH_PREFIX: &str = r"\\.\pipe\PulseHub.Agent.";

type ByteListener = PipeListener<pipe_mode::Bytes, pipe_mode::Bytes>;
pub type ByteStream = DuplexPipeStream<pipe_mode::Bytes>;

pub fn default_pipe_path() -> io::Result<String> {
    let sid = pulsehub_windows_session::current_logon_sid()?;
    if !sid.starts_with("S-1-5-5-")
        || !sid[2..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "当前 TokenLogonSid 格式无效",
        ));
    }
    Ok(format!("{PIPE_PATH_PREFIX}{sid}"))
}

pub struct Server {
    listener: ByteListener,
}

impl Server {
    pub fn bind(path: impl AsRef<Path>) -> io::Result<Self> {
        let sid = pulsehub_windows_session::current_logon_sid()?;
        let sddl = U16CString::from_str(format!("D:P(A;;GA;;;{sid})")).map_err(io::Error::other)?;
        let security_descriptor = SecurityDescriptor::deserialize(&sddl)?;
        let listener = PipeListenerOptions::new()
            .path(path.as_ref())
            .accept_remote(false)
            .inheritable(false)
            .input_buffer_size_hint(u32::try_from(crate::MAX_PAYLOAD_BYTES + 4).unwrap())
            .output_buffer_size_hint(u32::try_from(crate::MAX_PAYLOAD_BYTES + 4).unwrap())
            .security_descriptor(Some(security_descriptor))
            .create_duplex::<pipe_mode::Bytes>()?;
        Ok(Self { listener })
    }

    pub fn accept(&self) -> io::Result<ByteStream> {
        self.listener.accept()
    }

    pub fn serve_connection(
        &self,
        stream: &mut ByteStream,
        snapshot: &AgentSnapshot,
    ) -> Result<(), FrameError> {
        let mut session = Session::default();
        loop {
            match serve_next(stream, &mut session, snapshot) {
                Ok(()) => {}
                Err(FrameError::Io(io::ErrorKind::UnexpectedEof)) => return Ok(()),
                Err(error) => return Err(error),
            }
        }
    }
}

pub fn serve_connection_with(
    stream: &mut ByteStream,
    snapshot: impl Fn() -> AgentSnapshot,
) -> Result<(), FrameError> {
    serve_connection_with_handler(stream, snapshot, |_, _| None)
}

pub fn serve_connection_with_handler(
    stream: &mut ByteStream,
    snapshot: impl Fn() -> AgentSnapshot,
    mut handler: impl FnMut(&Request, &AgentSnapshot) -> Option<Response>,
) -> Result<(), FrameError> {
    let mut session = Session::default();
    loop {
        let request = match read_request(stream) {
            Ok(request) => request,
            Err(FrameError::Io(io::ErrorKind::UnexpectedEof)) => return Ok(()),
            Err(error) => return Err(error),
        };
        let current = snapshot();
        let response = match session.accept(&request) {
            Ok(()) => handler(&request, &current)
                .unwrap_or_else(|| dispatch_accepted_request(&request, &current)),
            Err(error) => Response::failure(
                request.request_id(),
                ErrorBody {
                    code: ErrorCode::InvalidRequest,
                    message: error.to_string(),
                    retryable: false,
                },
            ),
        };
        response.validate().map_err(FrameError::Protocol)?;
        write_frame(stream, &response)?;
    }
}

pub fn connect(path: impl AsRef<Path>) -> io::Result<ByteStream> {
    ByteStream::connect_by_path(path.as_ref())
}

pub fn connect_with_retry(
    path: impl AsRef<Path>,
    timeout: Duration,
    retry_interval: Duration,
) -> io::Result<ByteStream> {
    if retry_interval.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "IPC 重试间隔必须大于零",
        ));
    }
    let deadline = Instant::now() + timeout;
    loop {
        match connect(path.as_ref()) {
            Ok(stream) => return Ok(stream),
            Err(error)
                if matches!(error.raw_os_error(), Some(2 | 231)) && Instant::now() < deadline =>
            {
                thread::sleep(
                    retry_interval.min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    use crate::{
        DeviceStatus, Environment, PROTOCOL_VERSION, Request, Response, read_frame, write_frame,
    };

    use super::*;

    static NEXT_PIPE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn named_pipe_round_trips_hello_and_snapshot() {
        let path = format!(
            r"\\.\pipe\PulseHub.Test.{}.{}",
            std::process::id(),
            NEXT_PIPE.fetch_add(1, Ordering::Relaxed)
        );
        let server = Server::bind(&path).unwrap();
        let snapshot = AgentSnapshot {
            device_status: DeviceStatus::Ready,
            active_environment: Environment::Cs2,
            config_revision: 9,
            current_dpi: Some(100),
            desired_dpi: 100,
            dpi_capability: None,
            integration_status: crate::IntegrationStatus::Unknown,
        };
        let server_thread = thread::spawn({
            let snapshot = snapshot.clone();
            move || {
                let mut stream = server.accept().unwrap();
                let mut session = Session::default();
                serve_next(&mut stream, &mut session, &snapshot).unwrap();
                serve_next(&mut stream, &mut session, &snapshot).unwrap();
            }
        });

        let mut client = connect(&path).unwrap();
        write_frame(
            &mut client,
            &Request::Hello {
                version: PROTOCOL_VERSION,
                request_id: "hello-test".to_owned(),
                supported_versions: vec![PROTOCOL_VERSION],
                client: "pulsehub-test".to_owned(),
            },
        )
        .unwrap();
        let hello: Response = read_frame(&mut client).unwrap();
        assert!(hello.ok);

        write_frame(
            &mut client,
            &Request::GetSnapshot {
                version: PROTOCOL_VERSION,
                request_id: "snapshot-test".to_owned(),
            },
        )
        .unwrap();
        let response: Response = read_frame(&mut client).unwrap();
        assert_eq!(response.data, Some(serde_json::to_value(snapshot).unwrap()));
        server_thread.join().unwrap();
    }

    #[test]
    fn client_retries_until_listener_is_created() {
        let path = format!(
            r"\\.\pipe\PulseHub.DelayedTest.{}.{}",
            std::process::id(),
            NEXT_PIPE.fetch_add(1, Ordering::Relaxed)
        );
        let server_path = path.clone();
        let server_thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            let server = Server::bind(server_path).unwrap();
            let _stream = server.accept().unwrap();
        });
        let stream =
            connect_with_retry(path, Duration::from_secs(1), Duration::from_millis(10)).unwrap();
        drop(stream);
        server_thread.join().unwrap();
    }

    #[test]
    fn custom_handler_processes_apply_now_after_hello() {
        let path = format!(
            r"\\.\pipe\PulseHub.HandlerTest.{}.{}",
            std::process::id(),
            NEXT_PIPE.fetch_add(1, Ordering::Relaxed)
        );
        let server = Server::bind(&path).unwrap();
        let snapshot = AgentSnapshot {
            device_status: DeviceStatus::Ready,
            active_environment: Environment::Office,
            config_revision: 1,
            current_dpi: Some(3200),
            desired_dpi: 3200,
            dpi_capability: None,
            integration_status: crate::IntegrationStatus::Unknown,
        };
        let server_thread = thread::spawn(move || {
            let mut stream = server.accept().unwrap();
            serve_connection_with_handler(
                &mut stream,
                || snapshot.clone(),
                |request, _| {
                    matches!(request, Request::ApplyNow { .. }).then(|| {
                        Response::success(
                            request.request_id(),
                            serde_json::json!({"handled": true}),
                        )
                    })
                },
            )
            .unwrap();
        });
        let mut client = connect(&path).unwrap();
        write_frame(
            &mut client,
            &Request::Hello {
                version: PROTOCOL_VERSION,
                request_id: "hello-handler".to_owned(),
                supported_versions: vec![PROTOCOL_VERSION],
                client: "handler-test".to_owned(),
            },
        )
        .unwrap();
        let _: Response = read_frame(&mut client).unwrap();
        write_frame(
            &mut client,
            &Request::ApplyNow {
                version: PROTOCOL_VERSION,
                request_id: "apply-handler".to_owned(),
            },
        )
        .unwrap();
        let response: Response = read_frame(&mut client).unwrap();
        assert_eq!(response.data, Some(serde_json::json!({"handled": true})));
        drop(client);
        server_thread.join().unwrap();
    }

    #[test]
    fn default_path_contains_current_logon_sid() {
        let sid = pulsehub_windows_session::current_logon_sid().unwrap();
        assert_eq!(
            default_pipe_path().unwrap(),
            format!("{PIPE_PATH_PREFIX}{sid}")
        );
    }
}
