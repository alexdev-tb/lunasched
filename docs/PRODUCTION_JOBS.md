# Production Job Creation Guide

This guide covers creating production-ready jobs in lunasched v1.2.0 with all advanced features.

## Quick Reference

### Job Priorities (New in v1.2.0)

```bash
# Critical - Highest priority, executes first
lunasched add --name critical-backup --schedule "at 02:00" \
  --command /backup.sh --priority Critical

# High - Important jobs
lunasched add --name health-check --schedule "every 5m" \
  --command /check.sh --priority High

# Normal - Default priority
lunasched add --name log-rotate --schedule "at 00:00" \
  --command /rotate.sh

# Low - Background maintenance
lunasched add --name cleanup --schedule "every 1h" \
  --command /cleanup.sh --priority Low
```

### Execution Modes (New in v1.2.0)

```bash
# Sequential (Default) - One execution at a time
lunasched add --name backup --schedule "every 1h" \
  --command /backup.sh --execution-mode Sequential

# Parallel - Allow concurrent executions
lunasched add --name health --schedule "every 1m" \
  --command /check.sh --execution-mode Parallel

# Exclusive - Block ALL other jobs while running
lunasched add --name db-vacuum --schedule "on Sun at 03:00" \
  --command /vacuum.sh --execution-mode Exclusive
```

## Complete Examples

### Example 1: Production Backup (Critical)

```yaml
name: production-backup
schedule: "at 02:00"
timezone: "America/New_York"
command: /usr/local/bin/backup.sh
args: ["--full", "--compress"]
enabled: true

# Job characteristics
priority: Critical
execution_mode: Sequential
jitter_seconds: 0  # Run exactly at 02:00

# Retry on failure
retry_policy:
  max_attempts: 3
  backoff_strategy: Exponential
  initial_delay_seconds: 300  # First retry after 5 minutes
  max_delay_seconds: 3600     # Cap at 1 hour

# Resource limits
resource_limits:
  timeout_seconds: 7200   # Kill after 2 hours
  max_memory_mb: 4096     # 4GB limit
  cpu_quota: null         # No CPU limit

# Notifications
notification_config:
  on_failure:
    - type: Email
      to: "ops-team@company.com"
      subject: "CRITICAL: Production Backup Failed"
    - type: Slack
      webhook_url: "https://hooks.slack.com/services/XXX/YYY/ZZZ"
  on_success:
    - type: Slack
      webhook_url: "https://hooks.slack.com/services/XXX/YYY/ZZZ"

# Tags for organization
tags: ["production", "backup", "critical"]
```

**Via CLI:**
```bash
lunasched add --name production-backup \
  --schedule "at 02:00" \
  --timezone "America/New_York" \
  --command /usr/local/bin/backup.sh \
  --args "--full --compress" \
  --max-retries 3 \
  --timeout 7200 \
  --tags "production,backup,critical"
```

### Example 2: High-Frequency Health Check (High Priority)

```yaml
name: api-healthcheck
schedule: "every 5m"
command: /usr/local/bin/check-api.sh
enabled: true

# High priority ensures it runs even during load
priority: High

# Allow parallel executions (stateless check)
execution_mode: Parallel

# Add jitter to avoid thundering herd
jitter_seconds: 30  # Random 0-30s delay

# Quick retry on failure
retry_policy:
  max_attempts: 2
  backoff_strategy: Fixed
  initial_delay_seconds: 30

# Short timeout
resource_limits:
  timeout_seconds: 120

# Alert on persistent failure
notification_config:
  on_failure:
    - type: Discord
      webhook_url: "https://discord.com/api/webhooks/XXX"

tags: ["monitoring", "health"]
```

**Via CLI:**
```bash
lunasched add --name api-healthcheck \
  --schedule "every 5m" \
  --command /usr/local/bin/check-api.sh \
  --max-retries 2 \
  --timeout 120 \
  --jitter 30
```

### Example 3: Database Maintenance (Exclusive)

```yaml
name: db-vacuum
schedule: "on Sun at 03:00"
command: /usr/local/bin/vacuum-db.sh
enabled: true

# Low priority - can wait
priority: Low

# EXCLUSIVE: Don't run any other jobs while this runs
execution_mode: Exclusive

# Long timeout for large databases
resource_limits:
  timeout_seconds: 3600
  max_memory_mb: 2048

# Email DBA team on failure
notification_config:
  on_failure:
    - type: Email
      to: "dba-team@company.com"
      subject: "Database Vacuum Failed"

tags: ["database", "maintenance"]
```

### Example 4: Report with Hooks

```yaml
name: daily-report
schedule: "at 08:00"
timezone: "Europe/London"
command: /usr/local/bin/generate-report.sh
enabled: true

priority: Normal

# Run scripts on success/failure
hooks:
  on_success: "/usr/local/bin/send-report.sh"
  on_failure: "/usr/local/bin/alert-failure.sh"

# Webhook notification with custom headers
notification_config:
  on_success:
    - type: Webhook
      url: "https://your-api.com/report-complete"
      headers:
        Authorization: "Bearer YOUR_TOKEN"
        Content-Type: "application/json"

tags: ["reporting", "daily"]
```

