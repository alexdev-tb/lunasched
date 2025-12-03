use crate::job::{ScheduleConfig, CalendarParams};
use anyhow::{anyhow, Result};

pub fn parse_schedule(s: &str) -> Result<ScheduleConfig> {
    if s.starts_with("every ") {
        let duration_str = s.trim_start_matches("every ").trim();
        let seconds = parse_duration(duration_str)?;
        Ok(ScheduleConfig::Every(seconds))
    } else if s.starts_with("at ") || s.starts_with("on ") {
        parse_calendar(s)
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

fn parse_calendar(s: &str) -> Result<ScheduleConfig> {
    // Examples:
    // "at 14:30"
    // "on Mon,Wed at 09:00"
    // "on 1st Mon at 10:00"

    let (date_part, time_part) = if let Some(idx) = s.find(" at ") {
        let (d, t) = s.split_at(idx);
        (d.trim(), t.trim_start_matches(" at ").trim())
    } else if s.starts_with("at ") {
        ("", s.trim_start_matches("at ").trim())
    } else {
        return Err(anyhow!("Missing 'at' time specification"));
    };

    // Parse time
    let time_parts: Vec<&str> = time_part.split(':').collect();
    let (h, m, s) = match time_parts.len() {
        2 => (time_parts[0].parse()?, time_parts[1].parse()?, 0),
        3 => (time_parts[0].parse()?, time_parts[1].parse()?, time_parts[2].parse()?),
        _ => return Err(anyhow!("Invalid time format. Use HH:MM or HH:MM:SS")),
    };

    let mut days_of_week = None;
    let mut nth_weekday = None;

    if date_part.starts_with("on ") {
        let specs = date_part.trim_start_matches("on ").trim();
        
        // Check for "1st Mon", "2nd Fri", etc.
        if let Some(captures) = parse_nth_weekday(specs) {
            nth_weekday = Some(captures);
        } else {
            // Assume comma separated days: Mon,Wed
            let mut days = Vec::new();
            for day_str in specs.split(',') {
                let day = parse_weekday(day_str.trim())?;
                days.push(day);
            }
            days_of_week = Some(days);
        }
    }

    Ok(ScheduleConfig::Calendar(CalendarParams {
        days_of_week,
        nth_weekday,
        time: (h, m, s),
    }))
}

fn parse_weekday(s: &str) -> Result<u32> {
    match s.to_lowercase().as_str() {
        "mon" | "monday" => Ok(1),
        "tue" | "tuesday" => Ok(2),
        "wed" | "wednesday" => Ok(3),
        "thu" | "thursday" => Ok(4),
        "fri" | "friday" => Ok(5),
        "sat" | "saturday" => Ok(6),
        "sun" | "sunday" => Ok(7),
        _ => Err(anyhow!("Invalid weekday: {}", s)),
    }
}

fn parse_nth_weekday(s: &str) -> Option<(u32, u32)> {
    // e.g. "1st Mon"
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 2 {
        return None;
    }

    let n_str = parts[0].to_lowercase();
    let day_str = parts[1];

    let n = if n_str.starts_with("1st") { 1 }
    else if n_str.starts_with("2nd") { 2 }
    else if n_str.starts_with("3rd") { 3 }
    else if n_str.starts_with("4th") { 4 }
    else { return None; };

    if let Ok(day) = parse_weekday(day_str) {
        Some((n, day))
    } else {
        None
    }
}
