# Lunasched

Lunasched is a modern, robust, and user-friendly replacement for cron, built with Rust. It offers a daemon-client architecture, persistent job storage, execution history, and a flexible scheduling syntax.

## Features

- **User-Friendly Scheduling**: Support for intuitive syntax like `every 5m`, `every 1h`, as well as standard Cron expressions.
- **Persistence**: Jobs and execution history are stored in a local SQLite database (`lunasched.db`), ensuring they survive restarts.
- **History**: View detailed execution logs for your jobs directly from the CLI.
- **Daemon Architecture**: A background daemon manages the schedule and execution, while a lightweight CLI handles user interaction.

## Installation

### Prerequisites

- Rust (latest stable)
- SQLite (bundled)

### Build

```bash
cargo build --release
```

Binaries will be available in `target/release/`:
- `lunasched-daemon`
- `lunasched`

## Usage

### 1. Start the Daemon

```bash
./target/release/lunasched-daemon
```

The daemon will create `lunasched.db` in the current directory and listen on `/tmp/lunasched.sock`.

### 2. Add a Job

**Every 5 seconds:**
```bash
./target/release/lunasched add --name myjob --every 5s --command echo -- "Hello World"
```

**Standard Cron (Every minute):**
```bash
./target/release/lunasched add --name cronjob --cron "* * * * * *" --command echo -- "Cron Job"
```

### 3. List Jobs

```bash
./target/release/lunasched list
```

### 4. View History

```bash
./target/release/lunasched history myjob
```

### 5. Manual Trigger

```bash
./target/release/lunasched start myjob
```

### 6. Remove Job

```bash
./target/release/lunasched remove myjob
```

## Architecture

- **Daemon**: Handles scheduling, process spawning, output capturing, and database interactions.
- **CLI**: Sends requests to the daemon via Unix Domain Sockets.
- **Database**: SQLite is used for storing job configurations and execution logs.

## License

MIT
