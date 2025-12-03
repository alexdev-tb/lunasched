use common::ResourceLimits;
use std::process::Command;
use sysinfo::{System, ProcessRefreshKind};
use std::time::Duration;

pub struct ResourceManager {
    system: System,
}

impl ResourceManager {
    pub fn new() -> Self {
        Self {
            system: System::new_all(),
        }
    }

    /// Apply resource limits to a command before spawning
    pub fn apply_limits(&self, cmd: &mut Command, limits: &ResourceLimits) {
        // Note: Actual cgroup implementation would require root privileges
        // For now, we'll implement timeout at the execution level
        // Memory and CPU limits would require cgroup setup or ulimit
        
        // Environment variables that some programs respect
        if let Some(mem_mb) = limits.max_memory_mb {
            // This is informational; actual enforcement requires cgroups
            cmd.env("LUNASCHED_MAX_MEMORY_MB", mem_mb.to_string());
        }
        
        if let Some(cpu_quota) = limits.cpu_quota {
            cmd.env("LUNASCHED_CPU_QUOTA", cpu_quota.to_string());
        }
    }

    /// Check if system has enough resources
    pub fn check_resources_available(&mut self, limits: &ResourceLimits) -> bool {
        self.system.refresh_all();
        
        // Check memory availability
        if let Some(required_mb) = limits.max_memory_mb {
            let available_mb = self.system.available_memory() / 1024 / 1024;
            if available_mb < required_mb {
                log::warn!("Insufficient memory: required {}MB, available {}MB", required_mb, available_mb);
                return false;
            }
        }
        
        true
    }

    /// Monitor and enforce timeout for a process
    pub async fn enforce_timeout(
        pid: u32,
        timeout_seconds: u64,
    ) -> Result<(), &'static str> {
        let duration = Duration::from_secs(timeout_seconds);
        
        tokio::time::sleep(duration).await;
        
        // Check if process is still running
        let mut system = System::new();
        system.refresh_processes_specifics(ProcessRefreshKind::everything());
        
        if system.process(sysinfo::Pid::from_u32(pid)).is_some() {
            // Process still running, kill it
            log::warn!("Process {} exceeded timeout of {}s, terminating", pid, timeout_seconds);
            
            #[cfg(unix)]
            {
                use nix::sys::signal::{kill, Signal};
                use nix::unistd::Pid;
                let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
                
                // Give it a moment to clean up
                tokio::time::sleep(Duration::from_secs(2)).await;
                
                // Force kill if still alive
                system.refresh_processes_specifics(ProcessRefreshKind::everything());
                if system.process(sysinfo::Pid::from_u32(pid)).is_some() {
                    let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
                }
            }
            
            return Err("Process timeout exceeded");
        }
        
        Ok(())
    }
}

/// Calculate next retry delay based on backoff strategy
pub fn calculate_backoff_delay(
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

#[cfg(test)]
mod tests {
    use super::*;
    use common::BackoffStrategy;

    #[test]
    fn test_exponential_backoff() {
        let delay = calculate_backoff_delay(0, &BackoffStrategy::Exponential, 60, 3600);
        assert_eq!(delay, 60);
        
        let delay = calculate_backoff_delay(1, &BackoffStrategy::Exponential, 60, 3600);
        assert_eq!(delay, 120);
        
        let delay = calculate_backoff_delay(2, &BackoffStrategy::Exponential, 60, 3600);
        assert_eq!(delay, 240);
        
        // Test max delay cap
        let delay = calculate_backoff_delay(10, &BackoffStrategy::Exponential, 60, 3600);
        assert_eq!(delay, 3600);
    }

    #[test]
    fn test_linear_backoff() {
        let delay = calculate_backoff_delay(0, &BackoffStrategy::Linear, 60, 3600);
        assert_eq!(delay, 60);
        
        let delay = calculate_backoff_delay(1, &BackoffStrategy::Linear, 60, 3600);
        assert_eq!(delay, 120);
        
        let delay = calculate_backoff_delay(2, &BackoffStrategy::Linear, 60, 3600);
        assert_eq!(delay, 180);
    }

    #[test]
    fn test_fixed_backoff() {
        let delay = calculate_backoff_delay(0, &BackoffStrategy::Fixed, 60, 3600);
        assert_eq!(delay, 60);
        
        let delay = calculate_backoff_delay(5, &BackoffStrategy::Fixed, 60, 3600);
        assert_eq!(delay, 60);
    }
}
