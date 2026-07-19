//! Logitech HID++ 2.0 的只读探测会话。

use std::fmt;
use std::time::Duration;

use crate::discovery::G102_PRODUCT_IDS;

const REPORT_ID_SHORT: u8 = 0x10;
const REPORT_ID_LONG: u8 = 0x11;
const WIRED_DEVICE_INDEX: u8 = 0xff;
const SOFTWARE_ID: u8 = 0x08;
const ROOT_FEATURE_INDEX: u8 = 0x00;
const FEATURE_SET_ID: u16 = 0x0001;
const ADJUSTABLE_DPI_ID: u16 = 0x2201;
const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_FEATURE_COUNT: u8 = 64;
const MAX_TRACE_FRAMES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidppFeature {
    pub id: u16,
    pub index: u8,
    pub feature_type: u8,
    pub version: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DpiSensorInfo {
    pub index: u8,
    pub minimum: u16,
    pub maximum: u16,
    pub step: Option<u16>,
    pub discrete_values: Vec<u16>,
    pub current: u16,
    pub default: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidppProbeResult {
    pub protocol_major: u8,
    pub protocol_minor: u8,
    pub features: Vec<HidppFeature>,
    pub dpi_sensors: Vec<DpiSensorInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidppError {
    PlatformUnsupported,
    InterfaceNotFound,
    Backend(String),
    Timeout,
    InvalidResponse(String),
    Device { code: u8, description: &'static str },
}

impl fmt::Display for HidppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlatformUnsupported => formatter.write_str("HID++ 探测当前仅支持 Windows"),
            Self::InterfaceNotFound => formatter.write_str("未找到匹配的 G102 HID++ 短/长报告接口"),
            Self::Backend(message) => write!(formatter, "HID 后端错误：{message}"),
            Self::Timeout => formatter.write_str("等待 HID++ 响应超时"),
            Self::InvalidResponse(message) => write!(formatter, "无效 HID++ 响应：{message}"),
            Self::Device { code, description } => {
                write!(
                    formatter,
                    "设备返回 HID++ 错误 0x{code:02x}（{description}）"
                )
            }
        }
    }
}

impl std::error::Error for HidppError {}

#[cfg(windows)]
pub fn probe_first_g102(protocol_trace: bool) -> Result<HidppProbeResult, HidppError> {
    let mut transport = WindowsHidppTransport::open(protocol_trace)?;
    probe(&mut transport)
}

#[cfg(not(windows))]
pub fn probe_first_g102(_protocol_trace: bool) -> Result<HidppProbeResult, HidppError> {
    Err(HidppError::PlatformUnsupported)
}

trait Transport {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, HidppError>;
}

fn probe(transport: &mut impl Transport) -> Result<HidppProbeResult, HidppError> {
    let version_response = request(transport, ROOT_FEATURE_INDEX, 1, [0, 0, 0])?;
    let version_parameters = parameters(&version_response, 2)?;
    let protocol_major = version_parameters[0];
    let protocol_minor = version_parameters[1];
    if protocol_major < 2 {
        return Err(HidppError::InvalidResponse(format!(
            "目标设备报告 HID++ {protocol_major}.{protocol_minor}，需要 2.0 或更高版本"
        )));
    }

    let feature_set = get_feature(transport, FEATURE_SET_ID)?;
    if feature_set.index == 0 {
        return Err(HidppError::InvalidResponse(
            "设备未公开 Feature Set (0x0001)".to_owned(),
        ));
    }
    let count_response = request(transport, feature_set.index, 0, [0, 0, 0])?;
    let count = parameters(&count_response, 1)?[0];
    if count > MAX_FEATURE_COUNT {
        return Err(HidppError::InvalidResponse(format!(
            "功能数量 {count} 超过安全上限 {MAX_FEATURE_COUNT}"
        )));
    }

    let mut features = Vec::with_capacity(usize::from(count) + 1);
    for ordinal in 0..=count {
        let response = request(transport, feature_set.index, 1, [ordinal, 0, 0])?;
        let feature_parameters = parameters(&response, 3)?;
        let id = u16::from_be_bytes([feature_parameters[0], feature_parameters[1]]);
        let mut feature = get_feature(transport, id)?;
        feature.feature_type = feature_parameters[2];
        features.push(feature);
    }
    features.sort_by_key(|feature| feature.index);

    let dpi_sensors = match features
        .iter()
        .find(|feature| feature.id == ADJUSTABLE_DPI_ID)
    {
        Some(feature) => read_dpi_sensors(transport, feature.index)?,
        None => Vec::new(),
    };
    Ok(HidppProbeResult {
        protocol_major,
        protocol_minor,
        features,
        dpi_sensors,
    })
}

fn get_feature(transport: &mut impl Transport, id: u16) -> Result<HidppFeature, HidppError> {
    if id == 0 {
        return Ok(HidppFeature {
            id,
            index: ROOT_FEATURE_INDEX,
            feature_type: 0,
            version: 0,
        });
    }
    let [high, low] = id.to_be_bytes();
    let response = request(transport, ROOT_FEATURE_INDEX, 0, [high, low, 0])?;
    let feature_parameters = parameters(&response, 3)?;
    Ok(HidppFeature {
        id,
        index: feature_parameters[0],
        feature_type: feature_parameters[1],
        version: feature_parameters[2],
    })
}

fn read_dpi_sensors(
    transport: &mut impl Transport,
    feature_index: u8,
) -> Result<Vec<DpiSensorInfo>, HidppError> {
    let count_response = request(transport, feature_index, 0, [0, 0, 0])?;
    let count = parameters(&count_response, 1)?[0];
    if count > 8 {
        return Err(HidppError::InvalidResponse(format!(
            "DPI 传感器数量 {count} 超过安全上限 8"
        )));
    }
    let mut sensors = Vec::with_capacity(usize::from(count));
    for requested_index in 0..count {
        let list_response = request(transport, feature_index, 1, [requested_index, 0, 0])?;
        let list_parameters = parameters(&list_response, 1)?;
        let index = list_parameters[0];
        let (minimum, maximum, step, discrete_values) = parse_dpi_list(&list_parameters[1..])?;

        let current_response = request(transport, feature_index, 2, [index, 0, 0])?;
        let current_parameters = parameters(&current_response, 5)?;
        if current_parameters[0] != index {
            return Err(HidppError::InvalidResponse(format!(
                "DPI 传感器索引不匹配：请求 {index}，响应 {}",
                current_parameters[0]
            )));
        }
        sensors.push(DpiSensorInfo {
            index,
            minimum,
            maximum,
            step,
            discrete_values,
            current: u16::from_be_bytes([current_parameters[1], current_parameters[2]]),
            default: u16::from_be_bytes([current_parameters[3], current_parameters[4]]),
        });
    }
    Ok(sensors)
}

fn parse_dpi_list(bytes: &[u8]) -> Result<(u16, u16, Option<u16>, Vec<u16>), HidppError> {
    let mut values = Vec::new();
    let mut step = None;
    for pair in bytes.chunks_exact(2) {
        let value = u16::from_be_bytes([pair[0], pair[1]]);
        if value == 0 {
            break;
        }
        if value > 0xe000 {
            step = Some(value - 0xe000);
        } else {
            values.push(value);
        }
    }
    let minimum = values.iter().copied().min().ok_or_else(|| {
        HidppError::InvalidResponse("DPI 列表没有包含任何范围或离散值".to_owned())
    })?;
    let maximum = values.iter().copied().max().unwrap_or(minimum);
    Ok((minimum, maximum, step, values))
}

fn request(
    transport: &mut impl Transport,
    feature_index: u8,
    function: u8,
    parameters: [u8; 3],
) -> Result<Vec<u8>, HidppError> {
    if function > 0x0f {
        return Err(HidppError::InvalidResponse(
            "HID++ 函数号超出 4 位范围".to_owned(),
        ));
    }
    let request = [
        REPORT_ID_SHORT,
        WIRED_DEVICE_INDEX,
        feature_index,
        (function << 4) | SOFTWARE_ID,
        parameters[0],
        parameters[1],
        parameters[2],
    ];
    let response = transport.transact(&request, REQUEST_TIMEOUT)?;
    validate_response(&request, &response)?;
    Ok(response)
}

fn validate_response(request: &[u8], response: &[u8]) -> Result<(), HidppError> {
    if response.len() < 7 {
        return Err(HidppError::InvalidResponse(format!(
            "报告过短：{} 字节",
            response.len()
        )));
    }
    if !matches!(response[0], REPORT_ID_SHORT | REPORT_ID_LONG) {
        return Err(HidppError::InvalidResponse(format!(
            "未知报告 ID 0x{:02x}",
            response[0]
        )));
    }
    if response[1] != request[1] {
        return Err(HidppError::InvalidResponse(format!(
            "设备索引不匹配：0x{:02x}",
            response[1]
        )));
    }
    if matches!(response[2], 0x8f | 0xff) && response[3] == request[2] && response[4] == request[3]
    {
        let code = response[5];
        return Err(HidppError::Device {
            code,
            description: error_description(code),
        });
    }
    if response[2] != request[2] || response[3] != request[3] {
        return Err(HidppError::InvalidResponse(
            "响应与请求元组不匹配".to_owned(),
        ));
    }
    Ok(())
}

fn parameters(response: &[u8], minimum: usize) -> Result<&[u8], HidppError> {
    let parameters = response
        .get(4..)
        .ok_or_else(|| HidppError::InvalidResponse("响应没有参数区".to_owned()))?;
    if parameters.len() < minimum {
        return Err(HidppError::InvalidResponse(format!(
            "参数区过短：需要 {minimum} 字节，实际 {} 字节",
            parameters.len()
        )));
    }
    Ok(parameters)
}

fn error_description(code: u8) -> &'static str {
    match code {
        0x01 => "未知错误",
        0x02 => "无效参数",
        0x03 => "超出范围",
        0x04 => "硬件错误",
        0x05 => "Logitech 内部错误",
        0x06 => "无效功能索引",
        0x07 => "无效函数",
        0x08 => "设备忙",
        0x09 => "不支持",
        _ => "未记录错误",
    }
}

