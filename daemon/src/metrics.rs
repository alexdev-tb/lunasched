use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Metrics collector for Prometheus-compatible output
pub struct MetricsCollector {
    job_executions: Arc<DashMap<String, AtomicU64>>,
    job_successes: Arc<DashMap<String, AtomicU64>>,
    job_failures: Arc<DashMap<String, AtomicU64>>,
    job_durations: Arc<DashMap<String, Vec<u64>>>, // Store last 100 durations for percentiles
    scheduler_ticks: Arc<AtomicU64>,
    queue_depth: Arc<AtomicU64>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            job_executions: Arc::new(DashMap::new()),
            job_successes: Arc::new(DashMap::new()),
            job_failures: Arc::new(DashMap::new()),
            job_durations: Arc::new(DashMap::new()),
            scheduler_ticks: Arc::new(AtomicU64::new(0)),
            queue_depth: Arc::new(AtomicU64::new(0)),
        }
    }
    
    pub fn record_execution(&self, job_id: &str) {
        self.job_executions
            .entry(job_id.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_success(&self, job_id: &str, duration_ms: u64) {
        self.job_successes
            .entry(job_id.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
        
       // Store duration for percentile calculation (keep last 100)
        let mut entry = self.job_durations
            .entry(job_id.to_string())
            .or_insert_with(Vec::new);
        
        entry.push(duration_ms);
        
        // Trim to last 100 entries
        let len = entry.len();
        if len > 100 {
            entry.drain(0..len - 100);
        }
    }
    
    pub fn record_failure(&self, job_id: &str) {
        self.job_failures
            .entry(job_id.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn increment_scheduler_ticks(&self) {
        self.scheduler_ticks.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn set_queue_depth(&self, depth: u64) {
        self.queue_depth.store(depth, Ordering::Relaxed);
    }
    
    /// Generate Prometheus-compatible metrics output
    pub fn export(&self) -> String {
        let mut output = String::new();
        
        // Scheduler metrics
        output.push_str("# HELP lunasched_scheduler_ticks_total Total number of scheduler ticks\n");
        output.push_str("# TYPE lunasched_scheduler_ticks_total counter\n");
        output.push_str(&format!(
            "lunasched_scheduler_ticks_total {}\n\n",
            self.scheduler_ticks.load(Ordering::Relaxed)
        ));
        
        output.push_str("# HELP lunasched_queue_depth Current job queue depth\n");
        output.push_str("# TYPE lunasched_queue_depth gauge\n");
        output.push_str(&format!(
            "lunasched_queue_depth {}\n\n",
            self.queue_depth.load(Ordering::Relaxed)
        ));
        
        // Job execution metrics
        output.push_str("# HELP lunasched_job_executions_total Total number of job executions\n");
        output.push_str("# TYPE lunasched_job_executions_total counter\n");
        for entry in self.job_executions.iter() {
            output.push_str(&format!(
                "lunasched_job_executions_total{{job_id=\"{}\"}} {}\n",
                entry.key(),
                entry.value().load(Ordering::Relaxed)
            ));
        }
        output.push('\n');
        
        output.push_str("# HELP lunasched_job_successes_total Total number of successful job executions\n");
        output.push_str("# TYPE lunasched_job_successes_total counter\n");
        for entry in self.job_successes.iter() {
            output.push_str(&format!(
                "lunasched_job_successes_total{{job_id=\"{}\"}} {}\n",
                entry.key(),
                entry.value().load(Ordering::Relaxed)
            ));
        }
        output.push('\n');
        
        output.push_str("# HELP lunasched_job_failures_total Total number of failed job executions\n");
        output.push_str("# TYPE lunasched_job_failures_total counter\n");
        for entry in self.job_failures.iter() {
            output.push_str(&format!(
                "lunasched_job_failures_total{{job_id=\"{}\"}} {}\n",
                entry.key(),
                entry.value().load(Ordering::Relaxed)
            ));
        }
        output.push('\n');
        
        // Duration percentiles
        output.push_str("# HELP lunasched_job_duration_ms Job execution duration percentiles\n");
        output.push_str("# TYPE lunasched_job_duration_ms gauge\n");
        for entry in self.job_durations.iter() {
            let mut durations = entry.value().clone();
            if !durations.is_empty() {
                durations.sort_unstable();
                let p50 = percentile(&durations, 50.0);
                let p95 = percentile(&durations, 95.0);
                let p99 = percentile(&durations, 99.0);
                
                output.push_str(&format!(
                    "lunasched_job_duration_ms{{job_id=\"{}\",quantile=\"0.5\"}} {}\n",
                    entry.key(), p50
                ));
                output.push_str(&format!(
                    "lunasched_job_duration_ms{{job_id=\"{}\",quantile=\"0.95\"}} {}\n",
                    entry.key(), p95
                ));
                output.push_str(&format!(
                    "lunasched_job_duration_ms{{job_id=\"{}\",quantile=\"0.99\"}} {}\n",
                    entry.key(), p99
                ));
            }
        }
        
        output
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

fn percentile(sorted_data: &[u64], p: f64) -> u64 {
    if sorted_data.is_empty() {
        return 0;
    }
    let index = ((p / 100.0) * (sorted_data.len() as f64 - 1.0)).round() as usize;
    sorted_data[index.min(sorted_data.len() - 1)]
}
