// Configuration types and structures for superfreq
use crate::core::TurboSetting;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct BatteryChargeThresholds {
    pub start: u8,
    pub stop: u8,
}

impl BatteryChargeThresholds {
    pub fn new(start: u8, stop: u8) -> Result<Self, ConfigError> {
        if stop == 0 {
            return Err(ConfigError::ValidationError(
                "Stop threshold must be greater than 0%".to_string(),
            ));
        }
        if start >= stop {
            return Err(ConfigError::ValidationError(format!(
                "Start threshold ({start}) must be less than stop threshold ({stop})"
            )));
        }
        if stop > 100 {
            return Err(ConfigError::ValidationError(format!(
                "Stop threshold ({stop}) cannot exceed 100%"
            )));
        }

        Ok(Self { start, stop })
    }
}

impl TryFrom<(u8, u8)> for BatteryChargeThresholds {
    type Error = ConfigError;

    fn try_from(values: (u8, u8)) -> Result<Self, Self::Error> {
        let (start, stop) = values;
        Self::new(start, stop)
    }
}

// Structs for configuration using serde::Deserialize
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ProfileConfig {
    pub governor: Option<String>,
    pub turbo: Option<TurboSetting>,
    pub epp: Option<String>, // Energy Performance Preference (EPP)
    pub epb: Option<String>, // Energy Performance Bias (EPB) - usually an integer, but string for flexibility from sysfs
    pub min_freq_mhz: Option<u32>,
    pub max_freq_mhz: Option<u32>,
    pub platform_profile: Option<String>,
    pub turbo_auto_settings: Option<TurboAutoSettings>,
    pub enable_auto_turbo: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_charge_thresholds: Option<BatteryChargeThresholds>,
}

impl Default for ProfileConfig {
    fn default() -> Self {
        Self {
            governor: Some("schedutil".to_string()), // common sensible default (?)
            turbo: Some(TurboSetting::Auto),
            epp: None,              // defaults depend on governor and system
            epb: None,              // defaults depend on governor and system
            min_freq_mhz: None,     // no override
            max_freq_mhz: None,     // no override
            platform_profile: None, // no override
            turbo_auto_settings: Some(TurboAutoSettings::default()),
            enable_auto_turbo: default_enable_auto_turbo(),
            battery_charge_thresholds: None,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub charger: ProfileConfig,
    #[serde(default)]
    pub battery: ProfileConfig,
    pub ignored_power_supplies: Option<Vec<String>>,
    #[serde(default)]
    pub daemon: DaemonConfig,
}

// Error type for config loading
#[derive(Debug)]
pub enum ConfigError {
    IoError(std::io::Error),
    TomlError(toml::de::Error),
    ValidationError(String),
}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(err: toml::de::Error) -> Self {
        Self::TomlError(err)
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "I/O error: {e}"),
            Self::TomlError(e) => write!(f, "TOML parsing error: {e}"),
            Self::ValidationError(s) => write!(f, "Configuration validation error: {s}"),
        }
    }
}

impl std::error::Error for ConfigError {}

