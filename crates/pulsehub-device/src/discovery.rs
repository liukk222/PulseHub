//! HID collection 的只读枚举与报告描述符分析。

use std::collections::BTreeMap;
use std::fmt;

pub const LOGITECH_VENDOR_ID: u16 = 0x046d;
pub const G102_PRODUCT_IDS: &[u16] = &[0xc084, 0xc092, 0xc09d];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReportLengths {
    /// 包含报告 ID 字节的最大输入报告长度。
    pub input: usize,
    /// 包含报告 ID 字节的最大输出报告长度。
    pub output: usize,
    /// 包含报告 ID 字节的最大 Feature 报告长度。
    pub feature: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidCollectionInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub release_number: u16,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial_number: Option<String>,
    pub usage_page: u16,
    pub usage: u16,
    pub interface_number: i32,
    pub bus_type: String,
    pub report_descriptor_length: Option<usize>,
    pub report_lengths: Option<ReportLengths>,
    pub open_error: Option<String>,
}

impl HidCollectionInfo {
    #[must_use]
    pub fn is_logitech(&self) -> bool {
        self.vendor_id == LOGITECH_VENDOR_ID
    }

    #[must_use]
    pub fn is_known_g102(&self) -> bool {
        self.is_logitech() && G102_PRODUCT_IDS.contains(&self.product_id)
    }

    #[must_use]
    pub fn is_vendor_defined(&self) -> bool {
        (0xff00..=0xffff).contains(&self.usage_page)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryError {
    PlatformUnsupported,
    Backend(String),
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlatformUnsupported => formatter.write_str("HID 枚举当前仅支持 Windows"),
            Self::Backend(message) => write!(formatter, "HID 后端错误：{message}"),
        }
    }
}

impl std::error::Error for DiscoveryError {}

#[cfg(windows)]
pub fn enumerate_hid_collections(
    include_all: bool,
) -> Result<Vec<HidCollectionInfo>, DiscoveryError> {
    use hidapi::{HidApi, MAX_REPORT_DESCRIPTOR_SIZE};

    let api = HidApi::new().map_err(|error| DiscoveryError::Backend(error.to_string()))?;
    let mut collections = Vec::new();

    for device_info in api.device_list() {
        if !include_all && device_info.vendor_id() != LOGITECH_VENDOR_ID {
            continue;
        }

        let mut collection = HidCollectionInfo {
            vendor_id: device_info.vendor_id(),
            product_id: device_info.product_id(),
            release_number: device_info.release_number(),
            manufacturer: device_info.manufacturer_string().map(str::to_owned),
            product: device_info.product_string().map(str::to_owned),
            serial_number: device_info.serial_number().map(str::to_owned),
            usage_page: device_info.usage_page(),
            usage: device_info.usage(),
            interface_number: device_info.interface_number(),
            bus_type: format!("{:?}", device_info.bus_type()),
            report_descriptor_length: None,
            report_lengths: None,
            open_error: None,
        };

        match device_info.open_device(&api) {
            Ok(device) => {
                let mut descriptor = vec![0_u8; MAX_REPORT_DESCRIPTOR_SIZE];
                match device.get_report_descriptor(&mut descriptor) {
                    Ok(length) => {
                        descriptor.truncate(length);
                        collection.report_descriptor_length = Some(length);
                        match parse_report_lengths(&descriptor) {
                            Ok(lengths) => collection.report_lengths = Some(lengths),
                            Err(error) => collection.open_error = Some(error),
                        }
                    }
                    Err(error) => collection.open_error = Some(error.to_string()),
                }
            }
            Err(error) => collection.open_error = Some(error.to_string()),
        }

        collections.push(collection);
    }

    collections.sort_by_key(|item| {
        (
            item.vendor_id,
            item.product_id,
            item.interface_number,
            item.usage_page,
            item.usage,
        )
    });
    Ok(collections)
}

#[cfg(not(windows))]
pub fn enumerate_hid_collections(
    _include_all: bool,
) -> Result<Vec<HidCollectionInfo>, DiscoveryError> {
    Err(DiscoveryError::PlatformUnsupported)
}

#[derive(Debug, Clone, Copy, Default)]
struct GlobalState {
    report_size: u32,
    report_count: u32,
    report_id: u8,
}

#[derive(Debug, Clone, Copy)]
enum ReportKind {
    Input,
    Output,
    Feature,
}

