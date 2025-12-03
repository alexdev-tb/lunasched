mod scheduler;
mod db;
mod migrations;
mod config;
mod resource_manager;
mod notifier;
mod metrics;

use tokio::net::UnixListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use common::{Request, Response};
use std::sync::{Arc, Mutex};
use scheduler::Scheduler;
use db::Db;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    setup_logging()?;
    log::info!("Starting lunasched-daemon...");

    let db_path = common::DEFAULT_DB_PATH;
    
    // Open database and run migrations
    let db = match rusqlite::Connection::open(db_path) {
        Ok(conn) => {
            log::info!("Database opened at {}", db_path);
            let mut migrator = migrations::Migrator::new(conn);
            if let Err(e) = migrator.run_migrations() {
                log::error!("Failed to run database migrations: {}", e);
                return Err(anyhow::anyhow!("Migration failed: {}", e));
            }
            let conn = migrator.into_connection();
            Some(Arc::new(Mutex::new(Db::from_connection(conn))))
        },
        Err(e) => {
            log::error!("Failed to open database at {}: {}", db_path, e);
            None
        }
    };

    let scheduler = Arc::new(Mutex::new(Scheduler::new(db)));
    let socket_path = common::DEFAULT_SOCKET_PATH;

    // Ensure parent directory exists (critical for /var/run/lunasched after reboot)
    if let Some(parent) = std::path::Path::new(socket_path).parent() {
        if !parent.exists() {
            log::info!("Creating socket directory: {}", parent.display());
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::error!("Failed to create socket directory {}: {}", parent.display(), e);
                return Err(anyhow::anyhow!("Failed to create socket directory: {}", e));
            }
            
            // Set directory permissions to allow all users to access
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(parent)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(parent, perms)?;
            log::info!("Socket directory created with permissions 0755");
        }
    }

    // Remove stale socket file if it exists
    if std::path::Path::new(socket_path).exists() {
        log::info!("Removing stale socket file: {}", socket_path);
        std::fs::remove_file(socket_path)?;
    }

    // Bind to socket
    let listener = match UnixListener::bind(socket_path) {
        Ok(listener) => {
            log::info!("Successfully bound to socket: {}", socket_path);
            listener
        },
        Err(e) => {
            log::error!("Failed to bind to socket {}: {}", socket_path, e);
            log::error!("Possible causes: insufficient permissions, path issues, or another instance running");
            return Err(anyhow::anyhow!("Failed to bind to socket: {}", e));
        }
    };
    
    println!("Listening on {}", socket_path);
    
    // Set socket permissions to allow all users to connect
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(socket_path)?.permissions();
    perms.set_mode(0o666);
    std::fs::set_permissions(socket_path, perms)?;
    log::info!("Socket permissions set to 0666");

    // Spawn scheduler tick loop
    let tick_scheduler = scheduler.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            let mut sched = tick_scheduler.lock().unwrap();
            let jobs = sched.tick();
            
            drop(sched);

            for job in jobs {
                let s = tick_scheduler.clone();
                tokio::spawn(async move {
                    let sched = s.lock().unwrap();
                    sched.execute_job(&job, s.clone());
                });
            }
        }
    });

    loop {
        let (mut socket, addr) = listener.accept().await?;
        log::info!("New connection accepted from {:?}", addr);
        let scheduler = scheduler.clone();

        tokio::spawn(async move {
            let peer_uid = match socket.peer_cred() {
                Ok(cred) => cred.uid(),
                Err(e) => {
                    log::error!("Failed to get peer credentials: {}", e);
                    return;
                }
            };

            let mut buf = vec![0; 1024];
            loop {
                let n = match socket.read(&mut buf).await {
                    Ok(n) if n == 0 => return,
                    Ok(n) => n,
                    Err(e) => {
                        log::error!("failed to read from socket; err = {:?}", e);
                        return;
                    }
                };

                let mut req: Request = match serde_json::from_slice(&buf[0..n]) {
                    Ok(req) => req,
                    Err(e) => {
                        log::error!("failed to deserialize request; err = {:?}", e);
                        log::error!("Raw bytes received: {} bytes", n);
                        return;
                    }
                };

                log::info!("Received request: {:?}", req);
                
                let requester_owner = if peer_uid == 0 { "root" } else { "lunasched" };

                // Override owner for AddJob
                if let Request::AddJob(ref mut job) = req {
                    job.owner = requester_owner.to_string();
                }
                
                let resp = match req {
                    Request::AddJob(job) => {
                        let mut sched = scheduler.lock().unwrap();
                        // Check if job exists and verify ownership
                        if let Some(existing) = sched.jobs.get(&job.id.0) {
                            if existing.owner != requester_owner && requester_owner != "root" {
                                Response::Error(format!("Permission denied: Cannot overwrite job owned by {}", existing.owner))
                            } else {
                                sched.add_job(job);
                                Response::Ok
                            }
                        } else {
                            sched.add_job(job);
                            Response::Ok
                        }
                    },
                    Request::ListJobs => {
                        let sched = scheduler.lock().unwrap();
                        let jobs = sched.jobs.values().cloned().collect();
                        Response::JobList(jobs)
                    },
                    Request::StartJob(job_id) => {
                        let sched = scheduler.lock().unwrap();
                        if let Some(job) = sched.jobs.get(&job_id.0) {
                             if job.owner != requester_owner && requester_owner != "root" {
                                 Response::Error(format!("Permission denied: Cannot start job owned by {}", job.owner))
                             } else if sched.running_jobs.contains_key(&job_id.0) {
                                 Response::Error("Job is already running".to_string())
                             } else {
                                 let job_clone = job.clone();
                                 
                                 // Create execution context for manual start
                                 let execution_id = uuid::Uuid::new_v4().to_string();
                                 let now = chrono::Utc::now();
                                 sched.running_jobs.insert(
                                     job_id.0.clone(),
                                     scheduler::JobExecutionContext {
                                         execution_id: execution_id.clone(),
                                         scheduled_time: now,
                                         start_time: now,
                                         pid: None,
                                     },
                                 );
                                 
                                 log::info!("Manually starting job: {} (execution_id: {})", job_clone.name, execution_id);
                                 
                                 let s = scheduler.clone();
                                 sched.execute_job(&job_clone, s);
                                 Response::Ok
                             }
                        } else {
                            Response::Error("Job not found".to_string())
                        }
                    },
                    Request::RemoveJob(id) => {
                        let mut sched = scheduler.lock().unwrap();
                        if let Some(job) = sched.jobs.get(&id.0) {
                            if job.owner != requester_owner && requester_owner != "root" {
                                Response::Error(format!("Permission denied: Cannot remove job owned by {}", job.owner))
                            } else {
                                sched.remove_job(&id.0);
                                Response::Ok
                            }
                        } else {
                            Response::Error("Job not found".to_string())
                        }
                    },
                    Request::GetJob(id) => {
                        let sched = scheduler.lock().unwrap();
                        Response::JobDetail(sched.jobs.get(&id.0).cloned())
                    },
                    Request::GetHistory(job_id) => {
                        let sched = scheduler.lock().unwrap();
                        if let Some(ref db) = sched.db {
                            match db.lock().unwrap().get_history(&job_id.0) {
                                Ok(history) => Response::HistoryList(history),
                                Err(e) => Response::Error(format!("DB Error: {}", e)),
                            }
                        } else {
                            Response::Error("No database configured".to_string())
                        }
                    },
                };

                let resp_bytes = serde_json::to_vec(&resp).unwrap();

                if let Err(e) = socket.write_all(&resp_bytes).await {
                    eprintln!("failed to write to socket; err = {:?}", e);
                    return;
                }
            }
        });
    }
}

fn setup_logging() -> anyhow::Result<()> {
    let log_file = std::env::var("LUNASCHED_LOG").unwrap_or_else(|_| common::DEFAULT_LOG_FILE.to_string());
    let jobs_log_file = common::DEFAULT_JOBS_LOG_FILE;

    let base_config = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}][{}][{}] {}",
                chrono::Local::now().format("%Y-%m-%d][%H:%M:%S"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Info);

    // Main log file: Filter OUT job_output
    let main_log = fern::Dispatch::new()
        .filter(|metadata| metadata.target() != "job_output")
        .chain(std::io::stdout())
        .chain(fern::log_file(log_file)?);

    // Jobs log file: Filter IN job_output
    let jobs_log = fern::Dispatch::new()
        .filter(|metadata| metadata.target() == "job_output")
        .chain(fern::log_file(jobs_log_file)?);

    base_config
        .chain(main_log)
        .chain(jobs_log)
        .apply()?;
        
    Ok(())
}
