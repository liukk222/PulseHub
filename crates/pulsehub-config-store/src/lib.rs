#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CONFIG_SCHEMA_VERSION: u32 = 1;
pub const CONFIG_TRANSFER_SCHEMA_VERSION: u32 = 1;
pub const PRODUCTION_DEFAULTS_REVISION: u32 = 1;
pub const CONFIG_FILE_NAME: &str = "config.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub production_defaults_revision: u32,
    #[serde(default)]
    pub agent: AgentConfig,
    pub selection: SelectionConfig,
    pub profiles: ProfilesConfig,
    #[serde(default)]
    pub applications: Vec<ApplicationProfileConfig>,
    #[serde(default = "default_shutdown_profile")]
    pub shutdown_profile: ProfileConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    pub start_with_windows: bool,
    #[serde(default)]
    pub developer_logging: bool,
    #[serde(default)]
    pub language: UiLanguage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UiLanguage {
    #[default]
    ZhCn,
    En,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            start_with_windows: true,
            developer_logging: false,
            language: UiLanguage::ZhCn,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionConfig {
    pub mode: SelectionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_application_id: Option<String>,
    #[serde(default)]
    pub rules: Vec<SelectionRule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    Auto,
    Office,
    Cs2,
    Application,
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
    #[serde(default = "default_report_rate_hz")]
    pub report_rate_hz: u16,
    #[serde(default = "default_dpi_levels")]
    pub dpi_levels: Vec<u16>,
    #[serde(default)]
    pub button_mappings: Vec<ButtonMappingConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationProfileConfig {
    pub id: String,
    pub name: String,
    pub executable_path: String,
    pub process_name: String,
    pub profile: ProfileConfig,
}

/// 可在不同 Windows 安装之间迁移的配置内容。
///
/// 代理运行方式、界面语言和登录启动属于本机偏好，刻意不包含在内。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigTransfer {
    pub transfer_schema_version: u32,
    pub selection: SelectionConfig,
    pub profiles: ProfilesConfig,
    #[serde(default)]
    pub applications: Vec<ApplicationProfileConfig>,
    pub shutdown_profile: ProfileConfig,
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
    RevisionConflict { expected: u64, actual: u64 },
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
            Self::RevisionConflict { expected, actual } => write!(
                formatter,
                "配置修订冲突：提交基于 {expected}，当前修订为 {actual}"
            ),
        }
    }
}

#[derive(Debug)]
pub struct ConfigRepository {
    path: PathBuf,
    document: ConfigDocument,
    revision: u64,
}

impl ConfigRepository {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, ConfigError> {
        let path = path.into();
        let document = load_or_create_default(&path)?;
        Ok(Self {
            path,
            document,
            revision: 1,
        })
    }

    pub fn from_document(
        path: impl Into<PathBuf>,
        document: ConfigDocument,
    ) -> Result<Self, ConfigError> {
        document.validate()?;
        Ok(Self {
            path: path.into(),
            document,
            revision: 1,
        })
    }

    pub fn document(&self) -> &ConfigDocument {
        &self.document
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn validate_draft(&self, draft: serde_json::Value) -> Result<ConfigDocument, ConfigError> {
        let document: ConfigDocument = serde_json::from_value(draft)
            .map_err(|error| ConfigError::Validation(format!("JSON 草稿格式无效：{error}")))?;
        document.validate()?;
        Ok(document)
    }

    pub fn commit(
        &mut self,
        base_revision: u64,
        draft: serde_json::Value,
    ) -> Result<u64, ConfigError> {
        if base_revision != self.revision {
            return Err(ConfigError::RevisionConflict {
                expected: base_revision,
                actual: self.revision,
            });
        }
        let document = self.validate_draft(draft)?;
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| ConfigError::Validation("配置修订号溢出".to_owned()))?;
        save_atomic(&self.path, &document)?;
        self.document = document;
        self.revision = next_revision;
        Ok(self.revision)
    }
}

impl std::error::Error for ConfigError {}

