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
                    datetime_secondary TEXT(27),
                    message TEXT NOT NULL DEFAULT '',
                    valid INTEGER NOT NULL DEFAULT 1,
                    creation_time TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (datetime, datetime_secondary)
                )"
                ),
                params![],
            )?;
            queues.insert(queue.clone());
        }
        Ok(AppDb {
            conn: Mutex::new(conn),
            queues,
        })
    }
}
