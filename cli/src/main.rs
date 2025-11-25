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
        /// Cron schedule
        #[arg(long)]
        cron: Option<String>,
        /// Every X duration (e.g. "5s", "10m")
        #[arg(long)]
        every: Option<String>,
        /// Command to run
        #[arg(short, long)]
        command: String,
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

    let mut stream = UnixStream::connect(socket_path).await?;

    let req = match cli.command {
        Commands::Add { name, cron, every, command, args } => {
            let schedule = if let Some(c) = cron {
                common::ScheduleConfig::Cron(c)
            } else if let Some(e) = every {
                common::parse_schedule(&format!("every {}", e))?
            } else {
                return Err(anyhow::anyhow!("Must specify --cron or --every"));
            };

            let job = Job {
                id: JobId(name.clone()), // Use name as ID for now
                name,
                schedule,
                command,
                args,
                env: HashMap::new(),
                enabled: true,
                owner: String::new(), // Daemon will set this
            };
            Request::AddJob(job)
        },
        Commands::List => Request::ListJobs,
        Commands::Start { id } => Request::StartJob(JobId(id)),
        Commands::History { id } => Request::GetHistory(JobId(id)),
        Commands::Remove { id } => Request::RemoveJob(JobId(id)),
        Commands::Get { id } => Request::GetJob(JobId(id)),
    };

    let req_bytes = serde_json::to_vec(&req)?;
    stream.write_all(&req_bytes).await?;

    let mut buf = vec![0; 4096];
    let n = stream.read(&mut buf).await?;
    let resp: Response = serde_json::from_slice(&buf[0..n])?;

    match resp {
        Response::Ok => println!("Success"),
        Response::Error(e) => eprintln!("Error: {}", e),
        Response::JobList(jobs) => {
            println!("{:<20} {:<20} {:<20}", "ID", "Schedule", "Command");
            for job in jobs {
                let schedule_str = match job.schedule {
                    common::ScheduleConfig::Cron(s) => s,
                    common::ScheduleConfig::Every(s) => format!("every {}s", s),
                };
                println!("{:<20} {:<20} {:<20}", job.id, schedule_str, job.command);
            }
        },
        Response::HistoryList(history) => {
            println!("{:<20} {:<20} {:<10} {:<20}", "Run At", "Status", "Output (Preview)", "Job ID");
            for entry in history {
                let output_preview = entry.output.as_deref().unwrap_or("").lines().next().unwrap_or("").chars().take(20).collect::<String>();
                println!("{:<20} {:<20} {:<10} {:<20}", entry.run_at, entry.status, output_preview, entry.job_id);
            }
        },
        Response::JobDetail(job_opt) => {
            if let Some(job) = job_opt {
                println!("Job Details:");
                println!("  ID:       {}", job.id);
                println!("  Name:     {}", job.name);
                println!("  Schedule: {:?}", job.schedule);
                println!("  Command:  {}", job.command);
                println!("  Args:     {:?}", job.args);
                println!("  Enabled:  {}", job.enabled);
                println!("  Owner:    {}", job.owner);
            } else {
                println!("Job not found.");
            }
        },
    }

    Ok(())
}