// Intermediate structs for TOML parsing
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ProfileConfigToml {
    pub governor: Option<String>,
    pub turbo: Option<String>, // "always", "auto", "never"
    pub epp: Option<String>,
    pub epb: Option<String>,
    pub min_freq_mhz: Option<u32>,
    pub max_freq_mhz: Option<u32>,
    pub platform_profile: Option<String>,
    pub turbo_auto_settings: Option<TurboAutoSettings>,
    #[serde(default = "default_enable_auto_turbo")]
    pub enable_auto_turbo: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_charge_thresholds: Option<BatteryChargeThresholds>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct AppConfigToml {
    #[serde(default)]
    pub charger: ProfileConfigToml,
    #[serde(default)]
    pub battery: ProfileConfigToml,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_charge_thresholds: Option<BatteryChargeThresholds>,
    pub ignored_power_supplies: Option<Vec<String>>,
    #[serde(default)]
    pub daemon: DaemonConfigToml,
}

impl Default for ProfileConfigToml {
    fn default() -> Self {
        Self {
            governor: Some("schedutil".to_string()),
            turbo: Some("auto".to_string()),
            epp: None,
            epb: None,
            min_freq_mhz: None,
            max_freq_mhz: None,
            platform_profile: None,
            turbo_auto_settings: Some(TurboAutoSettings::default()),
            enable_auto_turbo: default_enable_auto_turbo(),
            battery_charge_thresholds: None,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct TurboAutoSettings {
    #[serde(default = "default_load_threshold_high")]
    pub load_threshold_high: f32,
    #[serde(default = "default_load_threshold_low")]
    pub load_threshold_low: f32,
    #[serde(default = "default_temp_threshold_high")]
    pub temp_threshold_high: f32,
}

// Default thresholds for Auto turbo mode
pub const DEFAULT_LOAD_THRESHOLD_HIGH: f32 = 70.0; // enable turbo if load is above this
pub const DEFAULT_LOAD_THRESHOLD_LOW: f32 = 30.0; // disable turbo if load is below this
pub const DEFAULT_TEMP_THRESHOLD_HIGH: f32 = 75.0; // disable turbo if temperature is above this

const fn default_load_threshold_high() -> f32 {
    DEFAULT_LOAD_THRESHOLD_HIGH
}
const fn default_load_threshold_low() -> f32 {
    DEFAULT_LOAD_THRESHOLD_LOW
}
const fn default_temp_threshold_high() -> f32 {
    DEFAULT_TEMP_THRESHOLD_HIGH
}

impl Default for TurboAutoSettings {
    fn default() -> Self {
        Self {
            load_threshold_high: DEFAULT_LOAD_THRESHOLD_HIGH,
            load_threshold_low: DEFAULT_LOAD_THRESHOLD_LOW,
            temp_threshold_high: DEFAULT_TEMP_THRESHOLD_HIGH,
        }
    }
}

impl From<ProfileConfigToml> for ProfileConfig {
    fn from(toml_config: ProfileConfigToml) -> Self {
        Self {
            governor: toml_config.governor,
            turbo: toml_config
                .turbo
                .and_then(|s| match s.to_lowercase().as_str() {
                    "always" => Some(TurboSetting::Always),
                    "auto" => Some(TurboSetting::Auto),
                    "never" => Some(TurboSetting::Never),
                    _ => None,
                }),
            epp: toml_config.epp,
            epb: toml_config.epb,
            min_freq_mhz: toml_config.min_freq_mhz,
            max_freq_mhz: toml_config.max_freq_mhz,
            platform_profile: toml_config.platform_profile,
            turbo_auto_settings: toml_config
                .turbo_auto_settings
                .or_else(|| Some(TurboAutoSettings::default())),
            enable_auto_turbo: toml_config.enable_auto_turbo,
            battery_charge_thresholds: toml_config.battery_charge_thresholds,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct DaemonConfig {
    #[serde(default = "default_poll_interval_sec")]
    pub poll_interval_sec: u64,
    #[serde(default = "default_adaptive_interval")]
    pub adaptive_interval: bool,
    #[serde(default = "default_min_poll_interval_sec")]
    pub min_poll_interval_sec: u64,
    #[serde(default = "default_max_poll_interval_sec")]
    pub max_poll_interval_sec: u64,
    #[serde(default = "default_throttle_on_battery")]
    pub throttle_on_battery: bool,
    #[serde(default = "default_log_level")]
    pub log_level: LogLevel,
    #[serde(default = "default_stats_file_path")]
    pub stats_file_path: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warning,
    Info,
    Debug,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            poll_interval_sec: default_poll_interval_sec(),
            adaptive_interval: default_adaptive_interval(),
            min_poll_interval_sec: default_min_poll_interval_sec(),
            max_poll_interval_sec: default_max_poll_interval_sec(),
            throttle_on_battery: default_throttle_on_battery(),
            log_level: default_log_level(),
            stats_file_path: default_stats_file_path(),
        }
    }
}

const fn default_poll_interval_sec() -> u64 {
    5
}

const fn default_adaptive_interval() -> bool {
    false
}

const fn default_min_poll_interval_sec() -> u64 {
    1
}

const fn default_max_poll_interval_sec() -> u64 {
    30
}

const fn default_throttle_on_battery() -> bool {
    true
}

const fn default_log_level() -> LogLevel {
    LogLevel::Info
}

const fn default_stats_file_path() -> Option<String> {
    None
}

const fn default_enable_auto_turbo() -> bool {
    true
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct DaemonConfigToml {
    #[serde(default = "default_poll_interval_sec")]
    pub poll_interval_sec: u64,
    #[serde(default = "default_adaptive_interval")]
    pub adaptive_interval: bool,
    #[serde(default = "default_min_poll_interval_sec")]
    pub min_poll_interval_sec: u64,
    #[serde(default = "default_max_poll_interval_sec")]
    pub max_poll_interval_sec: u64,
    #[serde(default = "default_throttle_on_battery")]
    pub throttle_on_battery: bool,
    #[serde(default = "default_log_level")]
    pub log_level: LogLevel,
    #[serde(default = "default_stats_file_path")]
    pub stats_file_path: Option<String>,
}

impl Default for DaemonConfigToml {
    fn default() -> Self {
        Self {
            poll_interval_sec: default_poll_interval_sec(),
            adaptive_interval: default_adaptive_interval(),
            min_poll_interval_sec: default_min_poll_interval_sec(),
            max_poll_interval_sec: default_max_poll_interval_sec(),
            throttle_on_battery: default_throttle_on_battery(),
            log_level: default_log_level(),
            stats_file_path: default_stats_file_path(),
        }
    }
}
