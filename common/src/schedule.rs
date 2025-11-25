use crate::job::ScheduleConfig;
use anyhow::{anyhow, Result};

pub fn parse_schedule(s: &str) -> Result<ScheduleConfig> {
    if s.starts_with("every ") {
        let duration_str = s.trim_start_matches("every ").trim();
        let seconds = parse_duration(duration_str)?;
        Ok(ScheduleConfig::Every(seconds))
    } else {
        Ok(ScheduleConfig::Cron(s.to_string()))
    }
}

fn parse_duration(s: &str) -> Result<u64> {
    let (num, unit) = s.split_at(s.len() - 1);
    let n: u64 = num.parse()?;
    match unit {
        "s" => Ok(n),
        "m" => Ok(n * 60),
        "h" => Ok(n * 3600),
        _ => Err(anyhow!("Unknown unit: {}", unit)),
    }
}
