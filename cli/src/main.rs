use clap::{Parser, Subcommand};
use common::{Job, JobId, Request, Response};
use tokio::net::UnixStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::collections::HashMap;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new job
    Add {
        /// Name of the job
        #[arg(short, long)]
        name: String,
        /// Schedule (e.g. "every 5s", "at 12:00", "*/5 * * * *")
        #[arg(long)]
        schedule: Option<String>,
        /// Cron schedule (deprecated, use --schedule)
        #[arg(long)]
        cron: Option<String>,
        /// Every X duration (deprecated, use --schedule)
        #[arg(long)]
        every: Option<String>,
        /// Command to run
        #[arg(short, long)]
        command: String,
        /// Max retry attempts (0 = no retries)
        #[arg(long, default_value = "0")]
        max_retries: u32,
        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
        /// Jitter in seconds (random delay)
        #[arg(long, default_value = "0")]
        jitter: u64,
        /// Timezone (e.g., "America/New_York")
        #[arg(long)]
        timezone: Option<String>,
        /// Tags (comma-separated)
        #[arg(long)]
        tags: Option<String>,
        /// Command to run on success
        #[arg(long)]
        on_success: Option<String>,
        /// Command to run on failure
        #[arg(long)]
        on_failure: Option<String>,
        /// Job priority (Low, Normal, High, Critical)
        #[arg(long, default_value = "Normal")]
        priority: String,
        /// Execution mode (Sequential, Parallel, Exclusive)
        #[arg(long, default_value = "Sequential")]
        execution_mode: String,
        /// Arguments
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// List all jobs
    List,
    /// Start a job manually
    Start {
        id: String,
    },
    /// View job history
    History {
        id: String,
        /// Show all history (default: last 5 executions)
        #[arg(long)]
        all: bool,
    },
    /// Remove a job
    Remove {
        id: String,
    },
    /// Get job details
    Get {
        id: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let socket_path = common::DEFAULT_SOCKET_PATH;

    // Add timeout to connection
    let mut stream = match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        UnixStream::connect(socket_path)
    ).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => {
            eprintln!("Failed to connect to daemon at {}: {}", socket_path, e);
            eprintln!("Is the lunasched daemon running? Try: sudo systemctl status lunasched");
            return Err(e.into());
        }
        Err(_) => {
            eprintln!("Connection timeout: daemon at {} is not responding", socket_path);
            eprintln!("Is the lunasched daemon running? Try: sudo systemctl status lunasched");
            return Err(anyhow::anyhow!("Connection timeout"));
        }
    };

    let req = match cli.command {
        Commands::Add { 
            name, schedule, cron, every, command, args,
            max_retries, timeout, jitter, timezone, tags,
            on_success, on_failure, priority, execution_mode
        } => {
            let schedule_config = if let Some(s) = schedule {
                common::parse_schedule(&s)?
            } else if let Some(c) = cron {
                common::ScheduleConfig::Cron(c)
            } else if let Some(e) = every {
                common::parse_schedule(&format!("every {}", e))?
            } else {
                return Err(anyhow::anyhow!("Must specify --schedule"));
            };

            let retry_policy = common::RetryPolicy {
                max_attempts: max_retries,
                backoff_strategy: common::BackoffStrategy::Exponential,
                initial_delay_seconds: 60,
                max_delay_seconds: 3600,
            };

            let resource_limits = common::ResourceLimits {
                timeout_seconds: timeout,
                max_memory_mb: None,
                cpu_quota: None,
            };

            let hooks = common::JobHooks {
                on_success,
                on_failure,
            };

            let tags_vec = tags.map(|t| 
                t.split(',').map(|s| s.trim().to_string()).collect()
            ).unwrap_or_default();

            // Parse priority
            let job_priority = match priority.to_lowercase().as_str() {
                "low" => common::JobPriority::Low,
                "normal" => common::JobPriority::Normal,
                "high" => common::JobPriority::High,
                "critical" => common::JobPriority::Critical,
                _ => {
                    return Err(anyhow::anyhow!("Invalid priority. Use: Low, Normal, High, or Critical"));
                }
            };

            // Parse execution mode
            let exec_mode = match execution_mode.to_lowercase().as_str() {
                "sequential" => common::ExecutionMode::Sequential,
                "parallel" => common::ExecutionMode::Parallel,
                "exclusive" => common::ExecutionMode::Exclusive,
                _ => {
                    return Err(anyhow::anyhow!("Invalid execution mode. Use: Sequential, Parallel, or Exclusive"));
                }
            };

            let job = Job {
                id: JobId(name.clone()),
                name,
                schedule: schedule_config,
                command,
                args,
                env: HashMap::new(),
                enabled: true,
                owner: String::new(),
                retry_policy,
                resource_limits,
                jitter_seconds: jitter,
                timezone,
                tags: tags_vec,
                dependencies: vec![],
                hooks,
                max_concurrent: 0,
                priority: job_priority,
                execution_mode: exec_mode,
                notification_config: common::NotificationConfig::default(),
            };
            Request::AddJob(job)
        },
        Commands::List => Request::ListJobs,
        Commands::Start { id } => Request::StartJob(JobId(id)),
        Commands::History { id, all } => Request::GetHistory { 
            job_id: JobId(id), 
            limit: if all { None } else { Some(5) } 
        },
        Commands::Remove { id } => Request::RemoveJob(JobId(id)),
        Commands::Get { id } => Request::GetJob(JobId(id)),
    };

    let req_bytes = serde_json::to_vec(&req)?;
    stream.write_all(&req_bytes).await?;

    // Read complete response with proper buffering
    let mut complete_buf = Vec::new();
    let mut temp_buf = vec![0; 8192];
    
    loop {
        let n = match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            stream.read(&mut temp_buf)
        ).await {
            Ok(Ok(0)) => break,  // EOF
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                eprintln!("Failed to read response from daemon: {}", e);
                return Err(e.into());
            }
            Err(_) => {
                eprintln!("Read timeout: daemon is not responding to the request");
                eprintln!("The daemon may be stuck or overloaded. Check logs at: {}", common::DEFAULT_LOG_FILE);
                return Err(anyhow::anyhow!("Read timeout"));
            }
        };
        
        complete_buf.extend_from_slice(&temp_buf[0..n]);
        
        // Try to parse - if successful, we have complete response
        if let Ok(resp) = serde_json::from_slice::<Response>(&complete_buf) {
            // Successfully parsed, handle response
            match resp {
        Response::Ok => println!("Success"),
        Response::Error(e) => eprintln!("Error: {}", e),
        Response::JobList(jobs) => {
            if jobs.is_empty() {
                println!("No jobs found.");
            } else {
                let mut table = comfy_table::Table::new();
                table.set_header(vec!["ID", "Name", "Schedule", "Command", "Enabled", "Owner"]);
                
                for job in jobs {
                    let schedule_str = match job.schedule {
                        common::ScheduleConfig::Cron(s) => s,
                        common::ScheduleConfig::Every(s) => format!("every {}s", s),
                        common::ScheduleConfig::Calendar(p) => {
                            let time = format!("{:02}:{:02}:{:02}", p.time.0, p.time.1, p.time.2);
                            if let Some(days) = p.days_of_week {
                                format!("on {:?} at {}", days, time)
                            } else if let Some((n, d)) = p.nth_weekday {
                                format!("on {}th day {} at {}", n, d, time)
                            } else {
                                format!("at {}", time)
                            }
                        }
                    };
                    
                    table.add_row(vec![
                        job.id.0,
                        job.name,
                        schedule_str,
                        job.command,
                        job.enabled.to_string(),
                        job.owner,
                    ]);
                }
                println!("{}", table);
            }
        },
        Response::HistoryList(history) => {
            if history.is_empty() {
                println!("No history found.");
            } else {
                let mut table = comfy_table::Table::new();
                table.set_header(vec!["Run At", "Job ID", "Status", "Output"]);
                
                for entry in history {
                    let output_str = entry.output.unwrap_or_default();
                    let output_preview: String = output_str.chars().take(50).collect();
                    let output_display = if output_str.len() > 50 {
                        format!("{}...", output_preview)
                    } else {
                        output_preview
                    };
                    
                    table.add_row(vec![
                        entry.run_at,
                        entry.job_id,
                        entry.status,
                        output_display.replace("\n", " "),
                    ]);
                }
                println!("{}", table);
            }
        },
        Response::JobDetail(job) => {
            if let Some(job) = job {
                use comfy_table::Cell;
                let mut table = comfy_table::Table::new();
                    table.add_row(vec![Cell::new("ID"), Cell::new(&job.id.0)]);
                    table.add_row(vec![Cell::new("Name"), Cell::new(&job.name)]);
                    table.add_row(vec![Cell::new("Command"), Cell::new(&job.command)]);
                    table.add_row(vec![Cell::new("Args"), Cell::new(&format!("{:?}", job.args))]);
                    table.add_row(vec![Cell::new("Enabled"), Cell::new(&job.enabled.to_string())]);
                    table.add_row(vec![Cell::new("Owner"), Cell::new(&job.owner)]);
                    table.add_row(vec![Cell::new("Priority"), Cell::new(&format!("{:?}", job.priority))]);
                    table.add_row(vec![Cell::new("Execution Mode"), Cell::new(&format!("{:?}", job.execution_mode))]);
                    table.add_row(vec![Cell::new("Schedule"), Cell::new(&format!("{:?}", job.schedule))]);
                    
                    if !job.tags.is_empty() {
                        table.add_row(vec![Cell::new("Tags"), Cell::new(&job.tags.join(", "))]);
                    }
                    if let Some(tz) = &job.timezone {
                        table.add_row(vec![Cell::new("Timezone"), Cell::new(tz)]);
                    }
                    if job.jitter_seconds > 0 {
                        table.add_row(vec![Cell::new("Jitter"), Cell::new(&format!("{}s", job.jitter_seconds))]);
                    }
                    if job.retry_policy.max_attempts > 0 {
                        table.add_row(vec![Cell::new("Max Retries"), Cell::new(&job.retry_policy.max_attempts.to_string())]);
                    }
                    if let Some(timeout) = job.resource_limits.timeout_seconds {
                        table.add_row(vec![Cell::new("Timeout"), Cell::new(&format!("{}s", timeout))]);
                    }
                
                println!("{}", table);
            } else {
                println!("Job not found.");
            }
        },
    }
            
            return Ok(());
        }
        
        // If buffer grows too large, something is wrong
        if complete_buf.len() > 10 * 1024 * 1024 {  // 10MB limit
            eprintln!("Response too large: {} bytes", complete_buf.len());
            return Err(anyhow::anyhow!("Response too large"));
        }
    }
    
    // If we get here, connection closed before complete response
    Err(anyhow::anyhow!("Connection closed before receiving complete response"))
}
