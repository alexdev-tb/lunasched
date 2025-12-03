# Lunasched v1.2.0

Lunasched is a modern, robust, and production-ready replacement for cron, built with Rust. It offers advanced scheduling capabilities, retry policies, notifications, metrics collection, and a user-friendly CLI.

## ✨ What's New in v1.2.0

### Critical Bug Fixes
- **Duplicate Job Execution Fix**: Implemented minute-level execution window tracking to prevent jobs from running multiple times in the same schedule window
- **Enhanced Calendar Scheduling**: Fixed race conditions in calendar-based schedules with precise execution tracking

### New Features
- **Multi-Channel Notifications**: Email (SMTP), Webhooks, Discord, and Slack integration
- **Prometheus Metrics**: Built-in metrics endpoint with job execution counters, duration percentiles (p50, p95, p99), and success/failure rates
- **Job Priorities**: Support for Low, Normal, High, and Critical priority levels
- **Execution Modes**: Sequential, Parallel, or Exclusive execution control
- **UUID Execution Tracking**: Every job execution gets a unique ID for precise tracking and debugging
- **Enhanced Logging**: Comprehensive execution logging with execution IDs and scheduled vs actual time tracking

## Features

- **Flexible Scheduling**: Support for `every X` syntax, standard Cron expressions, and calendar-based schedules (`at HH:MM [on Mon,Wed]`)
- **Advanced Retry Policies**: Exponential, linear, or fixed backoff strategies with configurable max attempts
- **Resource Limits**: Timeout enforcement, memory limits, and CPU quotas
- **Timezone Support**: Schedule jobs in different timezones
- **Job Dependencies**: Define job execution order with dependencies
- **Hooks**: Execute custom commands on job success or failure
- **Persistence**: Jobs and execution history stored in SQLite database
- **Daemon Architecture**: Background daemon with lightweight CLI client
- **User Isolation**: Secure user switching for job execution
- **Execution History**: Complete audit trail of all job executions

## Installation

### From Source

```bash
cargo build --release
```

Binaries will be available in `target/release/`:
- `lunasched-daemon` - The scheduler daemon
- `lunasched` - The CLI client

### Package Installation

Using the provided `install` script:

```bash
sudo ./install
```

This will:
1. Create the `lunasched` user
2. Install binaries to `/usr/local/bin/`
3. Set up systemd service
4. Create log directories
5. Start the daemon

## Quick Start

### 1. Start the Daemon

```bash
sudo systemctl start lunasched
sudo systemctl enable lunasched
```

Or run manually:

```bash
lunasched-daemon
```

### 2. Add Jobs

**Simple periodic job:**
```bash
lunasched add --name backup --schedule "every 1h" --command /usr/local/bin/backup.sh
```

**Calendar-based job:**
```bash
lunasched add --name daily-report --schedule "at 04:00" --command /usr/local/bin/report.sh
```

**Cron expression:**
```bash
lunasched add --name cronjob --schedule "cron:0 0 * * *" --command /usr/bin/cleanup.sh
```

### 3. Manage Jobs

**List all jobs:**
```bash
lunasched list
```

**View job details:**
```bash
lunasched get backup
```

**View execution history:**
```bash
lunasched history backup
```

**Manually trigger a job:**
```bash
lunasched start backup
```

**Remove a job:**
```bash
lunasched remove backup
```

## Advanced Features

### Notifications

Configure notifications in your job (via YAML/TOML config file or programmatically):

```yaml
jobs:
  - name: backup
    schedule: "every 1h"
    command: /usr/local/bin/backup.sh
    notification_config:
      on_success:
        - type: discord
          webhook_url: https://discord.com/api/webhooks/...
      on_failure:
        - type: email
          to: admin@example.com
          subject: "Backup Failed!"
        - type: slack
          webhook_url: https://hooks.slack.com/services/...
```

**Environment variables for email:**
```bash
export LUNASCHED_EMAIL_FROM="noreply@example.com"
export LUNASCHED_SMTP_SERVER="smtp.gmail.com"
export LUNASCHED_SMTP_USERNAME="your-email@gmail.com"
export LUNASCHED_SMTP_PASSWORD="your-app-password"
```

### Retry Policies

