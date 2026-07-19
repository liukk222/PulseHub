#![forbid(unsafe_code)]

use pulsehub_core::Profile;

pub const CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDocument {
    pub schema_version: u32,
    pub profiles: Vec<Profile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    UnsupportedSchema { found: u32 },
}

impl ConfigDocument {
    pub fn new(profiles: Vec<Profile>) -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            profiles,
        }
    }

    pub fn validate_schema(&self) -> Result<(), ConfigError> {
        if self.schema_version == CONFIG_SCHEMA_VERSION {
            Ok(())
        } else {
            Err(ConfigError::UnsupportedSchema {
                found: self.schema_version,
            })
        }
    }
}
