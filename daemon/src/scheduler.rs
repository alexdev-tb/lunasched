use common::{Job, ScheduleConfig};
use cron::Schedule;
use std::str::FromStr;
use chrono::{Utc, DateTime, Duration};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::db::Db;

pub struct Scheduler {
    pub jobs: HashMap<String, Job>,
    pub last_runs: HashMap<String, DateTime<Utc>>,
    pub db: Option<Arc<Mutex<Db>>>,
}

impl Scheduler {
    pub fn new(db: Option<Arc<Mutex<Db>>>) -> Self {
        let mut jobs = HashMap::new();
        if let Some(ref db) = db {
            if let Ok(loaded_jobs) = db.lock().unwrap().load_jobs() {
                jobs = loaded_jobs;
            }
        }
        
        Self {
            jobs,
            last_runs: HashMap::new(),
            db,
        }
    }

    pub fn add_job(&mut self, job: Job) {
        if let Some(ref db) = self.db {
            let _ = db.lock().unwrap().add_job(&job);
        }
        self.jobs.insert(job.id.0.clone(), job);
    }

    pub fn remove_job(&mut self, id: &str) -> bool {
        if let Some(ref db) = self.db {
            let _ = db.lock().unwrap().remove_job(id);
        }
        self.jobs.remove(id).is_some()
    }

    pub fn tick(&mut self) -> Vec<Job> {
        let mut jobs_to_run = Vec::new();
        let now = Utc::now();
        
        for job in self.jobs.values() {
            let last_run = self.last_runs.get(&job.id.0).cloned().unwrap_or_else(|| DateTime::<Utc>::MIN_UTC);
            
            let should_run = match &job.schedule {
                ScheduleConfig::Cron(expression) => {
                    if let Ok(schedule) = Schedule::from_str(expression) {
                        let start_time = if last_run == DateTime::<Utc>::MIN_UTC {
                            now - Duration::seconds(1)
                        } else {
                            last_run
                        };

                        if let Some(next) = schedule.after(&start_time).next() {
                            next <= now
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                },
                ScheduleConfig::Every(seconds) => {
                    let interval = Duration::seconds(*seconds as i64);
                    if last_run == DateTime::<Utc>::MIN_UTC {
                        true 
                    } else {
                        now - last_run >= interval
                    }
                },
            };

            if should_run {
                jobs_to_run.push(job.clone());
                self.last_runs.insert(job.id.0.clone(), now);
            }
        }
        jobs_to_run
    }

    pub fn execute_job(&self, job: &Job) {
        log::info!("Executing job: {} (owner: {})", job.name, job.owner);
        let mut cmd = tokio::process::Command::new(&job.command);
        cmd.args(&job.args);
        cmd.envs(&job.env);
        
        if job.owner == "lunasched" {
            match nix::unistd::User::from_name("lunasched") {
                Ok(Some(user)) => {
                    let uid = user.uid;
                    let gid = user.gid;
                    
                    unsafe {
                        cmd.pre_exec(move || {
                            let c_user = std::ffi::CString::new("lunasched").unwrap();
                            
                            // Set groups first (requires root)
                            nix::unistd::initgroups(&c_user, gid)
                                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                                
                            // Set GID
                            nix::unistd::setgid(gid)
                                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                                
                            // Set UID last (drops root)
                            nix::unistd::setuid(uid)
                                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                                
                            Ok(())
                        });
                    }
                },
                Ok(None) => {
                    log::error!("User 'lunasched' not found. Running as current user (likely root).");
                },
                Err(e) => {
                    log::error!("Failed to lookup user 'lunasched': {}", e);
                }
            }
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let job_name = job.name.clone();
        let job_id = job.id.0.clone();
        let db = self.db.clone();

        match cmd.spawn() {
            Ok(child) => {
                tokio::spawn(async move {
                    let child = child;
                    match child.wait_with_output().await {
                        Ok(output) => {
                            let status = output.status.to_string();
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            let log_output = format!("Stdout:\n{}\nStderr:\n{}", stdout, stderr);
                            
                            log::info!("Job {} finished with status: {}", job_name, status);
                            log::info!(target: "job_output", "Job: {}\n{}", job_name, log_output);

                            if let Some(db) = db {
                                let _ = db.lock().unwrap().log_history(&job_id, &status, &log_output);
                            }
                        }
                        Err(e) => {
                            let err_msg = format!("Failed to wait: {}", e);
                            log::error!("Job {} {}", job_name, err_msg);
                             if let Some(db) = db {
                                let _ = db.lock().unwrap().log_history(&job_id, "Error", &err_msg);
                            }
                        },
                    }
                });
            }
            Err(e) => {
                let err_msg = format!("Failed to spawn: {}", e);
                log::error!("Failed to spawn job {}: {}", job.name, e);
                 if let Some(db) = db {
                    let _ = db.lock().unwrap().log_history(&job_id, "SpawnError", &err_msg);
                }
            },
        }
    }
}