```yaml
jobs:
  - name: api-sync
    schedule: "every 15m"
    command: /usr/local/bin/sync.sh
    retry_policy:
      max_attempts: 3
      backoff_strategy: Exponential
      initial_delay_seconds: 60
      max_delay_seconds: 3600
```

### Resource Limits

```yaml
jobs:
  - name: heavy-task
    schedule: "at 02:00"
    command: /usr/local/bin/process.sh
    resource_limits:
      timeout_seconds: 1800  # 30 minutes
      max_memory_mb: 2048    # 2GB
      cpu_quota: 0.5         # 50% of one core
```

### Priorities & Execution Modes

```yaml
jobs:
  - name: critical-backup
    schedule: "every 1h"
    command: /usr/local/bin/backup.sh
    priority: Critical
    execution_mode: Sequential  # Wait for previous execution to finish
```

### Timezone-Aware Scheduling

```yaml
jobs:
  - name: ny-report
    schedule: "at 09:00"
    timezone: "America/New_York"
    command: /usr/local/bin/report.sh
```

## Metrics & Monitoring

Access Prometheus-compatible metrics at `/metrics` endpoint (future HTTP API):

```bash
curl http://localhost:8080/metrics
```

Metrics include:
- `lunasched_job_executions_total` - Total job executions per job
- `lunasched_job_successes_total` - Successful executions
- `lunasched_job_failures_total` - Failed executions
- `lunasched_job_duration_ms` - Duration percentiles (p50, p95, p99)
- `lunasched_scheduler_ticks_total` - Scheduler health
- `lunasched_queue_depth` - Current job queue size

## Architecture

```
┌─────────────────┐
│   lunasched CLI │
└────────┬────────┘
         │ Unix Socket
         │ (/tmp/lunasched.sock)
         ▼
┌────────────────────────┐
│  lunasched-daemon      │
│                        │
│  ┌─────────────────┐   │
│  │   Scheduler     │   │
│  │  - Execution    │   │
│  │    Tracking     │   │
│  │  - Priority     │   │
│  │    Queue        │   │
│  └─────────────────┘   │
│                        │
│  ┌─────────────────┐   │
│  │   Notifier      │   │
│  │  - Email        │   │
│  │  - Webhook      │   │
│  │  - Discord      │   │
│  │  - Slack        │   │
│  └─────────────────┘   │
│                        │
│  ┌─────────────────┐   │
│  │   Metrics       │   │
│  │  - Prometheus   │   │
│  │  - Percentiles  │   │
│  └─────────────────┘   │
└────────┬───────────────┘
         │
         ▼
    ┌──────────┐
    │ SQLite DB│
    │  - Jobs   │
    │  - History│
    │  - Metrics│
    │  - Windows│
    └──────────┘
```

## Troubleshooting

### Jobs Running Twice

lunasched v1.2.0 includes comprehensive duplicate prevention:
- Minute-level execution window tracking
- UUID-based execution IDs
- Persistent execution tracking in database

Check logs at `/var/log/lunasched/daemon.log` for execution IDs:
```
[2025-12-01][04:00:00][INFO] Scheduling job: k3s-backup (execution_id: 550e8400-e29b-41d4-a716-446655440000)
```

### Enable Debug Logging

```bash
export LUNASCHED_LOG=/var/log/lunasched/daemon.log
export RUST_LOG=debug
lunasched-daemon
```

### Database Location

Default: `lunasched.db` in current directory
Systemd service: `/var/lib/lunasched/lunasched.db`

## Upgrading from v1.1.0

The v1.2.0 release includes automatic database migrations:

1. Stop the daemon: `sudo systemctl stop lunasched`
2. Backup your database: `cp /var/lib/lunasched/lunasched.db /var/lib/lunasched/lunasched.db.backup`
3. Update binaries: `cargo build --release && sudo ./install`
4. Start the daemon: `sudo systemctl start lunasched`

The daemon will automatically migrate your database schema to v3.

## Configuration File Support

lunasched supports YAML and TOML configuration files for advanced job definitions:

```bash
lunasched import-config /etc/lunasched/jobs.yaml
```

See `lunasched-example.toml` for a full example.

## Contributing

Contributions are welcome! Please submit issues and pull requests on GitHub.

## License

MIT License - see LICENSE file for details.

## Version

**v1.2.0** - Production-ready cron replacement with advanced features
