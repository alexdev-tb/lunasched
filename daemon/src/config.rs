use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use anyhow::{Context, Result};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_tick_interval")]
    pub tick_interval_ms: u64,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_jobs: u32,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default = "default_socket_path")]
    pub socket_path: PathBuf,
}

fn default_tick_interval() -> u64 { 1000 }
fn default_max_concurrent() -> u32 { 10 }
fn default_data_dir() -> PathBuf { PathBuf::from("/var/lib/lunasched") }
fn default_socket_path() -> PathBuf { PathBuf::from(common::DEFAULT_SOCKET_PATH) }

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            tick_interval_ms: default_tick_interval(),
            max_concurrent_jobs: default_max_concurrent(),
            data_dir: default_data_dir(),
            socket_path: default_socket_path(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_format")]
    pub format: String,
    pub output: Option<PathBuf>,
}

fn default_log_level() -> String { "info".to_string() }
fn default_log_format() -> String { "text".to_string() }

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
            output: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionConfig {
    #[serde(default = "default_history_days")]
    pub history_days: u32,
    #[serde(default = "default_max_history_per_job")]
    pub max_history_per_job: u32,
}

fn default_history_days() -> u32 { 30 }
fn default_max_history_per_job() -> u32 { 100 }

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            history_days: default_history_days(),
            max_history_per_job: default_max_history_per_job(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub retention: RetentionConfig,
    #[serde(default)]
    pub jobs: Vec<common::Job>,
}

impl Config {
    /// Load configuration from a YAML file
    pub fn from_yaml_file(path: &PathBuf) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;
        let config: Config = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {:?}", path))?;
        Ok(config)
    }

    /// Load configuration from a TOML file
    pub fn from_toml_file(path: &PathBuf) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {:?}", path))?;
        Ok(config)
    }

    /// Detect file type by extension and load
    pub fn from_file(path: &PathBuf) -> Result<Self> {
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        
        match ext {
            "yaml" | "yml" => Self::from_yaml_file(path),
            "toml" => Self::from_toml_file(path),
            _ => Err(anyhow::anyhow!("Unsupported config file format. Use .yaml, .yml, or .toml")),
        }
    }

    /// Merge with another config, preferring values from other
    pub fn merge(&mut self, other: Config) {
        // Server settings
        self.server.tick_interval_ms = other.server.tick_interval_ms;
        self.server.max_concurrent_jobs = other.server.max_concurrent_jobs;
        self.server.data_dir = other.server.data_dir;
        self.server.socket_path = other.server.socket_path;
        
        // Logging settings
        self.logging.level = other.logging.level;
        self.logging.format = other.logging.format;
        if other.logging.output.is_some() {
            self.logging.output = other.logging.output;
        }
        
        // Retention settings
        self.retention.history_days = other.retention.history_days;
        self.retention.max_history_per_job = other.retention.max_history_per_job;
        
        // Jobs - append
        self.jobs.extend(other.jobs);
    }
}