impl Default for ConfigDocument {
    fn default() -> Self {
        let office_buttons = office_button_mappings();
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            production_defaults_revision: PRODUCTION_DEFAULTS_REVISION,
            agent: AgentConfig::default(),
            selection: SelectionConfig {
                mode: SelectionMode::Auto,
                fixed_application_id: None,
                rules: vec![SelectionRule {
                    environment: ProfileName::Cs2,
                    process_names: vec!["cs2.exe".to_owned()],
                }],
            },
            profiles: ProfilesConfig {
                office: ProfileConfig {
                    dpi: 1600,
                    report_rate_hz: default_report_rate_hz(),
                    dpi_levels: default_dpi_levels(),
                    button_mappings: office_buttons.clone(),
                },
                cs2: ProfileConfig {
                    dpi: 1600,
                    report_rate_hz: default_report_rate_hz(),
                    dpi_levels: default_dpi_levels(),
                    button_mappings: office_buttons,
                },
            },
            applications: Vec::new(),
            shutdown_profile: default_shutdown_profile(),
        }
    }
}

impl ConfigDocument {
    pub fn export_transfer(&self) -> ConfigTransfer {
        ConfigTransfer {
            transfer_schema_version: CONFIG_TRANSFER_SCHEMA_VERSION,
            selection: self.selection.clone(),
            profiles: self.profiles.clone(),
            applications: self.applications.clone(),
            shutdown_profile: self.shutdown_profile.clone(),
        }
    }

    pub fn apply_transfer(&mut self, transfer: ConfigTransfer) -> Result<(), ConfigError> {
        if transfer.transfer_schema_version != CONFIG_TRANSFER_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchema {
                found: transfer.transfer_schema_version,
            });
        }
        self.selection = transfer.selection;
        self.profiles = transfer.profiles;
        self.applications = transfer.applications;
        self.shutdown_profile = transfer.shutdown_profile;
        self.validate()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchema {
                found: self.schema_version,
            });
        }
        if self.production_defaults_revision != PRODUCTION_DEFAULTS_REVISION {
            return Err(ConfigError::Validation(format!(
                "不支持生产默认配置修订 {}",
                self.production_defaults_revision
            )));
        }
        validate_profile("office", &self.profiles.office)?;
        validate_profile("cs2", &self.profiles.cs2)?;
        validate_profile("shutdown", &self.shutdown_profile)?;
        let mut application_ids = HashSet::new();
        let mut application_processes = HashSet::new();
        let mut application_names = HashSet::new();
        for application in &self.applications {
            if application.id.trim().is_empty()
                || !application
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            {
                return Err(ConfigError::Validation(
                    "应用环境 id 只能包含字母、数字、连字符和下划线".to_owned(),
                ));
            }
            if !application_ids.insert(application.id.to_ascii_lowercase()) {
                return Err(ConfigError::Validation(format!(
                    "应用环境 id 重复：{}",
                    application.id
                )));
            }
            if application.name.trim().is_empty() || application.executable_path.trim().is_empty() {
                return Err(ConfigError::Validation(
                    "应用环境名称和 EXE 路径不能为空".to_owned(),
                ));
            }
            if !application_names.insert(application.name.to_ascii_lowercase()) {
                return Err(ConfigError::Validation(format!(
                    "应用环境名称重复：{}",
                    application.name
                )));
            }
            if !application
                .process_name
                .to_ascii_lowercase()
                .ends_with(".exe")
            {
                return Err(ConfigError::Validation(format!(
                    "应用环境进程名必须以 .exe 结尾：{}",
                    application.process_name
                )));
            }
            if !application_processes.insert(application.process_name.to_ascii_lowercase()) {
                return Err(ConfigError::Validation(format!(
                    "应用环境进程名重复：{}",
                    application.process_name
                )));
            }
            validate_profile(
                &format!("application.{}", application.id),
                &application.profile,
            )?;
        }
        match self.selection.mode {
            SelectionMode::Application => {
                let id = self
                    .selection
                    .fixed_application_id
                    .as_deref()
                    .ok_or_else(|| {
                        ConfigError::Validation("固定应用模式缺少应用环境 id".to_owned())
                    })?;
                if !application_ids.contains(&id.to_ascii_lowercase()) {
                    return Err(ConfigError::Validation(format!("固定应用环境不存在：{id}")));
                }
            }
            _ if self.selection.fixed_application_id.is_some() => {
                return Err(ConfigError::Validation(
                    "非固定应用模式不能保存 fixed_application_id".to_owned(),
                ));
            }
            _ => {}
        }
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
        let mut document: Self = toml::from_str(text).map_err(|error| ConfigError::Parse {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        if document.production_defaults_revision < PRODUCTION_DEFAULTS_REVISION {
            let defaults = Self::default();
            document.production_defaults_revision = PRODUCTION_DEFAULTS_REVISION;
            document.agent.developer_logging = false;
            document.selection = defaults.selection;
            document.profiles = defaults.profiles;
            document.applications.clear();
            document.shutdown_profile = defaults.shutdown_profile;
        }
        normalize_profile_dpi_levels(&mut document.profiles.office);
        normalize_profile_dpi_levels(&mut document.profiles.cs2);
        for application in &mut document.applications {
            normalize_profile_dpi_levels(&mut application.profile);
        }
        document.validate()?;
        Ok(document)
    }
}

