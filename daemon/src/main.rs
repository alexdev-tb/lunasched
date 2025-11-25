mod scheduler;
mod db;

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
    let db = match Db::new(db_path) {
        Ok(db) => Some(Arc::new(Mutex::new(db))),
        Err(e) => {
            log::error!("Failed to open database at {}: {}", db_path, e);
            None
        }
    };

    let scheduler = Arc::new(Mutex::new(Scheduler::new(db)));
    let socket_path = common::DEFAULT_SOCKET_PATH;

    if std::path::Path::new(socket_path).exists() {
        std::fs::remove_file(socket_path)?;
    }

    let listener = UnixListener::bind(socket_path)?;
    println!("Listening on {}", socket_path);
    
    // Set socket permissions to allow all users to connect
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(socket_path)?.permissions();
    perms.set_mode(0o666);
    std::fs::set_permissions(socket_path, perms)?;

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
                    sched.execute_job(&job);
                });
            }
        }
    });

    loop {
        let (mut socket, _) = listener.accept().await?;
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
                             } else {
                                 let job_clone = job.clone();
                                 sched.execute_job(&job_clone);
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
