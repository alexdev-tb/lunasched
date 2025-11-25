use rusqlite::{params, Connection, Result};
use common::{Job, ScheduleConfig, JobId};
use std::collections::HashMap;

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        
        let _ = conn.execute("ALTER TABLE jobs ADD COLUMN owner TEXT DEFAULT 'root'", []);

        conn.execute(
            "CREATE TABLE IF NOT EXISTS jobs (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                schedule_type TEXT NOT NULL,
                schedule_value TEXT NOT NULL,
                command TEXT NOT NULL,
                args TEXT NOT NULL,
                env TEXT NOT NULL,
                enabled BOOLEAN NOT NULL,
                owner TEXT NOT NULL
            )",
            [],
        )?;
        
        conn.execute(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY,
                job_id TEXT NOT NULL,
                run_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                status TEXT NOT NULL,
                output TEXT
            )",
            [],
        )?;
        
        Ok(Self { conn })
    }

    pub fn add_job(&self, job: &Job) -> Result<()> {
        let (sched_type, sched_val) = match &job.schedule {
            ScheduleConfig::Cron(s) => ("cron", s.clone()),
            ScheduleConfig::Every(s) => ("every", s.to_string()),
        };
        
        let args_json = serde_json::to_string(&job.args).unwrap();
        let env_json = serde_json::to_string(&job.env).unwrap();

        self.conn.execute(
            "INSERT OR REPLACE INTO jobs (id, name, schedule_type, schedule_value, command, args, env, enabled, owner)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![job.id.0, job.name, sched_type, sched_val, job.command, args_json, env_json, job.enabled, job.owner],
        )?;
        Ok(())
    }

    pub fn remove_job(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM jobs WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn load_jobs(&self) -> Result<HashMap<String, Job>> {
        let mut stmt = self.conn.prepare("SELECT id, name, schedule_type, schedule_value, command, args, env, enabled, owner FROM jobs")?;
        let job_iter = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let sched_type: String = row.get(2)?;
            let sched_val: String = row.get(3)?;
            let command: String = row.get(4)?;
            let args_json: String = row.get(5)?;
            let env_json: String = row.get(6)?;
            let enabled: bool = row.get(7)?;
            let owner: String = row.get(8).unwrap_or("root".to_string()); // Handle missing owner if any

            let schedule = match sched_type.as_str() {
                "cron" => ScheduleConfig::Cron(sched_val),
                "every" => ScheduleConfig::Every(sched_val.parse().unwrap_or(0)),
                _ => ScheduleConfig::Cron(sched_val), // Fallback
            };

            let args: Vec<String> = serde_json::from_str(&args_json).unwrap_or_default();
            let env: HashMap<String, String> = serde_json::from_str(&env_json).unwrap_or_default();

            Ok(Job {
                id: JobId(id),
                name,
                schedule,
                command,
                args,
                env,
                enabled,
                owner,
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
        let mut stmt = self.conn.prepare("SELECT id, job_id, run_at, status, output FROM history WHERE job_id = ?1 ORDER BY run_at DESC LIMIT 10")?;
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
}