impl ConfigTransfer {
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(self).map_err(|error| ConfigError::Serialize(error.to_string()))
    }

    pub fn from_toml(path: &Path, text: &str) -> Result<Self, ConfigError> {
        let transfer: Self = toml::from_str(text).map_err(|error| ConfigError::Parse {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        let mut document = ConfigDocument::default();
        document.apply_transfer(transfer.clone())?;
        Ok(transfer)
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
    if path.exists() {
        match load_one_with_migration_status(path) {
            Ok((document, migrated)) => {
                if migrated {
                    save_atomic(path, &document)?;
                }
                return Ok(document);
            }
            Err(_) => return load_with_backup(path),
        }
    }
    if backup_path(path).exists() {
        return load_with_backup(path);
    }
    let document = ConfigDocument::default();
    save_atomic(path, &document)?;
    Ok(document)
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
    load_one_with_migration_status(path).map(|(document, _)| document)
}

fn load_one_with_migration_status(path: &Path) -> Result<(ConfigDocument, bool), ConfigError> {
    let text = fs::read_to_string(path).map_err(|source| io_error(path, source))?;
    let parsed: toml::Value = toml::from_str(&text).map_err(|error| ConfigError::Parse {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let previous_revision = parsed
        .get("production_defaults_revision")
        .and_then(toml::Value::as_integer)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let document = ConfigDocument::from_toml(path, &text)?;
    Ok((document, previous_revision < PRODUCTION_DEFAULTS_REVISION))
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
    if ![1000, 500, 250, 125].contains(&profile.report_rate_hz) {
        return Err(ConfigError::Validation(format!(
            "{name} 回报率只能是 1000、500、250 或 125 Hz"
        )));
    }
    if profile.dpi_levels.len() != 4
        || profile.dpi_levels.contains(&0)
        || !profile.dpi_levels.windows(2).all(|pair| pair[0] < pair[1])
    {
        return Err(ConfigError::Validation(format!(
            "{name} DPI 档位必须包含四个严格递增的正整数"
        )));
    }
    if profile_uses_dpi_cycle(profile) && !profile.dpi_levels.contains(&profile.dpi) {
        return Err(ConfigError::Validation(format!(
            "{name} 当前 DPI 必须属于四个 DPI 切换档位"
        )));
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
        ("g102:middle", logical("mouse:middle")),
        ("g102:side_back", logical("mouse:back")),
        ("g102:side_forward", logical("mouse:forward")),
        ("g102:dpi", logical("mouse:dpi_cycle")),
    ]
    .into_iter()
    .map(|(physical_control, action)| ButtonMappingConfig {
        physical_control: physical_control.to_owned(),
        action,
    })
    .collect()
}

fn default_dpi_levels() -> Vec<u16> {
    vec![800, 1600, 2400, 3200]
}

fn default_report_rate_hz() -> u16 {
    1000
}

fn default_shutdown_profile() -> ProfileConfig {
    ProfileConfig {
        dpi: 1600,
        report_rate_hz: default_report_rate_hz(),
        dpi_levels: default_dpi_levels(),
        button_mappings: office_button_mappings(),
    }
}

fn profile_uses_dpi_cycle(profile: &ProfileConfig) -> bool {
    profile.button_mappings.iter().any(|mapping| {
        mapping.physical_control == "g102:dpi"
            && matches!(
                &mapping.action,
                ButtonActionConfig::LogicalControl { value } if value == "mouse:dpi_cycle"
            )
    })
}

fn normalize_profile_dpi_levels(profile: &mut ProfileConfig) {
    if !profile_uses_dpi_cycle(profile)
        || profile.dpi_levels.len() != 4
        || profile.dpi_levels.contains(&profile.dpi)
    {
        return;
    }
    let nearest = profile
        .dpi_levels
        .iter()
        .enumerate()
        .min_by_key(|(_, level)| level.abs_diff(profile.dpi))
        .map(|(index, _)| index)
        .unwrap_or(0);
    profile.dpi_levels[nearest] = profile.dpi;
    profile.dpi_levels.sort_unstable();
}

fn logical(value: &str) -> ButtonActionConfig {
    ButtonActionConfig::LogicalControl {
        value: value.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        ApplicationProfileConfig, ButtonActionConfig, ConfigDocument, ConfigError,
        ConfigRepository, SelectionMode, backup_path, load_or_create_default, load_with_backup,
        office_button_mappings, save_atomic,
    };

    #[test]
    fn default_config_round_trips_as_toml() {
        let document = ConfigDocument::default();
        let text = document.to_toml().unwrap();
        let decoded = ConfigDocument::from_toml("config.toml".as_ref(), &text).unwrap();
        assert_eq!(decoded, document);
        assert!(text.contains("physical_control = \"g102:side_back\""));
        assert!(text.contains("value = \"mouse:back\""));
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
    fn report_rate_accepts_only_the_four_product_options() {
        for rate in [1000, 500, 250, 125] {
            let mut document = ConfigDocument::default();
            document.profiles.office.report_rate_hz = rate;
            assert!(document.validate().is_ok(), "{rate} Hz 应当有效");
        }
        let mut document = ConfigDocument::default();
        document.profiles.office.report_rate_hz = 333;
        assert!(matches!(
            document.validate(),
            Err(ConfigError::Validation(_))
        ));
    }

    #[test]
    fn default_shutdown_profile_preserves_the_previous_safe_exit_behavior() {
        let profile = &ConfigDocument::default().shutdown_profile;
        assert_eq!(profile.dpi, 1600);
        assert_eq!(profile.report_rate_hz, 1000);
        assert_eq!(profile.button_mappings, office_button_mappings());
    }

    #[test]
    fn first_run_defaults_keep_all_original_mouse_controls() {
        let document = ConfigDocument::default();
        assert!(!document.agent.developer_logging);
        assert!(document.applications.is_empty());
        for profile in [
            &document.profiles.office,
            &document.profiles.cs2,
            &document.shutdown_profile,
        ] {
            assert_eq!(profile.dpi, 1600);
            assert_eq!(profile.dpi_levels, [800, 1600, 2400, 3200]);
            for (control, expected) in [
                ("g102:left", "mouse:left"),
                ("g102:right", "mouse:right"),
                ("g102:middle", "mouse:middle"),
                ("g102:side_back", "mouse:back"),
                ("g102:side_forward", "mouse:forward"),
                ("g102:dpi", "mouse:dpi_cycle"),
            ] {
                assert!(profile.button_mappings.iter().any(|mapping| {
                    mapping.physical_control == control
                        && matches!(
                            &mapping.action,
                            ButtonActionConfig::LogicalControl { value } if value == expected
                        )
                }));
            }
        }
    }

    #[test]
    fn legacy_dpi_cycle_profile_keeps_current_dpi_during_level_migration() {
        let mut document = ConfigDocument::default();
        document.profiles.office.dpi = 1800;
        document.profiles.office.dpi_levels = vec![800, 1600, 2400, 3200];
        let text = toml::to_string_pretty(&document).unwrap();

        let migrated = ConfigDocument::from_toml("config.toml".as_ref(), &text).unwrap();

        assert_eq!(migrated.profiles.office.dpi, 1800);
        assert_eq!(migrated.profiles.office.dpi_levels, [800, 1800, 2400, 3200]);
    }

    #[test]
    fn pre_v012_config_migrates_once_to_safe_production_defaults() {
        let mut legacy = ConfigDocument::default();
        legacy.agent.start_with_windows = false;
        legacy.agent.language = super::UiLanguage::En;
        legacy.agent.developer_logging = true;
        legacy.profiles.office.dpi = 3200;
        legacy.profiles.cs2.dpi = 3200;
        legacy.applications.push(ApplicationProfileConfig {
            id: "winword".to_owned(),
            name: "Word 环境".to_owned(),
            executable_path: r"C:\Program Files\Microsoft Office\WINWORD.EXE".to_owned(),
            process_name: "WINWORD.EXE".to_owned(),
            profile: legacy.profiles.cs2.clone(),
        });
        let text = toml::to_string_pretty(&legacy)
            .unwrap()
            .replace("production_defaults_revision = 1\n", "");

        let migrated = ConfigDocument::from_toml("legacy.toml".as_ref(), &text).unwrap();

        assert_eq!(
            migrated.production_defaults_revision,
            super::PRODUCTION_DEFAULTS_REVISION
        );
        assert!(!migrated.agent.start_with_windows);
        assert_eq!(migrated.agent.language, super::UiLanguage::En);
        assert!(!migrated.agent.developer_logging);
        assert!(migrated.applications.is_empty());
        for profile in [
            &migrated.profiles.office,
            &migrated.profiles.cs2,
            &migrated.shutdown_profile,
        ] {
            assert_eq!(profile.dpi, 1600);
            assert_eq!(profile.button_mappings, office_button_mappings());
        }
    }

    #[test]
    fn production_defaults_migration_is_persisted_with_backup() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("pulsehub-migration-{nonce}"));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.toml");
        let mut legacy = ConfigDocument::default();
        legacy.profiles.office.dpi = 3200;
        let legacy_text = toml::to_string_pretty(&legacy)
            .unwrap()
            .replace("production_defaults_revision = 1\n", "");
        std::fs::write(&path, &legacy_text).unwrap();

        let migrated = load_or_create_default(&path).unwrap();

        assert_eq!(migrated.profiles.office.dpi, 1600);
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("production_defaults_revision = 1"));
        assert_eq!(
            std::fs::read_to_string(backup_path(&path)).unwrap(),
            legacy_text
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn application_profiles_round_trip_and_reject_duplicate_processes() {
        let mut document = ConfigDocument::default();
        document.applications.push(ApplicationProfileConfig {
            id: "winword".to_owned(),
            name: "WINWORD".to_owned(),
            executable_path: r"C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE"
                .to_owned(),
            process_name: "WINWORD.EXE".to_owned(),
            profile: document.profiles.cs2.clone(),
        });
        let text = document.to_toml().unwrap();
        let decoded = ConfigDocument::from_toml("config.toml".as_ref(), &text).unwrap();
        assert_eq!(decoded.applications, document.applications);

        let mut duplicate = document.applications[0].clone();
        duplicate.id = "word_second".to_owned();
        duplicate.name = "Word Second".to_owned();
        duplicate.process_name = "winword.exe".to_owned();
        document.applications.push(duplicate);
        assert!(matches!(
            document.validate(),
            Err(ConfigError::Validation(message)) if message.contains("进程名重复")
        ));
    }

    #[test]
    fn transfer_round_trip_keeps_profiles_and_applications_but_not_machine_preferences() {
        let mut original = ConfigDocument::default();
        original.agent.start_with_windows = false;
        original.agent.language = super::UiLanguage::En;
        original.profiles.office.dpi = 3200;
        original.applications.push(ApplicationProfileConfig {
            id: "winword".to_owned(),
            name: "Word 环境".to_owned(),
            executable_path: r"C:\Program Files\Microsoft Office\WINWORD.EXE".to_owned(),
            process_name: "WINWORD.EXE".to_owned(),
            profile: original.profiles.cs2.clone(),
        });

        let text = original.export_transfer().to_toml().unwrap();
        let transfer =
            super::ConfigTransfer::from_toml("export.pulsehub.toml".as_ref(), &text).unwrap();
        let mut destination = ConfigDocument::default();
        destination.agent.start_with_windows = true;
        destination.apply_transfer(transfer).unwrap();

        assert_eq!(destination.profiles, original.profiles);
        assert_eq!(destination.applications, original.applications);
        assert!(destination.agent.start_with_windows);
        assert_eq!(destination.agent.language, super::UiLanguage::ZhCn);
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

    #[test]
    fn repository_validates_commits_and_rejects_stale_revision() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("pulsehub-repository-{nonce}"));
        let path = directory.join("config.toml");
        let original = ConfigDocument::default();
        let mut repository = ConfigRepository::from_document(&path, original.clone()).unwrap();
        let mut updated = original;
        updated.profiles.office.dpi = 3200;
        let draft = serde_json::to_value(&updated).unwrap();

        assert_eq!(repository.validate_draft(draft.clone()).unwrap(), updated);
        assert_eq!(repository.commit(1, draft.clone()).unwrap(), 2);
        assert_eq!(repository.revision(), 2);
        assert_eq!(load_with_backup(&path).unwrap(), updated);
        assert!(matches!(
            repository.commit(1, draft),
            Err(ConfigError::RevisionConflict {
                expected: 1,
                actual: 2
            })
        ));

        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn repository_rejects_invalid_json_without_changing_revision() {
        let document = ConfigDocument::default();
        let mut repository = ConfigRepository::from_document("unused.toml", document).unwrap();
        let invalid = serde_json::json!({"schema_version": 1, "unknown": true});

        assert!(matches!(
            repository.commit(1, invalid),
            Err(ConfigError::Validation(_))
        ));
        assert_eq!(repository.revision(), 1);
    }
}