fn response_matches(request: &[u8], response: &[u8]) -> bool {
    if response.len() < 5 || !matches!(response[0], REPORT_ID_SHORT | REPORT_ID_LONG) {
        return false;
    }
    if response[1] != request[1] {
        return false;
    }
    (response[2] == request[2] && response[3] == request[3])
        || (matches!(response[2], 0x8f | 0xff)
            && response[3] == request[2]
            && response[4] == request[3])
}

#[cfg(windows)]
struct WindowsHidppTransport {
    short: hidapi::HidDevice,
    long: hidapi::HidDevice,
    trace: bool,
    traced_frames: usize,
}

#[cfg(windows)]
impl WindowsHidppTransport {
    fn open(trace: bool) -> Result<Self, HidppError> {
        let api = hidapi::HidApi::new().map_err(|error| HidppError::Backend(error.to_string()))?;
        let short_info = api.device_list().find(|info| {
            info.vendor_id() == 0x046d
                && G102_PRODUCT_IDS.contains(&info.product_id())
                && info.usage_page() == 0xff00
                && info.usage() == 0x0001
        });
        let long_info = api.device_list().find(|info| {
            info.vendor_id() == 0x046d
                && G102_PRODUCT_IDS.contains(&info.product_id())
                && info.usage_page() == 0xff00
                && info.usage() == 0x0002
        });
        let (short_info, long_info) = short_info
            .zip(long_info)
            .ok_or(HidppError::InterfaceNotFound)?;
        if short_info.product_id() != long_info.product_id()
            || short_info.serial_number() != long_info.serial_number()
            || short_info.interface_number() != long_info.interface_number()
        {
            return Err(HidppError::InterfaceNotFound);
        }
        let short = short_info
            .open_device(&api)
            .map_err(|error| HidppError::Backend(error.to_string()))?;
        let long = long_info
            .open_device(&api)
            .map_err(|error| HidppError::Backend(error.to_string()))?;
        Ok(Self {
            short,
            long,
            trace,
            traced_frames: 0,
        })
    }