/// 从 HID 报告描述符计算各类报告的最大缓冲区长度。
///
/// HID API 的缓冲区始终预留首字节作为报告 ID；设备未使用编号报告时该字节为 0。
pub fn parse_report_lengths(descriptor: &[u8]) -> Result<ReportLengths, String> {
    let mut cursor = 0_usize;
    let mut state = GlobalState::default();
    let mut stack = Vec::new();
    let mut input_bits = BTreeMap::<u8, u64>::new();
    let mut output_bits = BTreeMap::<u8, u64>::new();
    let mut feature_bits = BTreeMap::<u8, u64>::new();

    while cursor < descriptor.len() {
        let prefix = descriptor[cursor];
        cursor += 1;

        if prefix == 0xfe {
            if cursor + 2 > descriptor.len() {
                return Err("截断的 HID 长项头".to_owned());
            }
            let size = usize::from(descriptor[cursor]);
            cursor += 2;
            if cursor + size > descriptor.len() {
                return Err("截断的 HID 长项数据".to_owned());
            }
            cursor += size;
            continue;
        }

        let data_size = match prefix & 0x03 {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 4,
            _ => unreachable!(),
        };
        if cursor + data_size > descriptor.len() {
            return Err("截断的 HID 短项数据".to_owned());
        }
        let value = read_unsigned(&descriptor[cursor..cursor + data_size]);
        cursor += data_size;

        let item_type = (prefix >> 2) & 0x03;
        let tag = (prefix >> 4) & 0x0f;
        match (item_type, tag) {
            (1, 7) => state.report_size = value,
            (1, 8) => {
                state.report_id =
                    u8::try_from(value).map_err(|_| "HID Report ID 超出 u8 范围".to_owned())?;
                if state.report_id == 0 {
                    return Err("HID 描述符包含非法的 Report ID 0".to_owned());
                }
            }
            (1, 9) => state.report_count = value,
            (1, 10) => stack.push(state),
            (1, 11) => {
                state = stack.pop().ok_or_else(|| "HID 全局状态栈下溢".to_owned())?;
            }
            (0, 8) => add_report_bits(&mut input_bits, state, ReportKind::Input)?,
            (0, 9) => add_report_bits(&mut output_bits, state, ReportKind::Output)?,
            (0, 11) => add_report_bits(&mut feature_bits, state, ReportKind::Feature)?,
            _ => {}
        }
    }

    if !stack.is_empty() {
        return Err("HID 全局状态栈未闭合".to_owned());
    }

    Ok(ReportLengths {
        input: max_report_bytes(&input_bits),
        output: max_report_bytes(&output_bits),
        feature: max_report_bytes(&feature_bits),
    })
}

fn read_unsigned(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .enumerate()
        .fold(0_u32, |value, (offset, byte)| {
            value | (u32::from(*byte) << (offset * 8))
        })
}

fn add_report_bits(
    reports: &mut BTreeMap<u8, u64>,
    state: GlobalState,
    kind: ReportKind,
) -> Result<(), String> {
    let bits = u64::from(state.report_size)
        .checked_mul(u64::from(state.report_count))
        .ok_or_else(|| format!("{kind:?} 报告位数溢出"))?;
    let total = reports.entry(state.report_id).or_default();
    *total = total
        .checked_add(bits)
        .ok_or_else(|| format!("{kind:?} 报告累计位数溢出"))?;
    Ok(())
}

fn max_report_bytes(reports: &BTreeMap<u8, u64>) -> usize {
    reports
        .values()
        .copied()
        .max()
        .map_or(0, |bits| 1 + bits.div_ceil(8) as usize)
}

#[cfg(test)]
mod tests {
    use super::{ReportLengths, parse_report_lengths};

    #[test]
    fn parses_numbered_input_output_and_feature_reports() {
        let descriptor = [
            0x85, 0x01, // Report ID 1
            0x75, 0x08, // Report Size 8
            0x95, 0x03, // Report Count 3
            0x81, 0x02, // Input
            0x95, 0x02, // Report Count 2
            0x91, 0x02, // Output
            0x85, 0x02, // Report ID 2
            0x75, 0x04, // Report Size 4
            0x95, 0x03, // Report Count 3
            0xb1, 0x02, // Feature
        ];

        assert_eq!(
            parse_report_lengths(&descriptor),
            Ok(ReportLengths {
                input: 4,
                output: 3,
                feature: 3,
            })
        );
    }

    #[test]
    fn accumulates_fields_for_the_same_report_id() {
        let descriptor = [
            0x75, 0x01, 0x95, 0x03, 0x81, 0x02, // 3 bits
            0x75, 0x01, 0x95, 0x05, 0x81, 0x03, // 5 bits padding
        ];

        assert_eq!(parse_report_lengths(&descriptor).unwrap().input, 2);
    }

    #[test]
    fn rejects_truncated_items() {
        assert!(parse_report_lengths(&[0x76, 0x08]).is_err());
        assert!(parse_report_lengths(&[0xfe, 0x04, 0x00, 0x01]).is_err());
    }
}
