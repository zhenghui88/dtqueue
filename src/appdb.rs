use crate::AppConfig;
use crate::utils::sanitize_queue_name;
use rusqlite::{Connection, Result, params};
use std::collections::HashSet;
use std::sync::Mutex;

pub struct AppDb {
    pub conn: Mutex<Connection>,
    pub queues: HashSet<String>,
}

impl AppDb {
    pub fn new(config: &AppConfig) -> Result<Self> {
        let conn = Connection::open(&config.database_path)?;
        let mut queues = HashSet::new();

        for queue in &config.queues {
            let table = sanitize_queue_name(queue).expect("Failed to sanitize queue name");
            conn.execute(
                &format!(
                    "CREATE TABLE IF NOT EXISTS {table} (
                    datetime TEXT(27) NOT NULL,
                    datetime_secondary TEXT(27) NOT NULL DEFAULT '',
                    message TEXT NOT NULL DEFAULT '',
                    valid INTEGER NOT NULL DEFAULT 1,
                    last_modified TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (datetime, datetime_secondary)
                )"
                ),
                params![],
            )?;
            queues.insert(queue.clone());
            let sql = format!(
                "CREATE TRIGGER IF NOT EXISTS update_{table}_timestamp
                 AFTER UPDATE ON {table}
                 BEGIN UPDATE {table} SET last_modified = CURRENT_TIMESTAMP WHERE datetime = NEW.datetime AND datetime_secondary = NEW.datetime_secondary; END;",
            );
            conn.execute(&sql, [])?;

            // Add index to improve performance when filtering by valid status
            let index_sql = format!(
                "CREATE INDEX IF NOT EXISTS idx_{table}_valid ON {table} (valid, datetime, datetime_secondary)"
            );
            conn.execute(&index_sql, [])?;
        }
        Ok(AppDb {
            conn: Mutex::new(conn),
            queues,
        })
    }
}
