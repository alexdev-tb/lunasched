use rusqlite::{params, Connection, Result};
const SCHEMA_VERSION: i32 = 3;

pub struct Migrator {
    conn: Connection,
}

impl Migrator {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    pub fn run_migrations(&mut self) -> Result<()> {
        // Create schema_version table if it doesn't exist
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY,
                applied_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        let current_version = self.get_current_version()?;
        log::info!("Current database schema version: {}", current_version);

        if current_version < SCHEMA_VERSION {
            log::info!("Migrating database from version {} to {}", current_version, SCHEMA_VERSION);
            self.migrate_from(current_version)?;
        }

        Ok(())
    }

    fn get_current_version(&self) -> Result<i32> {
        let version: Result<i32> = self.conn.query_row(
            "SELECT MAX(version) FROM schema_version",
            [],
            |row| row.get(0),
        );
        Ok(version.unwrap_or(0))
    }

    fn set_version(&self, version: i32) -> Result<()> {
        self.conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            params![version],
        )?;
        Ok(())
    }

    fn migrate_from(&mut self, from_version: i32) -> Result<()> {
        let tx = self.conn.transaction()?;

        for version in (from_version + 1)..=SCHEMA_VERSION {
            log::info!("Applying migration to version {}", version);
            match version {
                1 => Self::migrate_to_v1_impl(&tx)?,
                2 => Self::migrate_to_v2_impl(&tx)?,
                3 => Self::migrate_to_v3_impl(&tx)?,
                _ => return Err(rusqlite::Error::InvalidQuery),
            }
            
            tx.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![version],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    fn migrate_to_v1_impl(tx: &rusqlite::Transaction) -> Result<()> {
        // Base schema (original)
        tx.execute(
            "CREATE TABLE IF NOT EXISTS jobs (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                schedule_type TEXT NOT NULL,
                schedule_value TEXT NOT NULL,
                command TEXT NOT NULL,
                args TEXT NOT NULL,
                env TEXT NOT NULL,
                enabled BOOLEAN NOT NULL,
                owner TEXT NOT NULL DEFAULT 'root'
            )",
            [],
        )?;

        tx.execute(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY,
                job_id TEXT NOT NULL,
                run_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                status TEXT NOT NULL,
                output TEXT
            )",
            [],
        )?;

        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_history_job_id ON history(job_id)",
            [],
        )?;

        Ok(())
    }

    fn migrate_to_v2_impl(tx: &rusqlite::Transaction) -> Result<()> {
        // Add new Phase 1 columns
        log::info!("Adding Phase 1 enhancement columns...");
        
        // Add columns with default values
        let _ = tx.execute("ALTER TABLE jobs ADD COLUMN retry_policy TEXT DEFAULT '{}'", []);
        let _ = tx.execute("ALTER TABLE jobs ADD COLUMN resource_limits TEXT DEFAULT '{}'", []);
        let _ = tx.execute("ALTER TABLE jobs ADD COLUMN jitter_seconds INTEGER DEFAULT 0", []);
        let _ = tx.execute("ALTER TABLE jobs ADD COLUMN timezone TEXT", []);
        let _ = tx.execute("ALTER TABLE jobs ADD COLUMN tags TEXT DEFAULT '[]'", []);
        let _ = tx.execute("ALTER TABLE jobs ADD COLUMN dependencies TEXT DEFAULT '[]'", []);
        let _ = tx.execute("ALTER TABLE jobs ADD COLUMN hooks TEXT DEFAULT '{}'", []);
        let _ = tx.execute("ALTER TABLE jobs ADD COLUMN max_concurrent INTEGER DEFAULT 0", []);

        // Create retry attempts tracking table
        tx.execute(
            "CREATE TABLE IF NOT EXISTS retry_attempts (
                id INTEGER PRIMARY KEY,
                job_id TEXT NOT NULL,
                attempt_number INTEGER NOT NULL,
                run_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                next_retry_at DATETIME,
                error TEXT,
                FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
            )",
            [],
        )?;

        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_retry_attempts_job_id ON retry_attempts(job_id)",
            [],
        )?;

        // Create job metrics table
        tx.execute(
            "CREATE TABLE IF NOT EXISTS job_metrics (
                job_id TEXT PRIMARY KEY,
                total_runs INTEGER DEFAULT 0,
                successful_runs INTEGER DEFAULT 0,
                failed_runs INTEGER DEFAULT 0,
                avg_duration_ms INTEGER DEFAULT 0,
                last_duration_ms INTEGER DEFAULT 0,
                last_run_at DATETIME,
                FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
            )",
            [],
        )?;

        // Create job dependencies table
        tx.execute(
            "CREATE TABLE IF NOT EXISTS job_dependencies (
                id INTEGER PRIMARY KEY,
                job_id TEXT NOT NULL,
                depends_on_job_id TEXT NOT NULL,
                FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE,
                FOREIGN KEY (depends_on_job_id) REFERENCES jobs(id) ON DELETE CASCADE
            )",
            [],
        )?;

        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_job_dependencies_job_id ON job_dependencies(job_id)",
            [],
        )?;

        log::info!("Phase 1 migration completed successfully");
        Ok(())
    }

    fn migrate_to_v3_impl(tx: &rusqlite::Transaction) -> Result<()> {
        // Add new Phase 2 (v1.2.0) columns
        log::info!("Adding Phase 2 (v1.2.0) enhancement columns...");
        
        let _ = tx.execute("ALTER TABLE jobs ADD COLUMN priority TEXT DEFAULT 'Normal'", []);
        let _ = tx.execute("ALTER TABLE jobs ADD COLUMN execution_mode TEXT DEFAULT 'Sequential'", []);
        let _ = tx.execute("ALTER TABLE jobs ADD COLUMN notification_config TEXT DEFAULT '{}'", []);
        
        // Create execution windows tracking table for duplicate prevention
        tx.execute(
            "CREATE TABLE IF NOT EXISTS execution_windows (
                id INTEGER PRIMARY KEY,
                job_id TEXT NOT NULL,
                execution_id TEXT NOT NULL,
                scheduled_time DATETIME NOT NULL,
                actual_start_time DATETIME NOT NULL,
                pid INTEGER,
                FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
            )",
            [],
        )?;
        
        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_execution_windows_job_id ON execution_windows(job_id)",
            [],
        )?;
        
        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_execution_windows_scheduled_time ON execution_windows(scheduled_time)",
            [],
        )?;
        
        // Create notification log table
        tx.execute(
            "CREATE TABLE IF NOT EXISTS notification_log (
                id INTEGER PRIMARY KEY,
                job_id TEXT NOT NULL,
                execution_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                channel_type TEXT NOT NULL,
                delivered_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                status TEXT NOT NULL,
                error TEXT,
                FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
            )",
            [],
        )?;
        
        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_notification_log_job_id ON notification_log(job_id)",
            [],
        )?;

        log::info!("Phase 2 (v1.2.0) migration completed successfully");
        Ok(())
    }

    pub fn into_connection(self) -> Connection {
        self.conn
    }
}
