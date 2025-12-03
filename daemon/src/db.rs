use rusqlite::{params, Connection, Result};
use common::{Job, ScheduleConfig, JobId, RetryPolicy, ResourceLimits, JobHooks};
use std::collections::HashMap;

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        Ok(Self { conn })
    }

    pub fn from_connection(conn: Connection) -> Self {
        Self { conn }
    }

    pub fn add_job(&self, job: &Job) -> Result<()> {
        let (sched_type, sched_val) = match &job.schedule {
            ScheduleConfig::Cron(s) => ("cron", s.clone()),
            ScheduleConfig::Every(s) => ("every", s.to_string()),
            ScheduleConfig::Calendar(p) => ("calendar", serde_json::to_string(p).unwrap()),
        };
        
        let args_json = serde_json::to_string(&job.args).unwrap();
        let env_json = serde_json::to_string(&job.env).unwrap();
        
        // Serialize Phase 1 fields
        let retry_policy_json = serde_json::to_string(&job.retry_policy).unwrap();
        let resource_limits_json = serde_json::to_string(&job.resource_limits).unwrap();
        let tags_json = serde_json::to_string(&job.tags).unwrap();
        let dependencies_json = serde_json::to_string(&job.dependencies).unwrap();
        let hooks_json = serde_json::to_string(&job.hooks).unwrap();
        
        // Serialize Phase 2 (v1.2.0) fields
        let priority_json = serde_json::to_string(&job.priority).unwrap();
        let execution_mode_json = serde_json::to_string(&job.execution_mode).unwrap();
        let notification_config_json = serde_json::to_string(&job.notification_config).unwrap();

        self.conn.execute(
            "INSERT OR REPLACE INTO jobs 
             (id, name, schedule_type, schedule_value, command, args, env, enabled, owner,
              retry_policy, resource_limits, jitter_seconds, timezone, tags, dependencies, hooks, max_concurrent,
              priority, execution_mode, notification_config)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
            params![
                job.id.0, job.name, sched_type, sched_val, job.command, args_json, env_json, 
                job.enabled, job.owner,
                retry_policy_json, resource_limits_json, job.jitter_seconds as i64, 
                job.timezone, tags_json, dependencies_json, hooks_json, job.max_concurrent as i64,
                priority_json, execution_mode_json, notification_config_json
            ],
        )?;
        Ok(())
    }

    pub fn remove_job(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM jobs WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn load_jobs(&self) -> Result<HashMap<String, Job>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, schedule_type, schedule_value, command, args, env, enabled, owner,
                    retry_policy, resource_limits, jitter_seconds, timezone, tags, dependencies, hooks, max_concurrent,
                    priority, execution_mode, notification_config
             FROM jobs"
        )?;
        
        let job_iter = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let sched_type: String = row.get(2)?;
            let sched_val: String = row.get(3)?;
            let command: String = row.get(4)?;
            let args_json: String = row.get(5)?;
            let env_json: String = row.get(6)?;
            let enabled: bool = row.get(7)?;
            let owner: String = row.get(8)?;
            
            // Load Phase 1 fields with fallbacks for old schema
            let retry_policy_json: String = row.get(9).unwrap_or_else(|_| "{}".to_string());
            let resource_limits_json: String = row.get(10).unwrap_or_else(|_| "{}".to_string());
            let jitter_seconds: i64 = row.get(11).unwrap_or(0);
            let timezone: Option<String> = row.get(12).ok();
            let tags_json: String = row.get(13).unwrap_or_else(|_| "[]".to_string());
            let dependencies_json: String = row.get(14).unwrap_or_else(|_| "[]".to_string());
            let hooks_json: String = row.get(15).unwrap_or_else(|_| "{}".to_string());
            let max_concurrent: i64 = row.get(16).unwrap_or(0);

            let schedule = match sched_type.as_str() {
                "cron" => ScheduleConfig::Cron(sched_val),
                "every" => ScheduleConfig::Every(sched_val.parse().unwrap_or(0)),
                "calendar" => ScheduleConfig::Calendar(serde_json::from_str(&sched_val).unwrap()),
                _ => ScheduleConfig::Cron(sched_val), // Fallback
            };

            let args: Vec<String> = serde_json::from_str(&args_json).unwrap_or_default();
            let env: HashMap<String, String> = serde_json::from_str(&env_json).unwrap_or_default();
            
            let retry_policy: RetryPolicy = serde_json::from_str(&retry_policy_json)
                .unwrap_or_default();
            let resource_limits: ResourceLimits = serde_json::from_str(&resource_limits_json)
                .unwrap_or_default();
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            let dependencies: Vec<JobId> = serde_json::from_str(&dependencies_json).unwrap_or_default();
            let hooks: JobHooks = serde_json::from_str(&hooks_json).unwrap_or_default();
            
            // Load Phase 2 (v1.2.0) fields
            let priority_json: String = row.get(17).unwrap_or_else(|_| "{}".to_string());
            let execution_mode_json: String = row.get(18).unwrap_or_else(|_| "{}".to_string());
            let notification_config_json: String = row.get(19).unwrap_or_else(|_| "{}".to_string());
            
            use common::{JobPriority, ExecutionMode, NotificationConfig};
            let priority: JobPriority = serde_json::from_str(&priority_json).unwrap_or_default();
            let execution_mode: ExecutionMode = serde_json::from_str(&execution_mode_json).unwrap_or_default();
            let notification_config: NotificationConfig = serde_json::from_str(&notification_config_json).unwrap_or_default();

            Ok(Job {
                id: JobId(id),
                name,
                schedule,
                command,
                args,
                env,
                enabled,
                owner,
                retry_policy,
                resource_limits,
                jitter_seconds: jitter_seconds as u64,
                timezone,
                tags,
                dependencies,
                hooks,
                max_concurrent: max_concurrent as u32,
                priority,
                execution_mode,
                notification_config,
            })
        })?;

        let mut jobs = HashMap::new();
        for job in job_iter {
            let job = job?;
            jobs.insert(job.id.0.clone(), job);
        }
        Ok(jobs)
    }

    pub fn log_history(&self, job_id: &str, status: &str, output: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO history (job_id, status, output) VALUES (?1, ?2, ?3)",
            params![job_id, status, output],
        )?;
        Ok(())
    }

    pub fn get_history(&self, job_id: &str) -> Result<Vec<common::HistoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, job_id, run_at, status, output 
             FROM history 
             WHERE job_id = ?1 
             ORDER BY run_at DESC 
             LIMIT 100"
        )?;
        
        let history_iter = stmt.query_map(params![job_id], |row| {
            Ok(common::HistoryEntry {
                id: row.get(0)?,
                job_id: row.get(1)?,
                run_at: row.get(2)?,
                status: row.get(3)?,
                output: row.get(4)?,
            })
        })?;

        let mut history = Vec::new();
        for entry in history_iter {
            history.push(entry?);
        }
        Ok(history)
    }

    pub fn log_retry_attempt(&self, job_id: &str, attempt: u32, next_retry: Option<&str>, error: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO retry_attempts (job_id, attempt_number, next_retry_at, error) 
             VALUES (?1, ?2, ?3, ?4)",
            params![job_id, attempt, next_retry, error],
        )?;
        Ok(())
    }

    pub fn update_job_metrics(&self, job_id: &str, success: bool, duration_ms: i64) -> Result<()> {
        // Insert or update metrics
        self.conn.execute(
            "INSERT INTO job_metrics (job_id, total_runs, successful_runs, failed_runs, last_duration_ms, last_run_at)
             VALUES (?1, 1, ?2, ?3, ?4, datetime('now'))
             ON CONFLICT(job_id) DO UPDATE SET
                total_runs = total_runs + 1,
                successful_runs = successful_runs + ?2,
                failed_runs = failed_runs + ?3,
                last_duration_ms = ?4,
                avg_duration_ms = (avg_duration_ms * total_runs + ?4) / (total_runs + 1),
                last_run_at = datetime('now')",
            params![job_id, if success { 1 } else { 0 }, if success { 0 } else { 1 }, duration_ms],
        )?;
        Ok(())
    }
}
