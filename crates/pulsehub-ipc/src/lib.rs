#![forbid(unsafe_code)]

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestType {
    Hello,
    GetSnapshot,
    ValidateDraft,
    CommitConfig,
    ApplyNow,
    SetSelectionMode,
    AttachUi,
}

impl RequestType {
    pub const fn as_wire_name(self) -> &'static str {
        match self {
            Self::Hello => "hello",
            Self::GetSnapshot => "get_snapshot",
            Self::ValidateDraft => "validate_draft",
            Self::CommitConfig => "commit_config",
            Self::ApplyNow => "apply_now",
            Self::SetSelectionMode => "set_selection_mode",
            Self::AttachUi => "attach_ui",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RequestType;

    #[test]
    fn request_names_match_the_documented_wire_format() {
        assert_eq!(RequestType::GetSnapshot.as_wire_name(), "get_snapshot");
    }
}
