// https://www.youtube.com/watch?v=xvFZjo5PgG0

pub mod ipc;
pub mod job;
pub mod schedule;

pub use ipc::{Request, Response, HistoryEntry};
pub use job::{Job, JobId, ScheduleConfig, CalendarParams, JobStatus, 
             RetryPolicy, ResourceLimits, JobHooks, BackoffStrategy,
             JobPriority, ExecutionMode, NotificationConfig, NotificationChannel};
pub use schedule::parse_schedule;

// Production paths (follow FHS - Filesystem Hierarchy Standard)
pub const DEFAULT_SOCKET_PATH: &str = "/var/run/lunasched/lunasched.sock";
pub const DEFAULT_DB_PATH: &str = "/var/lib/lunasched/lunasched.db";
pub const DEFAULT_CONFIG_PATH: &str = "/etc/lunasched/config.yaml";
pub const DEFAULT_LOG_FILE: &str = "/var/log/lunasched/daemon.log";
pub const DEFAULT_JOBS_LOG_FILE: &str = "/var/log/lunasched/jobs.log";

// Fallback paths for non-root users
pub const USER_SOCKET_PATH: &str = "/tmp/lunasched.sock";
pub const USER_DB_PATH: &str = "lunasched.db";
pub const USER_CONFIG_PATH: &str = "~/.config/lunasched/config.yaml";
pub const USER_LOG_FILE: &str = "lunasched-daemon.log";
pub const USER_JOBS_LOG_FILE: &str = "lunasched-jobs.log";