    fn trace(&mut self, direction: &str, frame: &[u8]) {
        if self.trace && self.traced_frames < MAX_TRACE_FRAMES {
            let bytes = frame
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(" ");
            eprintln!("HID++ {direction}: {bytes}");
            self.traced_frames += 1;
        }
    }

    fn try_read_matching(
        &mut self,
        request: &[u8],
        timeout_ms: i32,
    ) -> Result<Option<Vec<u8>>, HidppError> {
        let mut buffer = [0_u8; 20];
        for use_long in [false, true] {
            let length = if use_long {
                self.long.read_timeout(&mut buffer, timeout_ms)
            } else {
                self.short.read_timeout(&mut buffer, timeout_ms)
            }
            .map_err(|error| HidppError::Backend(error.to_string()))?;
            if length == 0 {
                continue;
            }
            let response = &buffer[..length];
            self.trace("RX", response);
            if response_matches(request, response) {
                return Ok(Some(response.to_vec()));
            }
        }
        Ok(None)
    }
}

#[cfg(windows)]
impl Transport for WindowsHidppTransport {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, HidppError> {
        self.trace("TX", request);
        let written = self
            .short
            .write(request)
            .map_err(|error| HidppError::Backend(error.to_string()))?;
        if written != request.len() {
            return Err(HidppError::Backend(format!(
                "HID++ 请求只写入 {written}/{} 字节",
                request.len()
            )));
        }

        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let timeout_ms = i32::try_from(remaining.as_millis().clamp(1, 10)).unwrap_or(10);
            if let Some(response) = self.try_read_matching(request, timeout_ms)? {
                return Ok(response);
            }
        }
        Err(HidppError::Timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::{HidppError, parse_dpi_list, response_matches, validate_response};

    const G102_FIXTURE: &str = include_str!("../tests/fixtures/g102-c092-release-5200.txt");

    #[test]
    fn matches_long_response_to_short_request() {
        let request = [0x10, 0xff, 0x00, 0x08, 0x22, 0x01, 0x00];
        let response = [
            0x11, 0xff, 0x00, 0x08, 0x0a, 0x00, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert!(response_matches(&request, &response));
        assert!(validate_response(&request, &response).is_ok());
    }

    #[test]
    fn decodes_continuous_dpi_range() {
        let bytes = [0x00, 0x32, 0xe0, 0x32, 0x1f, 0x40, 0x00, 0x00];
        assert_eq!(
            parse_dpi_list(&bytes).unwrap(),
            (50, 8000, Some(50), vec![50, 8000])
        );
    }

    #[test]
    fn surfaces_hidpp_device_error() {
        let request = [0x10, 0xff, 0x0a, 0x28, 0, 0, 0];
        let response = [0x10, 0xff, 0xff, 0x0a, 0x28, 0x09, 0];
        assert!(matches!(
            validate_response(&request, &response),
            Err(HidppError::Device { code: 0x09, .. })
        ));
    }

    #[test]
    fn validates_redacted_g102_fixture() {
        let protocol = fixture_frame("protocol_version_response");
        assert_eq!(&protocol[4..6], &[4, 2]);

        let dpi_list = fixture_frame("dpi_list_response");
        assert_eq!(
            parse_dpi_list(&dpi_list[5..]).unwrap(),
            (50, 8000, Some(50), vec![50, 8000])
        );

        let current = fixture_frame("dpi_current_response");
        assert_eq!(u16::from_be_bytes([current[5], current[6]]), 3200);
        assert_eq!(u16::from_be_bytes([current[7], current[8]]), 800);
        assert!(!G102_FIXTURE.contains("serial"));
        assert!(!G102_FIXTURE.contains("path"));
    }

    fn fixture_frame(key: &str) -> Vec<u8> {
        let prefix = format!("{key}=");
        G102_FIXTURE
            .lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("fixture 缺少 {key}"))
            .split_ascii_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).expect("fixture 必须包含十六进制字节"))
            .collect()
    }
}
