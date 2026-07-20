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

/// G102 LIGHTSYNC 的六个物理槽位，顺序与 profile format 0x04 一致。
pub const G102_BUTTON_NAMES: [&str; 6] = [
    "左键",
    "右键",
    "滚轮键（中键）",
    "侧键（后，G4）",
    "侧键（前，G5）",
    "DPI 切换键（G6）",
];

/// 用户确认的办公环境目标映射。该函数只构造内存模型，不执行设备 I/O。
pub fn g102_office_button_actions() -> [OnboardButtonAction; 6] {
    [
        OnboardButtonAction::Mouse {
            button: Some(1),
            mask: 0x0001,
        },
        OnboardButtonAction::Mouse {
            button: Some(2),
            mask: 0x0002,
        },
        OnboardButtonAction::Keyboard {
            modifiers: 0x00,
            key: 0x2a,
        },
        OnboardButtonAction::Keyboard {
            modifiers: 0x01,
            key: 0x19,
        },
        OnboardButtonAction::Keyboard {
            modifiers: 0x01,
            key: 0x06,
        },
        OnboardButtonAction::Keyboard {
            modifiers: 0x01,
            key: 0x04,
        },
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DpiWriteResult {
    pub sensor_index: u8,
    pub before: u16,
    pub requested: u16,
    pub after: u16,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardWriteResult {
    pub profile_sector: u16,
    pub changed_buttons: Vec<usize>,
    pub dpi_changed: bool,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardModeWriteResult {
    pub before: OnboardMode,
    pub after: OnboardMode,
}

#[derive(Debug, Clone, Copy)]
enum ProfileDpiSettings<'a> {
    Single(u16),
    Levels { dpi: u16, levels: &'a [u16; 4] },
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
pub fn read_first_g102_dpi(protocol_trace: bool) -> Result<u16, HidppError> {
    let mut transport = WindowsHidppTransport::open(protocol_trace)?;
    let feature = get_feature(&mut transport, ADJUSTABLE_DPI_ID)?;
    if feature.index == 0 {
        return Err(HidppError::InvalidResponse(
            "设备未公开 ADJUSTABLE_DPI (0x2201)".to_owned(),
        ));
    }
    let count_response = request(&mut transport, feature.index, 0, [0, 0, 0])?;
    if parameters(&count_response, 1)?[0] == 0 {
        return Err(HidppError::InvalidResponse(
            "设备没有 DPI 传感器".to_owned(),
        ));
    }
    read_sensor_dpi(&mut transport, feature.index, 0).map(|(current, _)| current)
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

    if sensor.current == requested {
        return Ok(DpiWriteResult {
            sensor_index: sensor.index,
            before: sensor.current,
            requested,
            after: sensor.current,
            changed: false,
        });
    }

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
        changed: true,
    })
}

#[cfg(windows)]
pub fn apply_first_g102_office_buttons(
    protocol_trace: bool,
) -> Result<OnboardWriteResult, HidppError> {
    apply_first_g102_button_actions(&g102_office_button_actions(), protocol_trace)
}

#[cfg(windows)]
pub fn apply_first_g102_button_actions(
    actions: &[OnboardButtonAction; 6],
    protocol_trace: bool,
) -> Result<OnboardWriteResult, HidppError> {
    let mut transport = WindowsHidppTransport::open(protocol_trace)?;
    if transport.product_id != 0xc092 || transport.release_number != 0x5200 {
        return Err(HidppError::InvalidResponse(format!(
            "拒绝板载写入：仅验证过 046d:c092 release=5200，当前为 046d:{:04x} release={:04x}",
            transport.product_id, transport.release_number
        )));
    }
    apply_profile_settings(&mut transport, actions, None)
}

#[cfg(windows)]
pub fn apply_first_g102_profile(
    actions: &[OnboardButtonAction; 6],
    dpi: u16,
    dpi_levels: Option<&[u16; 4]>,
    protocol_trace: bool,
) -> Result<OnboardWriteResult, HidppError> {
    let mut transport = WindowsHidppTransport::open(protocol_trace)?;
    if transport.product_id != 0xc092 || transport.release_number != 0x5200 {
        return Err(HidppError::InvalidResponse(format!(
            "拒绝板载写入：仅验证过 046d:c092 release=5200，当前为 046d:{:04x} release={:04x}",
            transport.product_id, transport.release_number
        )));
    }
    let dpi = dpi_levels.map_or(ProfileDpiSettings::Single(dpi), |levels| {
        ProfileDpiSettings::Levels { dpi, levels }
    });
    apply_profile_settings(&mut transport, actions, Some(dpi))
}

#[cfg(windows)]
pub fn activate_first_g102_onboard_mode(
    protocol_trace: bool,
) -> Result<OnboardModeWriteResult, HidppError> {
    let mut transport = WindowsHidppTransport::open(protocol_trace)?;
    if transport.product_id != 0xc092 || transport.release_number != 0x5200 {
        return Err(HidppError::InvalidResponse(format!(
            "拒绝模式切换：仅验证过 046d:c092 release=5200，当前为 046d:{:04x} release={:04x}",
            transport.product_id, transport.release_number
        )));
    }
    let feature = get_feature(&mut transport, ONBOARD_PROFILES_ID)?;
    if feature.index == 0 {
        return Err(HidppError::InvalidResponse(
            "设备未公开 ONBOARD_PROFILES (0x8100)".to_owned(),
        ));
    }
    let before = read_onboard_mode(&mut transport, feature.index)?;
    if before != OnboardMode::Onboard {
        set_onboard_mode(&mut transport, feature.index, OnboardMode::Onboard)?;
    }
    let after = read_onboard_mode(&mut transport, feature.index)?;
    if after != OnboardMode::Onboard {
        return Err(HidppError::InvalidResponse(format!(
            "板载模式写后回读不一致：设备返回 {after:?}"
        )));
    }
    Ok(OnboardModeWriteResult { before, after })
}

#[cfg(not(windows))]
pub fn probe_first_g102(_protocol_trace: bool) -> Result<HidppProbeResult, HidppError> {
    Err(HidppError::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn read_first_g102_dpi(_protocol_trace: bool) -> Result<u16, HidppError> {
    Err(HidppError::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn set_first_g102_dpi(
    _requested: u16,
    _protocol_trace: bool,
) -> Result<DpiWriteResult, HidppError> {
    Err(HidppError::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn apply_first_g102_office_buttons(
    _protocol_trace: bool,
) -> Result<OnboardWriteResult, HidppError> {
    Err(HidppError::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn apply_first_g102_button_actions(
    _actions: &[OnboardButtonAction; 6],
    _protocol_trace: bool,
) -> Result<OnboardWriteResult, HidppError> {
    Err(HidppError::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn apply_first_g102_profile(
    _actions: &[OnboardButtonAction; 6],
    _dpi: u16,
    _dpi_levels: Option<&[u16; 4]>,
    _protocol_trace: bool,
) -> Result<OnboardWriteResult, HidppError> {
    Err(HidppError::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn activate_first_g102_onboard_mode(
    _protocol_trace: bool,
) -> Result<OnboardModeWriteResult, HidppError> {
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

fn read_onboard_mode(
    transport: &mut impl Transport,
    feature_index: u8,
) -> Result<OnboardMode, HidppError> {
    let response = request(transport, feature_index, 2, [0, 0, 0])?;
    Ok(parameters(&response, 1)?[0].into())
}

fn set_onboard_mode(
    transport: &mut impl Transport,
    feature_index: u8,
    mode: OnboardMode,
) -> Result<(), HidppError> {
    let value = match mode {
        OnboardMode::NoChange => 0,
        OnboardMode::Onboard => 1,
        OnboardMode::Host => 2,
        OnboardMode::Unknown(value) => {
            return Err(HidppError::InvalidResponse(format!(
                "拒绝设置未知板载模式 0x{value:02x}"
            )));
        }
    };
    request(transport, feature_index, 1, [value, 0, 0])?;
    Ok(())
}

fn apply_profile_settings(
    transport: &mut impl Transport,
    actions: &[OnboardButtonAction; 6],
    dpi: Option<ProfileDpiSettings<'_>>,
) -> Result<OnboardWriteResult, HidppError> {
    let feature = get_feature(transport, ONBOARD_PROFILES_ID)?;
    if feature.index == 0 {
        return Err(HidppError::InvalidResponse(
            "设备未公开 ONBOARD_PROFILES (0x8100)".to_owned(),
        ));
    }
    let info = read_onboard_profiles(transport, feature.index)?;
    if info.memory_model_id != 0x01
        || info.profile_format_id != 0x04
        || info.profile_count != 1
        || info.button_count != 6
        || info.sector_size != 255
    {
        return Err(HidppError::InvalidResponse(format!(
            "拒绝板载写入：未验证的格式 memory=0x{:02x} profile=0x{:02x} profiles={} buttons={} sector_size={}",
            info.memory_model_id,
            info.profile_format_id,
            info.profile_count,
            info.button_count,
            info.sector_size
        )));
    }

    let directory = read_onboard_sector(transport, feature.index, 0, info.sector_size)?;
    validate_sector_crc(&directory)?;
    let profile_sector = u16::from_be_bytes([directory[0], directory[1]]);
    if profile_sector == 0xffff || profile_sector >= u16::from(info.sector_count) {
        return Err(HidppError::InvalidResponse(format!(
            "拒绝板载写入：配置扇区地址 0x{profile_sector:04x} 无效"
        )));
    }

    let original = read_onboard_sector(transport, feature.index, profile_sector, info.sector_size)?;
    let (desired, changed_buttons, dpi_changed) =
        build_profile_sector(&original, info.button_count, actions, dpi)?;
    if changed_buttons.is_empty() && !dpi_changed {
        return Ok(OnboardWriteResult {
            profile_sector,
            changed_buttons,
            dpi_changed,
            verified: true,
        });
    }

    if let Err(write_error) = write_onboard_sector(
        transport,
        feature.index,
        profile_sector,
        info.sector_size,
        &desired,
    ) {
        let recovery = restore_onboard_sector(
            transport,
            feature.index,
            profile_sector,
            info.sector_size,
            &original,
        );
        return Err(write_failure_with_recovery(write_error, recovery));
    }

    let readback =
        match read_onboard_sector(transport, feature.index, profile_sector, info.sector_size) {
            Ok(readback) => readback,
            Err(read_error) => {
                let recovery = restore_onboard_sector(
                    transport,
                    feature.index,
                    profile_sector,
                    info.sector_size,
                    &original,
                );
                return Err(write_failure_with_recovery(read_error, recovery));
            }
        };
    if readback != desired || validate_sector_crc(&readback).is_err() {
        let recovery = restore_onboard_sector(
            transport,
            feature.index,
            profile_sector,
            info.sector_size,
            &original,
        );
        return Err(write_failure_with_recovery(
            HidppError::InvalidResponse("板载配置写后整扇区回读不一致".to_owned()),
            recovery,
        ));
    }

    Ok(OnboardWriteResult {
        profile_sector,
        changed_buttons,
        dpi_changed,
        verified: true,
    })
}

#[cfg(test)]
fn build_office_profile_sector(
    original: &[u8],
    button_count: u8,
) -> Result<(Vec<u8>, Vec<usize>), HidppError> {
    let (sector, buttons, _) =
        build_profile_sector(original, button_count, &g102_office_button_actions(), None)?;
    Ok((sector, buttons))
}

fn build_profile_sector(
    original: &[u8],
    button_count: u8,
    actions: &[OnboardButtonAction; 6],
    dpi: Option<ProfileDpiSettings<'_>>,
) -> Result<(Vec<u8>, Vec<usize>, bool), HidppError> {
    validate_sector_crc(original)?;
    if button_count != 6 || original.len() != 255 {
        return Err(HidppError::InvalidResponse(
            "办公映射只支持 6 按键、255 字节的已验证配置格式".to_owned(),
        ));
    }
    let mut desired = original.to_vec();
    let mut dpi_changed = false;
    if let Some(ProfileDpiSettings::Levels { dpi, levels }) = dpi {
        let default_index = levels
            .iter()
            .position(|level| *level == dpi)
            .ok_or_else(|| {
                HidppError::InvalidResponse(format!(
                    "当前 DPI {dpi} 不在四个切换档位中，请先调整当前 DPI 或档位"
                ))
            })?;
        // The firmware keeps a separate active-slot cursor. Put the requested DPI in
        // slot zero so the runtime DPI and cursor agree and the first G6 press advances.
        let mut slots = [0_u16; 5];
        for (slot, level) in slots[..4].iter_mut().zip(
            levels[default_index..]
                .iter()
                .chain(levels[..default_index].iter()),
        ) {
            *slot = *level;
        }
        for (index, level) in slots.iter().enumerate() {
            let start = 3 + index * 2;
            let encoded = level.to_le_bytes();
            if desired[start..start + 2] != encoded {
                desired[start..start + 2].copy_from_slice(&encoded);
                dpi_changed = true;
            }
        }
        if desired[1] != 0 {
            desired[1] = 0;
            dpi_changed = true;
        }
    } else if let Some(ProfileDpiSettings::Single(dpi)) = dpi {
        let default_index = usize::from(desired[1]);
        if default_index >= 5 {
            return Err(HidppError::InvalidResponse(format!(
                "板载默认 DPI 索引 {default_index} 超出已验证范围"
            )));
        }
        let start = 3 + default_index * 2;
        let encoded = dpi.to_le_bytes();
        if desired[start..start + 2] != encoded {
            desired[start..start + 2].copy_from_slice(&encoded);
            dpi_changed = true;
        }
    }
    let mut changed_buttons = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        let start = 32 + index * 4;
        let encoded = encode_onboard_button(action)?;
        if desired[start..start + 4] != encoded {
            desired[start..start + 4].copy_from_slice(&encoded);
            changed_buttons.push(index);
        }
    }
    let payload_length = desired.len() - 2;
    let crc = crc_ccitt(&desired[..payload_length]);
    desired[payload_length..].copy_from_slice(&crc.to_be_bytes());
    Ok((desired, changed_buttons, dpi_changed))
}

fn write_onboard_sector(
    transport: &mut impl Transport,
    feature_index: u8,
    sector: u16,
    sector_size: u16,
    data: &[u8],
) -> Result<(), HidppError> {
    if data.len() != usize::from(sector_size) {
        return Err(HidppError::InvalidResponse(
            "板载写入数据长度与扇区大小不一致".to_owned(),
        ));
    }
    validate_sector_crc(data)?;

    let mut start = [0_u8; 16];
    start[0..2].copy_from_slice(&sector.to_be_bytes());
    start[2..4].copy_from_slice(&0_u16.to_be_bytes());
    start[4..6].copy_from_slice(&sector_size.to_be_bytes());
    request_long(transport, feature_index, 6, start)?;

    let mut padded = data.to_vec();
    let transfer_size = data.len().next_multiple_of(16);
    padded.resize(transfer_size, 0);
    for chunk in padded.chunks_exact(16) {
        let mut parameters = [0_u8; 16];
        parameters.copy_from_slice(chunk);
        if let Err(error) = request_long(transport, feature_index, 7, parameters) {
            let _ = request(transport, feature_index, 8, [0, 0, 0]);
            return Err(error);
        }
    }
    request(transport, feature_index, 8, [0, 0, 0])?;
    Ok(())
}

fn restore_onboard_sector(
    transport: &mut impl Transport,
    feature_index: u8,
    sector: u16,
    sector_size: u16,
    original: &[u8],
) -> Result<(), HidppError> {
    write_onboard_sector(transport, feature_index, sector, sector_size, original)?;
    let restored = read_onboard_sector(transport, feature_index, sector, sector_size)?;
    if restored == original {
        Ok(())
    } else {
        Err(HidppError::InvalidResponse(
            "原配置恢复后回读不一致".to_owned(),
        ))
    }
}

fn write_failure_with_recovery(
    write_error: HidppError,
    recovery: Result<(), HidppError>,
) -> HidppError {
    match recovery {
        Ok(()) => {
            HidppError::InvalidResponse(format!("板载写入失败，已恢复并验证原配置：{write_error}"))
        }
        Err(recovery_error) => HidppError::InvalidResponse(format!(
            "板载写入失败且原配置恢复未验证；写入错误：{write_error}；恢复错误：{recovery_error}"
        )),
    }
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

/// 将单一按键动作编码为 profile format 0x04 的四字节绑定。
///
/// 此函数不执行设备 I/O，并明确拒绝宏和未知动作。
pub fn encode_onboard_button(action: &OnboardButtonAction) -> Result<[u8; 4], HidppError> {
    match action {
        OnboardButtonAction::Mouse { mask, .. } => {
            let [high, low] = mask.to_be_bytes();
            Ok([0x80, 0x01, high, low])
        }
        OnboardButtonAction::Keyboard { modifiers, key } => Ok([0x80, 0x02, *modifiers, *key]),
        OnboardButtonAction::ConsumerControl { usage } => {
            let [high, low] = usage.to_be_bytes();
            Ok([0x80, 0x03, high, low])
        }
        OnboardButtonAction::Special { code, profile } => Ok([0x90, *code, 0, *profile]),
        OnboardButtonAction::Disabled => Ok([0xff; 4]),
        OnboardButtonAction::Macro { .. } => Err(HidppError::InvalidResponse(
            "安全策略禁止编码宏动作".to_owned(),
        )),
        OnboardButtonAction::Unknown(_) => Err(HidppError::InvalidResponse(
            "不能编码未知按键动作".to_owned(),
        )),
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
    product_id: u16,
    release_number: u16,
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
            product_id: short_info.product_id(),
            release_number: short_info.release_number(),
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
        DpiSensorInfo, HidppError, OnboardButtonAction, OnboardMode, ProfileDpiSettings, Transport,
        build_office_profile_sector, build_profile_sector, crc_ccitt, encode_onboard_button,
        g102_office_button_actions, parse_dpi_list, parse_onboard_profile, parse_onboard_profiles,
        request_long, response_matches, set_onboard_mode, set_sensor_dpi, validate_requested_dpi,
        validate_response, write_onboard_sector,
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
    fn encodes_activate_onboard_mode_as_function_one() {
        let mut transport = RecordingTransport {
            response: vec![0x10, 0xff, 0x0f, 0x18, 0x01, 0x00, 0x00],
            request: Vec::new(),
        };
        set_onboard_mode(&mut transport, 0x0f, OnboardMode::Onboard).unwrap();
        assert_eq!(transport.request, [0x10, 0xff, 0x0f, 0x18, 0x01, 0, 0]);
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

    #[test]
    fn encodes_confirmed_office_button_mapping() {
        let encoded = g102_office_button_actions()
            .iter()
            .map(encode_onboard_button)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(
            encoded,
            [
                [0x80, 0x01, 0x00, 0x01], // 左键
                [0x80, 0x01, 0x00, 0x02], // 右键
                [0x80, 0x02, 0x00, 0x2a], // Backspace
                [0x80, 0x02, 0x01, 0x19], // Ctrl+V，侧后 G4
                [0x80, 0x02, 0x01, 0x06], // Ctrl+C，侧前 G5
                [0x80, 0x02, 0x01, 0x04], // Ctrl+A，DPI G6
            ]
        );

        assert!(encode_onboard_button(&OnboardButtonAction::Macro { page: 1, offset: 2 }).is_err());
    }

    #[test]
    fn patches_only_office_buttons_and_crc() {
        let mut original = vec![0x5a_u8; 255];
        let current = [
            [0x80, 0x01, 0x00, 0x01],
            [0x80, 0x01, 0x00, 0x02],
            [0x80, 0x02, 0x00, 0xe1],
            [0x80, 0x02, 0x01, 0x19],
            [0x80, 0x02, 0x01, 0x06],
            [0x80, 0x02, 0x00, 0x39],
        ];
        for (index, binding) in current.iter().enumerate() {
            let start = 32 + index * 4;
            original[start..start + 4].copy_from_slice(binding);
        }
        let payload_length = original.len() - 2;
        let crc = crc_ccitt(&original[..payload_length]);
        original[payload_length..].copy_from_slice(&crc.to_be_bytes());

        let (desired, changed) = build_office_profile_sector(&original, 6).unwrap();
        assert_eq!(changed, [2, 5]);
        assert_eq!(&desired[40..44], &[0x80, 0x02, 0x00, 0x2a]);
        assert_eq!(&desired[52..56], &[0x80, 0x02, 0x01, 0x04]);
        assert_eq!(&desired[..32], &original[..32]);
        assert_eq!(&desired[56..253], &original[56..253]);
        assert_eq!(
            u16::from_be_bytes([desired[253], desired[254]]),
            crc_ccitt(&desired[..253])
        );
    }

    #[test]
    fn patches_default_dpi_slot_with_buttons_in_one_sector() {
        let mut original = vec![0_u8; 255];
        original[1] = 2;
        original[7..9].copy_from_slice(&800_u16.to_le_bytes());
        for (index, action) in g102_office_button_actions().iter().enumerate() {
            let start = 32 + index * 4;
            original[start..start + 4].copy_from_slice(&encode_onboard_button(action).unwrap());
        }
        let crc = crc_ccitt(&original[..253]);
        original[253..].copy_from_slice(&crc.to_be_bytes());

        let (desired, changed_buttons, dpi_changed) = build_profile_sector(
            &original,
            6,
            &g102_office_button_actions(),
            Some(ProfileDpiSettings::Levels {
                dpi: 3200,
                levels: &[800, 1600, 2400, 3200],
            }),
        )
        .unwrap();

        assert!(dpi_changed);
        assert!(changed_buttons.is_empty());
        assert_eq!(desired[1], 0);
        assert_eq!(
            &desired[3..11],
            &[0x80, 0x0c, 0x20, 0x03, 0x40, 0x06, 0x60, 0x09]
        );
        assert_eq!(&desired[11..13], &[0, 0]);
        assert_eq!(&desired[32..56], &original[32..56]);
        assert_eq!(
            crc_ccitt(&desired[..253]),
            u16::from_be_bytes([desired[253], desired[254]])
        );
    }

    #[test]
    fn writes_255_byte_sector_with_guarded_transaction() {
        let mut sector = vec![0_u8; 255];
        let crc = crc_ccitt(&sector[..253]);
        sector[253..].copy_from_slice(&crc.to_be_bytes());
        let mut transport = EchoTransport::default();

        write_onboard_sector(&mut transport, 0x0f, 1, 255, &sector).unwrap();

        assert_eq!(transport.requests.len(), 18);
        assert_eq!(transport.requests[0][3], 0x68);
        assert_eq!(&transport.requests[0][4..10], &[0, 1, 0, 0, 0, 255]);
        assert!(
            transport.requests[1..17]
                .iter()
                .all(|request| request[0] == 0x11 && request[3] == 0x78)
        );
        assert_eq!(transport.requests[16][19], 0);
        assert_eq!(transport.requests[17][0], 0x10);
        assert_eq!(transport.requests[17][3], 0x88);
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

    #[derive(Default)]
    struct EchoTransport {
        requests: Vec<Vec<u8>>,
    }

    impl Transport for EchoTransport {
        fn transact(&mut self, request: &[u8], _timeout: Duration) -> Result<Vec<u8>, HidppError> {
            self.requests.push(request.to_vec());
            Ok(request.to_vec())
        }
    }
}
