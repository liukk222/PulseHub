#![forbid(unsafe_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Environment {
    Office,
    Cs2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProfileId(pub Environment);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DpiValues {
    Range { min: u16, max: u16, step: u16 },
    Discrete(Vec<u16>),
}

impl DpiValues {
    pub fn contains(&self, dpi: u16) -> bool {
        match self {
            Self::Range { min, max, step } => {
                dpi >= *min && dpi <= *max && *step != 0 && (dpi - *min).is_multiple_of(*step)
            }
            Self::Discrete(values) => values.contains(&dpi),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PhysicalControlId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LogicalControlId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidKey {
    pub usage_page: u16,
    pub usage: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidConsumerControl {
    pub usage: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingMechanism {
    RuntimeRemap,
    OnboardCommit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ButtonAction {
    LogicalControl(LogicalControlId),
    OnboardKeyboard(HidKey),
    OnboardConsumer(HidConsumerControl),
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCapability {
    pub action: ButtonAction,
    pub mechanism: MappingMechanism,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlCapability {
    pub physical_control: PhysicalControlId,
    pub actions: Vec<ActionCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCapabilities {
    pub device: DeviceIdentity,
    pub dpi_values: DpiValues,
    pub controls: Vec<ControlCapability>,
    pub runtime_dpi: bool,
    pub runtime_button_mapping: bool,
    pub onboard_profile_count: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonMapping {
    pub physical_control: PhysicalControlId,
    pub action: ButtonAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub id: ProfileId,
    pub dpi: u16,
    pub button_mappings: Vec<ButtonMapping>,
}

#[cfg(test)]
mod tests {
    use super::DpiValues;

    #[test]
    fn range_rejects_values_outside_the_step() {
        let values = DpiValues::Range {
            min: 200,
            max: 800,
            step: 100,
        };

        assert!(values.contains(400));
        assert!(!values.contains(450));
        assert!(!values.contains(900));
    }
}
