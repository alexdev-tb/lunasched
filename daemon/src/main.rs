mod scheduler;
mod db;
mod migrations;

use tokio::net::UnixListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use common::{Request, Response};
use std::sync::{Arc, Mutex};
use scheduler::Scheduler;
use db::Db;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Set up panic handler BEFORE anything else
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info.location()
            .map(|l| format!(" at {}:{}", l.file(), l.line()))
            .unwrap_or_else(|| String::from(""));
        let payload = panic_info.payload()
            .downcast_ref::<&str>()
            .map(|s| *s)
            .or_else(|| panic_info.payload().downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("<no message>");
        
        log::error!("PANIC{}: {}", location, payload);
        eprintln!("FATAL: Daemon panicked{}: {}", location, payload);
        eprintln!("Check logs at: {}", common::DEFAULT_LOG_FILE);
    }));
    
    setup_logging()?;
    log::info!("Starting lunasched-daemon v{}...", env!("CARGO_PKG_VERSION"));

    let db_path = common::DEFAULT_DB_PATH;
    
    // Ensure parent directories exist
    if let Some(parent) = std::path::Path::new(db_path).parent() {
        if !parent.exists() {
            log::info!("Creating database directory: {}", parent.display());
            std::fs::create_dir_all(parent).map_err(|e| {
                log::error!("Failed to create database directory: {}", e);
                anyhow::anyhow!("Failed to create database directory: {}", e)
            })?;
        }
    }
    
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
            log::warn!("Continuing without database - jobs will not persist");
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
                // Don't hold lock while executing jobs!
                tokio::spawn(async move {
                    // Execute job without holding lock
                    Scheduler::execute_job(s.clone(), &job);
                });
            }
        }
    });

    // Set up signal handling for graceful shutdown
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    
    log::info!("Daemon initialization complete, ready to accept connections");

    // Main accept loop with graceful shutdown
    loop {
        tokio::select! {
            // Handle incoming connections
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((mut socket, addr)) => {
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

                            // Read complete message with proper buffering
                            let mut complete_buf = Vec::new();
                            let mut temp_buf = vec![0; 8192];
                            
                            loop {
                                let n = match socket.read(&mut temp_buf).await {
                                    Ok(0) => {
                                        if complete_buf.is_empty() {
                                            return;  // Connection closed
                                        }
                                        break;  // EOF, process what we have
                                    }
                                    Ok(n) => n,
                                    Err(e) => {
                                        log::error!("failed to read from socket; err = {:?}", e);
                                        return;
                                    }
                                };
                                
                                complete_buf.extend_from_slice(&temp_buf[0..n]);
                                
                                // Try to parse - if successful, we have a complete message
                                if let Ok(req) = serde_json::from_slice::<Request>(&complete_buf) {
                                    // Process the request
                                    let mut request = req;
                                    let requester_owner = if peer_uid == 0 { "root" } else { "lunasched" };

                                    // Override owner for AddJob
                                    if let Request::AddJob(ref mut job) = request {
                                        job.owner = requester_owner.to_string();
                                    }

                                    log::info!("Received request: {:?}", request);
                                    
                                    let resp = match request {
                                        Request::AddJob(job) => {
                                            let response = {
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
                                            };
                                            response
                                        },
                                        Request::ListJobs => {
                                            let jobs = {
                                                let sched = scheduler.lock().unwrap();
                                                sched.jobs.values().cloned().collect()
                                            };
                                            Response::JobList(jobs)
                                        },
                                        Request::StartJob(job_id) => {
                                            let response = {
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
                                                         drop(sched);  // Drop lock before executing job
                                                         Scheduler::execute_job(s, &job_clone);
                                                         Response::Ok
                                                     }
                                                } else {
                                                    Response::Error("Job not found".to_string())
                                                }
                                            };
                                            response
                                        },
                                        Request::RemoveJob(id) => {
                                            let response = {
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
                                            };
                                            response
                                        },
                                        Request::GetJob(id) => {
                                            let job_opt = {
                                                let sched = scheduler.lock().unwrap();
                                                sched.jobs.get(&id.0).cloned()
                                            };
                                            Response::JobDetail(job_opt)
                                        },
                                        Request::GetHistory { job_id, limit } => {
                                            let sched = scheduler.lock().unwrap();
                                            if let Some(ref db) = sched.db {
                                                match db.lock().unwrap().get_history(&job_id.0, limit) {
                                                    Ok(history) => Response::HistoryList(history),
                                                    Err(e) => Response::Error(format!("DB Error: {}", e)),
                                                }
                                            } else {
                                                Response::Error("No database configured".to_string())
                                            }
                                        },
                                    };
                                    
                                    log::debug!("About to serialize response: {:?}", resp);
                                    let resp_bytes = serde_json::to_vec(&resp).unwrap();
                                    log::debug!("Response serialized, {} bytes", resp_bytes.len());

                                    if let Err(e) = socket.write_all(&resp_bytes).await {
                                        log::error!("failed to write to socket; err = {:?}", e);
                                        return;
                                    }
                                    
                                    // Clear buffer for next request
                                    complete_buf.clear();
                                    continue;
                                }
                                
                                // If buffer grows too large, something is wrong
                                if complete_buf.len() > 1024 * 1024 {  // 1MB limit
                                    log::error!("Request too large: {} bytes", complete_buf.len());
                                    return;
                                }
                            }

                        });
                    }
                    Err(e) => {
                        log::error!("Accept error: {}", e);
                        // Continue on accept errors instead of crashing
                        continue;
                    }
                }
            },
            // Handle SIGTERM
            _ = sigterm.recv() => {
                log::info!("Received SIGTERM, initiating graceful shutdown...");
                break;
            },
            // Handle SIGINT (Ctrl+C)
            _ = sigint.recv() => {
                log::info!("Received SIGINT, initiating graceful shutdown...");
                break;
            },
        }
    }
    
    // Cleanup
    log::info!("Graceful shutdown complete");
    if let Err(e) = std::fs::remove_file(socket_path) {
        log::warn!("Failed to remove socket file: {}", e);
    }
    
    Ok(())
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