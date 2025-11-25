use serde::{Deserialize, Serialize};
use crate::job::{Job, JobId};

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    AddJob(Job),
    RemoveJob(JobId),
    ListJobs,
    GetJob(JobId),
    StartJob(JobId),
    GetHistory(JobId),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Ok,
    Error(String),
    JobList(Vec<Job>),
    JobDetail(Option<Job>),
    HistoryList(Vec<HistoryEntry>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub job_id: String,
    pub run_at: String, // DateTime string
    pub status: String,
    pub output: Option<String>,
}
