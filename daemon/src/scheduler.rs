use common::{Job, ScheduleConfig};
use cron::Schedule;
use std::str::FromStr;
use chrono::{Utc, DateTime, Duration, Timelike};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::db::Db;
use dashmap::DashMap;
use uuid::Uuid;
use sysinfo::{System, ProcessRefreshKind};

/// Calculate next retry delay based on backoff strategy
fn calculate_backoff_delay(
    attempt: u32,
    strategy: &common::BackoffStrategy,
    initial_delay: u64,
    max_delay: u64,
) -> u64 {
    use common::BackoffStrategy;
    
    let delay = match strategy {
        BackoffStrategy::Fixed => initial_delay,
        BackoffStrategy::Linear => initial_delay * (attempt as u64 + 1),
        BackoffStrategy::Exponential => {
            let base_delay = initial_delay * 2_u64.pow(attempt);
            base_delay
        },
    };
    
    delay.min(max_delay)
}

/// Monitor and enforce timeout for a process
async fn enforce_timeout(
    pid: u32,
    timeout_seconds: u64,
) -> Result<(), &'static str> {
    let duration = std::time::Duration::from_secs(timeout_seconds);
    
    tokio::time::sleep(duration).await;
    
    // Check if process is still running
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessRefreshKind::everything());
    
    if system.process(sysinfo::Pid::from_u32(pid)).is_some() {
        // Process still running, kill it
        log::warn!("Process {} exceeded timeout of {}s, terminating", pid, timeout_seconds);
        
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;
        let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
        
        // Give it a moment to clean up
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        
        // Force kill if still alive
        system.refresh_processes_specifics(ProcessRefreshKind::everything());
        if system.process(sysinfo::Pid::from_u32(pid)).is_some() {
            let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
        }
        
        return Err("Process timeout exceeded");
    }
    
    Ok(())
}

#[derive(Debug, Clone)]
pub struct JobExecutionContext {
    pub execution_id: String,
    pub scheduled_time: DateTime<Utc>,
    pub start_time: DateTime<Utc>,
    pub pid: Option<u32>,
}

pub struct Scheduler {
    pub jobs: HashMap<String, Job>,
    pub last_runs: HashMap<String, DateTime<Utc>>,
    pub last_execution_windows: HashMap<String, DateTime<Utc>>, // Track scheduled window to prevent duplicates
    pub running_jobs: Arc<DashMap<String, JobExecutionContext>>, // Enhanced with execution context
    pub db: Option<Arc<Mutex<Db>>>,
    pub retry_state: HashMap<String, RetryState>,
}

#[derive(Debug, Clone)]
pub struct RetryState {
    pub attempt: u32,
    pub next_attempt_at: Option<DateTime<Utc>>,
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
            last_execution_windows: HashMap::new(),
            running_jobs: Arc::new(DashMap::new()),
            db,
            retry_state: HashMap::new(),
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
        
