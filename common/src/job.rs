use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct JobId(pub String);

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarParams {
    pub days_of_week: Option<Vec<u32>>, // 0=Mon, 6=Sun (chrono-like but 0-indexed from Mon for simplicity in parsing? Or use chrono::Weekday)
    // Actually let's use u32 for simplicity in serialization: 1=Mon, 7=Sun to match ISO/Chrono
    pub nth_weekday: Option<(u32, u32)>, // (n, weekday) e.g. (1, 1) = 1st Monday
    pub time: (u32, u32, u32), // H, M, S
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScheduleConfig {
    Cron(String),
    Every(u64),
    Calendar(CalendarParams),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BackoffStrategy {
    Fixed,
    Linear,
    Exponential,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff_strategy: BackoffStrategy,
    pub initial_delay_seconds: u64,
    pub max_delay_seconds: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 0, // No retries by default
            backoff_strategy: BackoffStrategy::Exponential,
            initial_delay_seconds: 60,
            max_delay_seconds: 3600,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub timeout_seconds: Option<u64>,
    pub max_memory_mb: Option<u64>,
    pub cpu_quota: Option<f32>, // 0.0-1.0, 1.0 = 100% of one core
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            timeout_seconds: None,
            max_memory_mb: None,
            cpu_quota: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobHooks {
    pub on_failure: Option<String>,
    pub on_success: Option<String>,
}

impl Default for JobHooks {
    fn default() -> Self {
        Self {
            on_failure: None,
            on_success: None,
        }
    }
}

// New v1.2.0 structures
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobPriority {
    Low,
    Normal,
    High,
    Critical,
}

impl Default for JobPriority {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionMode {
    Sequential,  // Wait for previous execution to finish
    Parallel,    // Allow multiple executions
    Exclusive,   // Only one instance across all jobs
}

impl Default for ExecutionMode {
    fn default() -> Self {
        Self::Sequential
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub on_success: Option<Vec<NotificationChannel>>,
    pub on_failure: Option<Vec<NotificationChannel>>,
    pub on_start: Option<Vec<NotificationChannel>>,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            on_success: None,
            on_failure: None,
            on_start: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationChannel {
    Email { to: String, subject: Option<String> },
    Webhook { url: String, headers: Option<HashMap<String, String>> },
    Discord { webhook_url: String },
    Slack { webhook_url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub name: String,
    pub schedule: ScheduleConfig,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub enabled: bool,
    pub owner: String,
    
    // Phase 1 fields
    #[serde(default)]
    pub retry_policy: RetryPolicy,
    #[serde(default)]
    pub resource_limits: ResourceLimits,
    #[serde(default)]
    pub jitter_seconds: u64,
    pub timezone: Option<String>, // e.g., "America/New_York"
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<JobId>,
    #[serde(default)]
    pub hooks: JobHooks,
    #[serde(default)]
    pub max_concurrent: u32, // 0 = unlimited
    
    // Phase 2 fields (v1.2.0)
    #[serde(default)]
    pub priority: JobPriority,
    #[serde(default)]
    pub execution_mode: ExecutionMode,
    #[serde(default)]
    pub notification_config: NotificationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Running(u32), // PID
    Failed(i32), // Exit code
    Success,
}
