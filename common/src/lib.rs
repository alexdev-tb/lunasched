// https://www.youtube.com/watch?v=xvFZjo5PgG0

pub mod job;
pub mod ipc;
pub mod schedule;

pub use job::{Job, JobId, JobStatus, ScheduleConfig};
pub use ipc::{Request, Response, HistoryEntry};
pub use schedule::parse_schedule;

pub const DEFAULT_SOCKET_PATH: &str = "/var/run/lunasched.sock";
pub const DEFAULT_DB_PATH: &str = "/var/lib/lunasched/lunasched.db";
pub const DEFAULT_LOG_FILE: &str = "/var/log/lunasched/lunasched.log";
pub const DEFAULT_JOBS_LOG_FILE: &str = "/var/log/lunasched/jobs.log";
