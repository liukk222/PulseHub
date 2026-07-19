#![forbid(unsafe_code)]

use pulsehub_core::Environment;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyToken {
    pub connection_generation: u64,
    pub invalidation_epoch: u64,
}

pub fn select_environment(executable_name: Option<&str>) -> Environment {
    match executable_name {
        Some(name) if name.eq_ignore_ascii_case("cs2.exe") => Environment::Cs2,
        _ => Environment::Office,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cs2_is_matched_case_insensitively() {
        assert_eq!(select_environment(Some("CS2.EXE")), Environment::Cs2);
        assert_eq!(select_environment(Some("steam.exe")), Environment::Office);
    }
}