## Schedule Syntax

### Standard Schedules

```bash
# Every X seconds/minutes/hours/days
"every 30s"
"every 5m"
"every 2h"
"every 1d"

# At specific time (daily)
"at 14:30"
"at 02:00"

# On specific days at time
"on Mon,Wed,Fri at 09:00"
"on Sat,Sun at 12:00"

# Nth weekday of month
"on 1st Mon at 10:00"    # First Monday
"on 2nd Fri at 15:00"    # Second Friday
"on 3rd Wed at 08:30"    # Third Wednesday

# Cron expressions (advanced)
"cron:0 */6 * * *"       # Every 6 hours
"cron:0 0 1 * *"         # First of month
```

## Retry Strategies

### Exponential Backoff (Recommended)
```yaml
retry_policy:
  max_attempts: 5
  backoff_strategy: Exponential
  initial_delay_seconds: 60    # 1min, 2min, 4min, 8min, 16min
  max_delay_seconds: 3600      # Cap at 1 hour
```

### Linear Backoff
```yaml
retry_policy:
  max_attempts: 3
  backoff_strategy: Linear
  initial_delay_seconds: 300   # 5min, 10min, 15min
  max_delay_seconds: 1800
```

### Fixed Delay
```yaml
retry_policy:
  max_attempts: 3
  backoff_strategy: Fixed
  initial_delay_seconds: 60    # Always 1 minute
  max_delay_seconds: 60
```

## Notification Types

### Email (SMTP)
Requires environment variables:
```bash
export LUNASCHED_EMAIL_FROM="noreply@company.com"
export LUNASCHED_SMTP_SERVER="smtp.gmail.com"
export LUNASCHED_SMTP_USERNAME="your-email@gmail.com"
export LUNASCHED_SMTP_PASSWORD="your-app-password"
```

```yaml
notification_config:
  on_failure:
    - type: Email
      to: "ops@company.com"
      subject: "Job Failed"
```

### Slack
```yaml
notification_config:
  on_success:
    - type: Slack
      webhook_url: "https://hooks.slack.com/services/XXX/YYY/ZZZ"
```

### Discord
```yaml
notification_config:
  on_failure:
    - type: Discord
      webhook_url: "https://discord.com/api/webhooks/XXX/YYY"
```

### Generic Webhook
```yaml
notification_config:
  on_success:
    - type: Webhook
      url: "https://api.example.com/notify"
      headers:
        Authorization: "Bearer TOKEN"
        X-Custom-Header: "value"
```

## Resource Limits

### Timeout
```yaml
resource_limits:
  timeout_seconds: 1800  # Kill job after 30 minutes
```

### Memory Limit (cgroups required)
```yaml
resource_limits:
  max_memory_mb: 2048  # 2GB limit
```

### CPU Quota (cgroups required)
```yaml
resource_limits:
  cpu_quota: 0.5  # 50% of one core
```

## Best Practices

### 1. Use Appropriate Priorities

- **Critical**: Production backups, disaster recovery
- **High**: Health checks, monitoring, alerts
- **Normal**: Reports, logs, routine tasks
- **Low**: Cleanup, optimization, analytics

### 2. Set Execution Modes Correctly

- **Sequential**: Default for most jobs (prevents overlap)
- **Parallel**: Stateless checks, independent tasks
- **Exclusive**: Database maintenance, system-wide operations

### 3. Configure Retries Wisely

- Transient failures: Use Exponential backoff with 3-5 attempts
- Network calls: Fixed delay with 2-3 attempts
- One-time tasks: No retries (max_attempts: 0)

### 4. Always Set Timeouts

```yaml
resource_limits:
  timeout_seconds: 3600  # Prevent hung jobs
```

### 5. Use Jitter for High-Frequency Jobs

```yaml
jitter_seconds: 60  # Spread load over 1 minute
```

### 6. Tag Your Jobs

```yaml
tags: ["production", "database", "critical"]
```

Then filter:
```bash
lunasched list --tag production
```

### 7. Use Timezone for Global Teams

```yaml
timezone: "America/New_York"  # Schedule in local time
```

### 8. Monitor with Notifications

Set up notifications for critical jobs:
- Email ops team on failure
- Slack for success/failure status
- Webhooks for integration with monitoring systems

## Troubleshooting

### Check Execution History
```bash
lunasched history job-name
```

### View Job Details
```bash
lunasched get job-name
```

### Check Daemon Logs
```bash
tail -f /var/log/lunasched/daemon.log
```

### Look for Execution IDs
Every run gets a unique ID for tracking:
```
[2025-12-01][04:00:00][INFO] Scheduling job: backup (execution_id: 550e8400-...)
```

### List All Jobs
```bash
lunasched list
```

## Configuration File Location

Place configuration at:
- `/etc/lunasched/config.yaml` (recommended)
- `~/.config/lunasched/config.yaml` (user-specific)

Import jobs from config:
```bash
lunasched import-config /etc/lunasched/config.yaml
```

## See Also

- [README.md](../README.md) - Full documentation
- [lunasched-config.yaml](../lunasched-config.yaml) - Example configuration
- [walkthrough.md](../walkthrough.md) - v1.2.0 changes
