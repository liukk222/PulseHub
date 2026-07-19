#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CONFIG_SCHEMA_VERSION: u32 = 1;
pub const CONFIG_FILE_NAME: &str = "config.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub agent: AgentConfig,
    pub selection: SelectionConfig,
    pub profiles: ProfilesConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    pub start_with_windows: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            start_with_windows: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionConfig {
    pub mode: SelectionMode,
    #[serde(default)]
    pub rules: Vec<SelectionRule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    Auto,
    Office,
    Cs2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionRule {
    pub environment: ProfileName,
    pub process_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfilesConfig {
    pub office: ProfileConfig,
    pub cs2: ProfileConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
    pub dpi: u16,
    #[serde(default)]
    pub button_mappings: Vec<ButtonMappingConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonMappingConfig {
    pub physical_control: String,
    pub action: ButtonActionConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ButtonActionConfig {
    LogicalControl {
        value: String,
    },
    OnboardKeyboard {
        usage_page: u16,
        usage: u16,
        #[serde(default)]
        modifiers: u8,
    },
    OnboardConsumer {
        usage: u16,
    },
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileName {
    Office,
    Cs2,
}

#[derive(Debug)]
pub enum ConfigError {
    MissingAppData,
    UnsupportedSchema { found: u32 },
    Validation(String),
    Io { path: PathBuf, source: io::Error },
    Parse { path: PathBuf, message: String },
    Serialize(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingAppData => formatter.write_str("环境变量 APPDATA 不可用"),
            Self::UnsupportedSchema { found } => {
                write!(formatter, "不支持配置 schema {found}")
            }
            Self::Validation(message) => write!(formatter, "配置校验失败：{message}"),
            Self::Io { path, source } => {
                write!(formatter, "配置 I/O 失败 {}：{source}", path.display())
            }
            Self::Parse { path, message } => {
                write!(formatter, "配置解析失败 {}：{message}", path.display())
            }
            Self::Serialize(message) => write!(formatter, "配置序列化失败：{message}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Default for ConfigDocument {
    fn default() -> Self {
        let office_buttons = office_button_mappings();
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            agent: AgentConfig::default(),
            selection: SelectionConfig {
                mode: SelectionMode::Auto,
                rules: vec![SelectionRule {
                    environment: ProfileName::Cs2,
                    process_names: vec!["cs2.exe".to_owned()],
                }],
            },
            profiles: ProfilesConfig {
                office: ProfileConfig {
                    dpi: 1800,
                    button_mappings: office_buttons.clone(),
                },
                cs2: ProfileConfig {
                    dpi: 800,
                    button_mappings: office_buttons,
                },
            },
        }
    }
}

impl ConfigDocument {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchema {
                found: self.schema_version,
            });
        }
        validate_profile("office", &self.profiles.office)?;
        validate_profile("cs2", &self.profiles.cs2)?;
        if self.selection.mode == SelectionMode::Auto && self.selection.rules.is_empty() {
            return Err(ConfigError::Validation(
                "auto 模式至少需要一条进程规则".to_owned(),
            ));
        }
        for rule in &self.selection.rules {
            if rule.process_names.is_empty()
                || rule.process_names.iter().any(|name| {
                    name.trim().is_empty() || !name.to_ascii_lowercase().ends_with(".exe")
                })
            {
                return Err(ConfigError::Validation(
                    "进程规则必须包含非空的 .exe 文件名".to_owned(),
                ));
            }
        }
        Ok(())
    }

    pub fn to_toml(&self) -> Result<String, ConfigError> {
        self.validate()?;
        toml::to_string_pretty(self).map_err(|error| ConfigError::Serialize(error.to_string()))
    }

    pub fn from_toml(path: &Path, text: &str) -> Result<Self, ConfigError> {
        let document: Self = toml::from_str(text).map_err(|error| ConfigError::Parse {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        document.validate()?;
        Ok(document)
    }
}

pub fn default_config_path() -> Result<PathBuf, ConfigError> {
    let app_data = env::var_os("APPDATA").ok_or(ConfigError::MissingAppData)?;
    Ok(PathBuf::from(app_data)
        .join("PulseHub")
        .join(CONFIG_FILE_NAME))
}

pub fn load_with_backup(path: &Path) -> Result<ConfigDocument, ConfigError> {
    match load_one(path) {
        Ok(document) => Ok(document),
        Err(primary_error) => {
            let backup = backup_path(path);
            load_one(&backup).map_err(|backup_error| {
                ConfigError::Validation(format!(
                    "主配置不可用（{primary_error}），备份也不可用（{backup_error}）"
                ))
            })
        }
    }
}

pub fn load_or_create_default(path: &Path) -> Result<ConfigDocument, ConfigError> {
    if path.exists() || backup_path(path).exists() {
        load_with_backup(path)
    } else {
        let document = ConfigDocument::default();
        save_atomic(path, &document)?;
        Ok(document)
    }
}

pub fn save_atomic(path: &Path, document: &ConfigDocument) -> Result<(), ConfigError> {
    let text = document.to_toml()?;
    let parent = path
        .parent()
        .ok_or_else(|| ConfigError::Validation("配置路径必须包含父目录".to_owned()))?;
    fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
    if path.exists() {
        let backup = backup_path(path);
        fs::copy(path, &backup).map_err(|source| io_error(&backup, source))?;
    }

    let mut temporary = tempfile::Builder::new()
        .prefix("config-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(|source| io_error(parent, source))?;
    temporary
        .write_all(text.as_bytes())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| io_error(temporary.path(), source))?;
    temporary
        .persist(path)
        .map_err(|error| io_error(path, error.error))?;
    Ok(())
}

fn load_one(path: &Path) -> Result<ConfigDocument, ConfigError> {
    let text = fs::read_to_string(path).map_err(|source| io_error(path, source))?;
    ConfigDocument::from_toml(path, &text)
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("toml.bak")
}

fn io_error(path: &Path, source: io::Error) -> ConfigError {
    ConfigError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn validate_profile(name: &str, profile: &ProfileConfig) -> Result<(), ConfigError> {
    if profile.dpi == 0 {
        return Err(ConfigError::Validation(format!("{name} DPI 必须大于 0")));
    }
    let mut controls = HashSet::new();
    for mapping in &profile.button_mappings {
        if mapping.physical_control.trim().is_empty()
            || !controls.insert(mapping.physical_control.as_str())
        {
            return Err(ConfigError::Validation(format!(
                "{name} 包含空或重复的 physical_control"
            )));
        }
        match &mapping.action {
            ButtonActionConfig::LogicalControl { value } if value.trim().is_empty() => {
                return Err(ConfigError::Validation(format!(
                    "{name} 包含空 logical_control"
                )));
            }
            ButtonActionConfig::OnboardKeyboard {
                usage_page, usage, ..
            } if *usage_page != 0x07 || *usage == 0 || *usage > 0xe7 => {
                return Err(ConfigError::Validation(format!(
                    "{name} 包含无效 Keyboard HID Usage"
                )));
            }
            _ => {}
        }
    }
    require_primary_click(profile, "g102:left", "mouse:left", name)?;
    require_primary_click(profile, "g102:right", "mouse:right", name)?;
    Ok(())
}

fn require_primary_click(
    profile: &ProfileConfig,
    control: &str,
    action: &str,
    name: &str,
) -> Result<(), ConfigError> {
    let preserved = profile.button_mappings.iter().any(|mapping| {
        mapping.physical_control == control
            && matches!(
                &mapping.action,
                ButtonActionConfig::LogicalControl { value } if value == action
            )
    });
    if preserved {
        Ok(())
    } else {
        Err(ConfigError::Validation(format!(
            "{name} 必须保留 {control} 的 {action}"
        )))
    }
}

fn office_button_mappings() -> Vec<ButtonMappingConfig> {
    [
        ("g102:left", logical("mouse:left")),
        ("g102:right", logical("mouse:right")),
        ("g102:middle", keyboard(0x2a, 0)),
        ("g102:side_back", keyboard(0x19, 1)),
        ("g102:side_forward", keyboard(0x06, 1)),
        ("g102:dpi", keyboard(0x04, 1)),
    ]
    .into_iter()
    .map(|(physical_control, action)| ButtonMappingConfig {
        physical_control: physical_control.to_owned(),
        action,
    })
    .collect()
}

fn logical(value: &str) -> ButtonActionConfig {
    ButtonActionConfig::LogicalControl {
        value: value.to_owned(),
    }
}

fn keyboard(usage: u16, modifiers: u8) -> ButtonActionConfig {
    ButtonActionConfig::OnboardKeyboard {
        usage_page: 0x07,
        usage,
        modifiers,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        ConfigDocument, ConfigError, SelectionMode, backup_path, load_or_create_default,
        load_with_backup, save_atomic,
    };

    #[test]
    fn default_config_round_trips_as_toml() {
        let document = ConfigDocument::default();
        let text = document.to_toml().unwrap();
        let decoded = ConfigDocument::from_toml("config.toml".as_ref(), &text).unwrap();
        assert_eq!(decoded, document);
        assert!(text.contains("physical_control = \"g102:side_back\""));
        assert!(text.contains("modifiers = 1"));
    }

    #[test]
    fn validation_protects_primary_clicks() {
        let mut document = ConfigDocument::default();
        document.profiles.office.button_mappings.remove(0);
        assert!(matches!(
            document.validate(),
            Err(ConfigError::Validation(_))
        ));
    }

    #[test]
    fn auto_selection_requires_rules() {
        let mut document = ConfigDocument::default();
        document.selection.mode = SelectionMode::Auto;
        document.selection.rules.clear();
        assert!(matches!(
            document.validate(),
            Err(ConfigError::Validation(_))
        ));
    }

    #[test]
    fn atomic_save_keeps_previous_backup_and_can_recover() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("pulsehub-config-{nonce}"));
        let path = directory.join("config.toml");
        let original = ConfigDocument::default();
        save_atomic(&path, &original).unwrap();

        let mut updated = original.clone();
        updated.profiles.office.dpi = 1600;
        save_atomic(&path, &updated).unwrap();
        assert_eq!(load_with_backup(&path).unwrap(), updated);

        std::fs::write(&path, "not valid toml").unwrap();
        assert_eq!(load_with_backup(&path).unwrap(), original);
        assert!(backup_path(&path).exists());

        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn creates_default_config_on_first_load() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("pulsehub-first-load-{nonce}"));
        let path = directory.join("config.toml");

        let document = load_or_create_default(&path).unwrap();
        assert_eq!(document, ConfigDocument::default());
        assert!(path.exists());

        std::fs::remove_dir_all(&directory).unwrap();
    }
}
