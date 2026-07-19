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
const ONBOARD_PROFILES_ID: u16 = 0x8100;
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
    pub onboard_profiles: Option<OnboardProfilesInfo>,
    pub onboard_profile: Option<OnboardProfileSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardMode {
    NoChange,
    Onboard,
    Host,
    Unknown(u8),
}

impl From<u8> for OnboardMode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::NoChange,
            1 => Self::Onboard,
            2 => Self::Host,
            value => Self::Unknown(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardProfilesInfo {
    pub memory_model_id: u8,
    pub profile_format_id: u8,
    pub macro_format_id: u8,
    pub profile_count: u8,
    pub rom_profile_count: u8,
    pub button_count: u8,
    pub sector_count: u8,
    pub sector_size: u16,
    pub mechanical_layout: u8,
    pub various_info: u8,
    pub mode: OnboardMode,
    pub current_profile: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardProfileSnapshot {
    pub address: u16,
    pub enabled: bool,
    pub report_rate_hz: u16,
    pub default_dpi_index: u8,
    pub shifted_dpi_index: u8,
    pub dpi_slots: Vec<u16>,
    pub buttons: Vec<OnboardButtonAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnboardButtonAction {
    Macro { page: u8, offset: u8 },
    Mouse { button: Option<u8>, mask: u16 },
    Keyboard { modifiers: u8, key: u8 },
    ConsumerControl { usage: u16 },
    Special { code: u8, profile: u8 },
    Disabled,
    Unknown([u8; 4]),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DpiWriteResult {
    pub sensor_index: u8,
    pub before: u16,
    pub requested: u16,
    pub after: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidppError {
    PlatformUnsupported,
    InterfaceNotFound,
    Backend(String),
    Timeout,
    InvalidDpi {
        requested: u16,
        minimum: u16,
        maximum: u16,
        step: Option<u16>,
    },
    InvalidResponse(String),
    Device {
        code: u8,
        description: &'static str,
    },
}

impl fmt::Display for HidppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlatformUnsupported => formatter.write_str("HID++ 探测当前仅支持 Windows"),
            Self::InterfaceNotFound => formatter.write_str("未找到匹配的 G102 HID++ 短/长报告接口"),
            Self::Backend(message) => write!(formatter, "HID 后端错误：{message}"),
            Self::Timeout => formatter.write_str("等待 HID++ 响应超时"),
            Self::InvalidDpi {
                requested,
                minimum,
                maximum,
                step,
            } => match step {
                Some(step) => write!(
                    formatter,
                    "DPI {requested} 不受支持；有效范围为 {minimum}–{maximum}，步进 {step}"
                ),
                None => write!(
                    formatter,
                    "DPI {requested} 不在设备公布的离散值范围 {minimum}–{maximum} 内"
                ),
            },
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

#[cfg(windows)]
pub fn set_first_g102_dpi(
    requested: u16,
    protocol_trace: bool,
) -> Result<DpiWriteResult, HidppError> {
    let mut transport = WindowsHidppTransport::open(protocol_trace)?;
    let probe = probe(&mut transport)?;
    let feature = probe
        .features
        .iter()
        .find(|feature| feature.id == ADJUSTABLE_DPI_ID)
        .ok_or_else(|| {
            HidppError::InvalidResponse("设备未公开 ADJUSTABLE_DPI (0x2201)".to_owned())
        })?;
    let sensor = probe
        .dpi_sensors
        .first()
        .ok_or_else(|| HidppError::InvalidResponse("设备没有 DPI 传感器".to_owned()))?;
    validate_requested_dpi(sensor, requested)?;

    set_sensor_dpi(&mut transport, feature.index, sensor.index, requested)?;
    let after = read_sensor_dpi(&mut transport, feature.index, sensor.index)?.0;
    if after != requested {
        return Err(HidppError::InvalidResponse(format!(
            "DPI 写后回读不一致：请求 {requested}，设备返回 {after}"
        )));
    }
    Ok(DpiWriteResult {
        sensor_index: sensor.index,
        before: sensor.current,
        requested,
        after,
    })
}

#[cfg(not(windows))]
pub fn probe_first_g102(_protocol_trace: bool) -> Result<HidppProbeResult, HidppError> {
    Err(HidppError::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn set_first_g102_dpi(
    _requested: u16,
    _protocol_trace: bool,
) -> Result<DpiWriteResult, HidppError> {
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
    let onboard_feature_index = features
        .iter()
        .find(|feature| feature.id == ONBOARD_PROFILES_ID)
        .map(|feature| feature.index);
    let onboard_profiles = match onboard_feature_index {
        Some(index) => Some(read_onboard_profiles(transport, index)?),
        None => None,
    };
    let onboard_profile = match (onboard_feature_index, &onboard_profiles) {
        (Some(index), Some(info)) => read_first_onboard_profile(transport, index, info)?,
        _ => None,
    };
    Ok(HidppProbeResult {
        protocol_major,
        protocol_minor,
        features,
        dpi_sensors,
        onboard_profiles,
        onboard_profile,
    })
}

fn read_onboard_profiles(
    transport: &mut impl Transport,
    feature_index: u8,
) -> Result<OnboardProfilesInfo, HidppError> {
    let description_response = request(transport, feature_index, 0, [0, 0, 0])?;
    let description = parameters(&description_response, 16)?;
    let mode_response = request(transport, feature_index, 2, [0, 0, 0])?;
    let mode = parameters(&mode_response, 1)?[0];
    let current_response = request(transport, feature_index, 4, [0, 0, 0])?;
    let current = parameters(&current_response, 2)?;

    parse_onboard_profiles(description, mode, current)
}

fn parse_onboard_profiles(
    description: &[u8],
    mode: u8,
    current: &[u8],
) -> Result<OnboardProfilesInfo, HidppError> {
    if description.len() < 16 || current.len() < 2 {
        return Err(HidppError::InvalidResponse(
            "板载配置响应长度不足".to_owned(),
        ));
    }
    Ok(OnboardProfilesInfo {
        memory_model_id: description[0],
        profile_format_id: description[1],
        macro_format_id: description[2],
        profile_count: description[3],
        rom_profile_count: description[4],
        button_count: description[5],
        sector_count: description[6],
        sector_size: u16::from_be_bytes([description[7], description[8]]),
        mechanical_layout: description[9],
        various_info: description[10],
        mode: mode.into(),
        current_profile: current[1],
    })
}

fn read_first_onboard_profile(
    transport: &mut impl Transport,
    feature_index: u8,
    info: &OnboardProfilesInfo,
) -> Result<Option<OnboardProfileSnapshot>, HidppError> {
    if info.profile_count == 0 {
        return Ok(None);
    }
    let directory = read_onboard_sector(transport, feature_index, 0, info.sector_size)?;
    validate_sector_crc(&directory)?;
    let address = u16::from_be_bytes([directory[0], directory[1]]);
    if address == 0xffff {
        return Ok(None);
    }
    if address >= u16::from(info.sector_count) {
        return Err(HidppError::InvalidResponse(format!(
            "板载配置地址 0x{address:04x} 超出 {} 个扇区",
            info.sector_count
        )));
    }
    let enabled = directory[2] != 0;
    let profile = read_onboard_sector(transport, feature_index, address, info.sector_size)?;
    validate_sector_crc(&profile)?;
    parse_onboard_profile(address, enabled, &profile, info.button_count)
}

fn read_onboard_sector(
    transport: &mut impl Transport,
    feature_index: u8,
    sector: u16,
    sector_size: u16,
) -> Result<Vec<u8>, HidppError> {
    if !(18..=4096).contains(&sector_size) {
        return Err(HidppError::InvalidResponse(format!(
            "板载扇区大小 {sector_size} 超出安全范围 18–4096"
        )));
    }
    let size = usize::from(sector_size);
    let mut data = vec![0_u8; size];
    for logical_offset in (0..size).step_by(16) {
        let read_offset = logical_offset.min(size - 16);
        let [sector_high, sector_low] = sector.to_be_bytes();
        let [offset_high, offset_low] = u16::try_from(read_offset)
            .map_err(|_| HidppError::InvalidResponse("板载扇区偏移溢出".to_owned()))?
            .to_be_bytes();
        let mut request_parameters = [0_u8; 16];
        request_parameters[..4].copy_from_slice(&[
            sector_high,
            sector_low,
            offset_high,
            offset_low,
        ]);
        let response = request_long(transport, feature_index, 5, request_parameters)?;
        let response_parameters = parameters(&response, 16)?;
        data[read_offset..read_offset + 16].copy_from_slice(&response_parameters[..16]);
    }
    Ok(data)
}

fn validate_sector_crc(sector: &[u8]) -> Result<(), HidppError> {
    if sector.len() < 2 {
        return Err(HidppError::InvalidResponse("板载扇区过短".to_owned()));
    }
    let payload_length = sector.len() - 2;
    let expected = u16::from_be_bytes([sector[payload_length], sector[payload_length + 1]]);
    let actual = crc_ccitt(&sector[..payload_length]);
    if actual != expected {
        return Err(HidppError::InvalidResponse(format!(
            "板载扇区 CRC 不匹配：设备 0x{expected:04x}，计算 0x{actual:04x}"
        )));
    }
    Ok(())
}

fn crc_ccitt(data: &[u8]) -> u16 {
    let mut crc = 0xffff_u16;
    for byte in data {
        let temp = (crc >> 8) ^ u16::from(*byte);
        crc <<= 8;
        let mut quick = temp ^ (temp >> 4);
        crc ^= quick;
        quick <<= 5;
        crc ^= quick;
        quick <<= 7;
        crc ^= quick;
    }
    crc
}

fn parse_onboard_profile(
    address: u16,
    enabled: bool,
    sector: &[u8],
    button_count: u8,
) -> Result<Option<OnboardProfileSnapshot>, HidppError> {
    let button_count = usize::from(button_count);
    if button_count > 16 || sector.len() < 32 + button_count * 4 + 2 {
        return Err(HidppError::InvalidResponse(
            "板载配置的按键区域超出已知格式".to_owned(),
        ));
    }
    let report_rate_divisor = u16::from(sector[0]).max(1);
    let dpi_slots = sector[3..13]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .filter(|dpi| !matches!(*dpi, 0 | 0xffff))
        .collect();
    let buttons = sector[32..32 + button_count * 4]
        .chunks_exact(4)
        .map(|raw| parse_onboard_button([raw[0], raw[1], raw[2], raw[3]]))
        .collect();

    Ok(Some(OnboardProfileSnapshot {
        address,
        enabled,
        report_rate_hz: 1000 / report_rate_divisor,
        default_dpi_index: sector[1],
        shifted_dpi_index: sector[2],
        dpi_slots,
        buttons,
    }))
}

fn parse_onboard_button(raw: [u8; 4]) -> OnboardButtonAction {
    match (raw[0], raw[1]) {
        (0x00, _) => OnboardButtonAction::Macro {
            page: raw[1],
            offset: raw[3],
        },
        (0x80, 0x01) => {
            let mask = u16::from_be_bytes([raw[2], raw[3]]);
            let button = (mask != 0).then(|| u8::try_from(mask.trailing_zeros() + 1).unwrap_or(0));
            OnboardButtonAction::Mouse { button, mask }
        }
        (0x80, 0x02) => OnboardButtonAction::Keyboard {
            modifiers: raw[2],
            key: raw[3],
        },
        (0x80, 0x03) => OnboardButtonAction::ConsumerControl {
            usage: u16::from_be_bytes([raw[2], raw[3]]),
        },
        (0x90, _) => OnboardButtonAction::Special {
            code: raw[1],
            profile: raw[3],
        },
        (0xff, _) => OnboardButtonAction::Disabled,
        _ => OnboardButtonAction::Unknown(raw),
    }
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

        let (current, default) = read_sensor_dpi(transport, feature_index, index)?;
        sensors.push(DpiSensorInfo {
            index,
            minimum,
            maximum,
            step,
            discrete_values,
            current,
            default,
        });
    }
    Ok(sensors)
}

fn read_sensor_dpi(
    transport: &mut impl Transport,
    feature_index: u8,
    sensor_index: u8,
) -> Result<(u16, u16), HidppError> {
    let response = request(transport, feature_index, 2, [sensor_index, 0, 0])?;
    let response_parameters = parameters(&response, 5)?;
    if response_parameters[0] != sensor_index {
        return Err(HidppError::InvalidResponse(format!(
            "DPI 传感器索引不匹配：请求 {sensor_index}，响应 {}",
            response_parameters[0]
        )));
    }
    Ok((
        u16::from_be_bytes([response_parameters[1], response_parameters[2]]),
        u16::from_be_bytes([response_parameters[3], response_parameters[4]]),
    ))
}

fn set_sensor_dpi(
    transport: &mut impl Transport,
    feature_index: u8,
    sensor_index: u8,
    dpi: u16,
) -> Result<(), HidppError> {
    let [high, low] = dpi.to_be_bytes();
    request(transport, feature_index, 3, [sensor_index, high, low])?;
    Ok(())
}

fn validate_requested_dpi(sensor: &DpiSensorInfo, requested: u16) -> Result<(), HidppError> {
    let in_range = (sensor.minimum..=sensor.maximum).contains(&requested);
    let aligned = match sensor.step {
        Some(step) if step > 0 => requested
            .checked_sub(sensor.minimum)
            .is_some_and(|delta| delta % step == 0),
        Some(_) => false,
        None => sensor.discrete_values.contains(&requested),
    };
    if in_range && aligned {
        Ok(())
    } else {
        Err(HidppError::InvalidDpi {
            requested,
            minimum: sensor.minimum,
            maximum: sensor.maximum,
            step: sensor.step,
        })
    }
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

fn request_long(
    transport: &mut impl Transport,
    feature_index: u8,
    function: u8,
    parameters: [u8; 16],
) -> Result<Vec<u8>, HidppError> {
    if function > 0x0f {
        return Err(HidppError::InvalidResponse(
            "HID++ 函数号超出 4 位范围".to_owned(),
        ));
    }
    let mut request = [0_u8; 20];
    request[..4].copy_from_slice(&[
        REPORT_ID_LONG,
        WIRED_DEVICE_INDEX,
        feature_index,
        (function << 4) | SOFTWARE_ID,
    ]);
    request[4..].copy_from_slice(&parameters);
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
        let device = match request.first() {
            Some(&REPORT_ID_SHORT) => &self.short,
            Some(&REPORT_ID_LONG) => &self.long,
            _ => {
                return Err(HidppError::Backend(
                    "拒绝发送未知 HID++ 报告类型".to_owned(),
                ));
            }
        };
        let written = device
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
    use std::time::Duration;

    use super::{
        DpiSensorInfo, HidppError, OnboardButtonAction, OnboardMode, Transport, crc_ccitt,
        parse_dpi_list, parse_onboard_profile, parse_onboard_profiles, request_long,
        response_matches, set_sensor_dpi, validate_requested_dpi, validate_response,
    };

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

    #[test]
    fn parses_onboard_profile_capabilities_from_fixture() {
        let description = fixture_frame("onboard_description_response");
        let mode = fixture_frame("onboard_mode_response");
        let current = fixture_frame("onboard_current_profile_response");
        let info = parse_onboard_profiles(&description[4..], mode[4], &current[4..]).unwrap();

        assert_eq!(info.mode, OnboardMode::Host);
        assert_eq!(info.profile_count, 1);
        assert_eq!(info.rom_profile_count, 1);
        assert_eq!(info.button_count, 6);
        assert_eq!(info.sector_count, 16);
        assert_eq!(info.sector_size, 255);
        assert_eq!(info.current_profile, 0);
    }

    #[test]
    fn validates_requested_dpi_against_runtime_range() {
        let sensor = DpiSensorInfo {
            index: 0,
            minimum: 50,
            maximum: 8000,
            step: Some(50),
            discrete_values: vec![50, 8000],
            current: 3200,
            default: 800,
        };
        assert!(validate_requested_dpi(&sensor, 800).is_ok());
        assert!(matches!(
            validate_requested_dpi(&sensor, 25),
            Err(HidppError::InvalidDpi { .. })
        ));
        assert!(matches!(
            validate_requested_dpi(&sensor, 825),
            Err(HidppError::InvalidDpi { .. })
        ));
    }

    #[test]
    fn encodes_set_dpi_as_function_three() {
        let mut transport = RecordingTransport {
            response: vec![
                0x11, 0xff, 0x0a, 0x38, 0x00, 0x03, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            request: Vec::new(),
        };
        set_sensor_dpi(&mut transport, 0x0a, 0, 800).unwrap();
        assert_eq!(
            transport.request,
            [0x10, 0xff, 0x0a, 0x38, 0x00, 0x03, 0x20]
        );
    }

    #[test]
    fn crc_matches_ccitt_false_check_value() {
        assert_eq!(crc_ccitt(b"123456789"), 0x29b1);
    }

    #[test]
    fn encodes_memory_read_as_long_report() {
        let mut transport = RecordingTransport {
            response: vec![
                0x11, 0xff, 0x0f, 0x58, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            request: Vec::new(),
        };
        let mut parameters = [0_u8; 16];
        parameters[..4].copy_from_slice(&[0x00, 0x01, 0x00, 0x10]);
        request_long(&mut transport, 0x0f, 5, parameters).unwrap();

        assert_eq!(transport.request[0], 0x11);
        assert_eq!(&transport.request[1..8], &[0xff, 0x0f, 0x58, 0, 1, 0, 0x10]);
        assert_eq!(transport.request.len(), 20);
    }

    #[test]
    fn parses_profile_dpi_slots_and_button_actions() {
        let mut sector = vec![0_u8; 255];
        sector[0] = 1;
        sector[1] = 1;
        sector[2] = 2;
        for (offset, dpi) in [400_u16, 800, 1600, 3200, 0].into_iter().enumerate() {
            let start = 3 + offset * 2;
            sector[start..start + 2].copy_from_slice(&dpi.to_le_bytes());
        }
        sector[32..36].copy_from_slice(&[0x80, 0x01, 0x00, 0x01]);
        sector[36..40].copy_from_slice(&[0x80, 0x02, 0x02, 0x04]);
        sector[40..44].copy_from_slice(&[0x80, 0x03, 0x00, 0xe9]);
        sector[44..48].copy_from_slice(&[0x90, 0x05, 0x00, 0x00]);
        sector[48..52].copy_from_slice(&[0xff, 0, 0, 0]);
        sector[52..56].copy_from_slice(&[0x00, 0x03, 0, 0x20]);

        let profile = parse_onboard_profile(1, true, &sector, 6).unwrap().unwrap();
        assert_eq!(profile.report_rate_hz, 1000);
        assert_eq!(profile.dpi_slots, [400, 800, 1600, 3200]);
        assert_eq!(
            profile.buttons[0],
            OnboardButtonAction::Mouse {
                button: Some(1),
                mask: 1
            }
        );
        assert!(matches!(
            profile.buttons[3],
            OnboardButtonAction::Special { code: 5, .. }
        ));
        assert_eq!(profile.buttons[4], OnboardButtonAction::Disabled);
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

    struct RecordingTransport {
        response: Vec<u8>,
        request: Vec<u8>,
    }

    impl Transport for RecordingTransport {
        fn transact(&mut self, request: &[u8], _timeout: Duration) -> Result<Vec<u8>, HidppError> {
            self.request = request.to_vec();
            Ok(self.response.clone())
        }
    }
}