        // Check for scheduled retries
        let retry_jobs: Vec<String> = self.retry_state.iter()
            .filter_map(|(job_id, state)| {
                if let Some(next_attempt) = state.next_attempt_at {
                    if next_attempt <= now {
                        Some(job_id.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        
        for job_id in retry_jobs {
            if let Some(job) = self.jobs.get(&job_id) {
                if !self.running_jobs.contains_key(&job_id) {
                    log::info!("Retrying job: {} (attempt {})", job.name, 
                        self.retry_state.get(&job_id).map(|s| s.attempt + 1).unwrap_or(1));
                    
                    let execution_id = Uuid::new_v4().to_string();
                    let now = Utc::now();
                    
                    jobs_to_run.push(job.clone());
                    self.running_jobs.insert(
                        job_id.clone(),
                        JobExecutionContext {
                            execution_id,
                            scheduled_time: now,
                            start_time: now,
                            pid: None,
                        },
                    );
                }
            }
        }
        
        for job in self.jobs.values() {
            if !job.enabled {
                continue;
            }

            // Concurrency check - use contains_key instead of hashset
            if self.running_jobs.contains_key(&job.id.0) {
                continue;
            }

            let last_run = self.last_runs.get(&job.id.0).cloned().unwrap_or(DateTime::<Utc>::MIN_UTC);
            let mut next_run_time = now;

            let should_run = match &job.schedule {
                ScheduleConfig::Cron(expression) => {
                    if let Ok(schedule) = Schedule::from_str(expression) {
                        let start_time = if last_run == DateTime::<Utc>::MIN_UTC {
                            now - Duration::seconds(1)
                        } else {
                            last_run
                        };

                        if let Some(next) = schedule.after(&start_time).next() {
                            if next <= now {
                                next_run_time = next;
                                true
                            } else {
                                false
                            }
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
                        next_run_time = now;
                        true 
                    } else {
                        let expected = last_run + interval;
                        if expected <= now {
                            next_run_time = expected;
                            // Lag check: if we are behind by more than 10 intervals, reset to now
                            if (now - expected) > (interval * 10) {
                                log::warn!("Job {} is lagging significantly. Resetting schedule.", job.name);
                                next_run_time = now;
                            }
                            true
                        } else {
                            false
                        }
                    }
                },
                ScheduleConfig::Calendar(params) => {
                    // Use configured timezone or Local time for calendar matching
                    let now_local = if let Some(ref tz_str) = job.timezone {
                        use chrono_tz::Tz;
                        if let Ok(tz) = tz_str.parse::<Tz>() {
                            now.with_timezone(&tz).naive_local()
                        } else {
                            log::warn!("Invalid timezone '{}' for job {}, using local time", tz_str, job.name);
                            chrono::Local::now().naive_local()
                        }
                    } else {
                        chrono::Local::now().naive_local()
                    };
                    
                    // CRITICAL BUG FIX: Use minute-level precision and execution window tracking
                    // Create a window identifier based on the current minute (not second)
                    let current_window = now_local.with_second(0).unwrap().with_nanosecond(0).unwrap();
                    let last_window = self.last_execution_windows
                        .get(&job.id.0)
                        .and_then(|dt| {
                            if let Some(ref tz_str) = job.timezone {
                                use chrono_tz::Tz;
                                if let Ok(tz) = tz_str.parse::<Tz>() {
                                    Some(dt.with_timezone(&tz).naive_local().with_second(0).unwrap().with_nanosecond(0).unwrap())
                                } else {
                                    None
                                }
                            } else {
                                Some(dt.with_timezone(&chrono::Local).naive_local().with_second(0).unwrap().with_nanosecond(0).unwrap())
                            }
                        });
                    
                    // Prevent running twice in the same minute window
                    if let Some(last_win) = last_window {
                        if last_win == current_window {
                            false
                        } else {
                            use chrono::{Datelike, Timelike};
                            let (h, m, s) = params.time;
                            
                            if now_local.hour() == h && now_local.minute() == m && now_local.second() == s {
                                let mut day_match = true;
                                
                                if let Some(days) = &params.days_of_week {
                                    let current_iso_day = now_local.weekday().number_from_monday();
                                    if !days.contains(&current_iso_day) {
                                        day_match = false;
                                    }
                                }
                                
                                if let Some((n, weekday)) = params.nth_weekday {
                                    let current_iso_day = now_local.weekday().number_from_monday();
                                    if current_iso_day != weekday {
                                        day_match = false;
                                    } else {
                                        let day = now_local.day();
                                        let week_num = (day - 1) / 7 + 1;
                                        if week_num != n {
                                            day_match = false;
                                        }
                                    }
                                }
                                
                                if day_match {
                                    next_run_time = now;
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        }
                    } else {
                        // First run or no execution window recorded
                        use chrono::{Datelike, Timelike};
                        let (h, m, s) = params.time;
                        
                        if now_local.hour() == h && now_local.minute() == m && now_local.second() == s {
                            let mut day_match = true;
                            
                            if let Some(days) = &params.days_of_week {
                                let current_iso_day = now_local.weekday().number_from_monday();
                                if !days.contains(&current_iso_day) {
                                    day_match = false;
                                }
                            }
                            
                            if let Some((n, weekday)) = params.nth_weekday {
                                let current_iso_day = now_local.weekday().number_from_monday();
                                if current_iso_day != weekday {
                                    day_match = false;
                                } else {
                                    let day = now_local.day();
                                    let week_num = (day - 1) / 7 + 1;
                                    if week_num != n {
                                        day_match = false;
                                    }
                                }
                            }
                            
                            if day_match {
                                next_run_time = now;
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                },
            };

            if should_run {
                // Apply jitter if configured
                if job.jitter_seconds > 0 {
                    use rand::Rng;
                    let jitter_ms = rand::thread_rng().gen_range(0..job.jitter_seconds * 1000);
                    next_run_time = next_run_time + Duration::milliseconds(jitter_ms as i64);
                    log::debug!("Applied jitter of {}ms to job {}", jitter_ms, job.name);
                }
                
                // Create execution context
                let execution_id = Uuid::new_v4().to_string();
                log::info!("Scheduling job: {} (execution_id: {})", job.name, execution_id);
                
                jobs_to_run.push(job.clone());
                self.last_runs.insert(job.id.0.clone(), next_run_time);
                self.last_execution_windows.insert(job.id.0.clone(), next_run_time);
                
                // Insert execution context
                self.running_jobs.insert(
                    job.id.0.clone(),
                    JobExecutionContext {
                        execution_id,
                        scheduled_time: next_run_time,
                        start_time: now,
                        pid: None,
                    },
                );
            }
        }
        jobs_to_run
    }

    pub fn finish_job(&mut self, id: &str) {
        self.running_jobs.remove(id);
    }

    pub fn execute_job(scheduler: Arc<Mutex<Scheduler>>, job: &Job) {
        let (current_attempt, db, retry_policy, hooks) = {
            let sched = scheduler.lock().unwrap();
            let current_attempt = sched.retry_state.get(&job.id.0).map(|s| s.attempt).unwrap_or(0);
            let db = sched.db.clone();
            (current_attempt, db, job.retry_policy.clone(), job.hooks.clone())
        };
        
        log::info!("Executing job: {} (owner: {}, attempt: {})", job.name, job.owner, current_attempt + 1);
        
        
        // Construct full command string with args
        let full_command = if job.args.is_empty() {
            job.command.clone()
        } else {
            format!("{} {}", job.command, job.args.join(" "))
        };
        
        // Prepare command with proper user switching using sudo
        let mut cmd = tokio::process::Command::new("/usr/bin/sudo");
        
        // Run as specified user (defaults to "lunasched" if not specified)
        let user = if job.owner.is_empty() { "lunasched" } else { &job.owner };
        cmd.arg("-u");
        cmd.arg(user);
        
        // Use shell to execute the command
        cmd.arg("/bin/sh");
        cmd.arg("-c");
        cmd.arg(&full_command);
        
        // Set environment variables (sudo will pass them through)
        cmd.envs(&job.env);
        
        // Set working directory to /tmp (always accessible)
        cmd.current_dir("/tmp");
        
        log::info!("Executing as user '{}': /bin/sh -c '{}'", user, full_command);

        // Configure I/O
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        
        // Apply resource limits if configured
        let resource_limits = job.resource_limits.clone();

        let job_name = job.name.clone();
        let job_id = job.id.0.clone();


        match cmd.spawn() {
            Ok(child) => {
                let pid = child.id().unwrap();
                
                // Spawn timeout enforcer if configured
                if let Some(timeout_secs) = resource_limits.timeout_seconds {
                    let pid_clone = pid;
                    tokio::spawn(async move {
                        if let Err(e) = enforce_timeout(pid_clone, timeout_secs).await {
                            log::warn!("Timeout enforced: {}", e);
                        }
                    });
                }
                
                tokio::spawn(async move {
                    let start_time = std::time::Instant::now();
                    match child.wait_with_output().await {
                        Ok(output) => {
                            let duration_ms = start_time.elapsed().as_millis() as i64;
                            let success = output.status.success();
                            let exit_code = output.status.code().unwrap_or(-1);
                            
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            let log_output = format!("Stdout:\n{}\nStderr:\n{}", stdout, stderr);
                            
                            let status_str = if success { "success" } else { "failed" };
                            log::info!("Job {} finished with status: {} (exit code: {}, duration: {}ms)", 
                                job_name, status_str, exit_code, duration_ms);
                            log::info!(target: "job_output", "Job: {}\n{}", job_name, log_output);

                            // Log to database if configured
                            if let Some(ref db) = db {
                                // Metrics removed - keeping only history logging
                            }

                            if success {
                                // Job succeeded - clear retry state and run success hook
                                {
                                    let mut sched = scheduler.lock().unwrap();
                                    sched.retry_state.remove(&job_id);
                                }
                                
                                if let Some(ref db) = db {
                                    let _ = db.lock().unwrap().log_history(&job_id, status_str, &log_output);
                                }
                                
                                // Run success hook if configured
                                if let Some(on_success) = hooks.on_success {
                                    log::info!("Running success hook for job {}", job_name);
                                    let _ = std::process::Command::new("sh")
                                        .arg("-c")
                                        .arg(&on_success)
                                        .spawn();
                                }
                            } else {
                                // Job failed - check retry policy
                                let should_retry = current_attempt < retry_policy.max_attempts;
                                
                                if should_retry {
                                    let next_attempt = current_attempt + 1;
                                    let delay_secs = calculate_backoff_delay(
                                        current_attempt,
                                        &retry_policy.backoff_strategy,
                                        retry_policy.initial_delay_seconds,
                                        retry_policy.max_delay_seconds,
                                    );
                                    
                                    let next_attempt_at = Utc::now() + Duration::seconds(delay_secs as i64);
                                    log::warn!("Job {} failed (attempt {}/{}). Retrying in {}s", 
                                        job_name, next_attempt, retry_policy.max_attempts, delay_secs);
                                    
                                    {
                                        let mut sched = scheduler.lock().unwrap();
                                        sched.retry_state.insert(job_id.clone(), RetryState {
                                            attempt: next_attempt,
                                            next_attempt_at: Some(next_attempt_at),
                                        });
                                    }
                                    
                                    if let Some(ref db) = db {
                                        let next_retry_str = next_attempt_at.format("%Y-%m-%d %H:%M:%S").to_string();
                                        let _ = db.lock().unwrap().log_retry_attempt(
                                            &job_id,
                                            next_attempt,
                                            Some(&next_retry_str),
                                            &format!("Exit code: {}", exit_code)
                                        );
                                    }
                                } else {
                                    // All retries exhausted
                                    log::error!("Job {} failed after {} attempts", job_name, current_attempt + 1);
                                    {
                                        let mut sched = scheduler.lock().unwrap();
                                        sched.retry_state.remove(&job_id);
                                    }
                                    
                                    if let Some(ref db) = db {
                                        let _ = db.lock().unwrap().log_history(&job_id, "failed", &log_output);
                                    }
                                    
                                    // Run failure hook if configured
                                    if let Some(on_failure) = hooks.on_failure {
                                        log::info!("Running failure hook for job {}", job_name);
                                        let _ = std::process::Command::new("sh")
                                            .arg("-c")
                                            .arg(&on_failure)
                                            .spawn();
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let err_msg = format!("Failed to wait: {}", e);
                            log::error!("Job {} {}", job_name, err_msg);
                            
                            if let Some(ref db) = db {
                                let _ = db.lock().unwrap().log_history(&job_id, "Error", &err_msg);
                            }
                        },
                    }
                    
                    // Mark job as finished
                    scheduler.lock().unwrap().finish_job(&job_id);
                });
            }
            Err(e) => {
                let err_msg = format!("Failed to spawn: {}", e);
                log::error!("Failed to spawn job {}: {}", job.name, e);
                
                if let Some(ref db) = db {
                    let _ = db.lock().unwrap().log_history(&job_id, "SpawnError", &err_msg);
                }
                
                scheduler.lock().unwrap().finish_job(&job_id);
            },
        }
    }
}
