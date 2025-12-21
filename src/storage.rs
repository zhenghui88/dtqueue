use crate::AppConfig;
use crate::QueueItem;
use crate::utils::sanitize_queue_name;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::RwLock;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Queue not found: {0}")]
    QueueNotFound(String),
    #[error("Lock error")]
    LockError,
    #[error("Pool error: {0}")]
    PoolError(#[from] r2d2::Error),
}

pub type StorageResult<T> = Result<T, StorageError>;

pub trait Storage: Send + Sync {
    fn put_item(&self, queue: &str, item: QueueItem) -> StorageResult<()>;
    fn get_item(&self, queue: &str) -> StorageResult<Option<QueueItem>>;
    fn delete_item(&self, queue: &str) -> StorageResult<Option<QueueItem>>;
    fn queue_exists(&self, queue: &str) -> bool;
}

struct SqliteConnectionManager {
    path: String,
}

impl r2d2::ManageConnection for SqliteConnectionManager {
    type Connection = Connection;
    type Error = rusqlite::Error;

    fn connect(&self) -> Result<Connection, rusqlite::Error> {
        let conn = Connection::open(&self.path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(conn)
    }

    fn is_valid(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch("")
    }

    fn has_broken(&self, _conn: &mut Connection) -> bool {
        false
    }
}

pub struct SqliteStorage {
    pool: r2d2::Pool<SqliteConnectionManager>,
    queues: HashSet<String>,
    get_item_sqls: HashMap<String, String>,
    put_item_sqls: HashMap<String, String>,
    delete_item_sqls: HashMap<String, String>,
}

impl SqliteStorage {
    pub fn new(config: &AppConfig) -> StorageResult<Self> {
        let manager = SqliteConnectionManager {
            path: config.database_path.clone(),
        };
        let pool = r2d2::Pool::new(manager)?;
        let conn = pool.get().map_err(StorageError::PoolError)?;

        let mut queues = HashSet::new();
        let mut get_item_sqls = HashMap::new();
        let mut put_item_sqls = HashMap::new();
        let mut delete_item_sqls = HashMap::new();

        for queue in &config.queues {
            let table = sanitize_queue_name(queue)
                .ok_or_else(|| StorageError::QueueNotFound(queue.clone()))?;
            conn.execute(
                &format!(
                    "CREATE TABLE IF NOT EXISTS {table} (
                    datetime BIGINT NOT NULL,
                    datetime_secondary BIGINT NOT NULL DEFAULT -9223372036854775808,
                    message TEXT NOT NULL DEFAULT '',
                    valid INT2 NOT NULL DEFAULT 1,
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

            let index_sql = format!(
                "CREATE INDEX IF NOT EXISTS idx_{table}_isvalid ON {table} (valid, datetime, datetime_secondary)"
            );
            conn.execute(&index_sql, [])?;

            get_item_sqls.insert(
                queue.clone(),
                format!(
                    "SELECT datetime, datetime_secondary, message FROM {table} WHERE valid = 1 ORDER BY datetime ASC, datetime_secondary ASC LIMIT 1"
                ),
            );

            put_item_sqls.insert(
                queue.clone(),
                format!(
                    "INSERT OR REPLACE INTO {table} (datetime, datetime_secondary, message)
                    VALUES (?1, ?2, ?3)"
                ),
            );

            delete_item_sqls.insert(
                queue.clone(),
                format!(
                    "UPDATE {table} SET valid = 0 WHERE datetime = (SELECT datetime FROM {table} WHERE valid = 1 ORDER BY datetime ASC, datetime_secondary ASC LIMIT 1) AND datetime_secondary = (SELECT datetime_secondary FROM {table} WHERE valid = 1 ORDER BY datetime ASC, datetime_secondary ASC LIMIT 1) RETURNING datetime, datetime_secondary, message"
                ),
            );
        }

        Ok(SqliteStorage {
            pool,
            queues,
            get_item_sqls,
            put_item_sqls,
            delete_item_sqls,
        })
    }
}

impl Storage for SqliteStorage {
    fn put_item(&self, queue: &str, item: QueueItem) -> StorageResult<()> {
        if !self.queues.contains(queue) {
            return Err(StorageError::QueueNotFound(queue.to_string()));
        }

        let datetime_val = item.datetime.timestamp_millis();
        let datetime_secondary_val = item
            .datetime_secondary
            .map(|d| d.timestamp_millis())
            .unwrap_or(i64::MIN);

        let conn = self.pool.get().map_err(StorageError::PoolError)?;
        let put_sql = self
            .put_item_sqls
            .get(queue)
            .ok_or_else(|| StorageError::QueueNotFound(queue.to_string()))?;

        let mut stmt = conn.prepare_cached(put_sql)?;
        stmt.execute(params![datetime_val, datetime_secondary_val, item.message])?;

        Ok(())
    }

    fn get_item(&self, queue: &str) -> StorageResult<Option<QueueItem>> {
        if !self.queues.contains(queue) {
            return Err(StorageError::QueueNotFound(queue.to_string()));
        }

        let conn = self.pool.get().map_err(StorageError::PoolError)?;
        let sql = self
            .get_item_sqls
            .get(queue)
            .ok_or_else(|| StorageError::QueueNotFound(queue.to_string()))?;
        let mut stmt = conn.prepare_cached(sql)?;

        let item = stmt
            .query_row(params![], |row| {
                let datetime: i64 = row.get(0)?;
                let datetime_secondary: i64 = row.get(1)?;
                let message: String = row.get(2)?;
                Ok(QueueItem {
                    datetime: DateTime::<Utc>::from_timestamp_millis(datetime)
                        .expect("Invalid datetime from DB"),
                    datetime_secondary: if datetime_secondary == i64::MIN {
                        None
                    } else {
                        Some(
                            DateTime::<Utc>::from_timestamp_millis(datetime_secondary)
                                .expect("Invalid datetime_secondary from DB"),
                        )
                    },
                    message,
                })
            })
            .optional()?;

        Ok(item)
    }

    fn delete_item(&self, queue: &str) -> StorageResult<Option<QueueItem>> {
        if !self.queues.contains(queue) {
            return Err(StorageError::QueueNotFound(queue.to_string()));
        }

        let conn = self.pool.get().map_err(StorageError::PoolError)?;
        let delete_sql = self
            .delete_item_sqls
            .get(queue)
            .ok_or_else(|| StorageError::QueueNotFound(queue.to_string()))?;
        let mut stmt = conn.prepare_cached(delete_sql)?;

        let item = stmt
            .query_row(params![], |row| {
                let datetime: i64 = row.get(0)?;
                let datetime_secondary: i64 = row.get(1)?;
                let message: String = row.get(2)?;
                Ok(QueueItem {
                    datetime: DateTime::<Utc>::from_timestamp_millis(datetime)
                        .expect("Invalid datetime from DB"),
                    datetime_secondary: if datetime_secondary == i64::MIN {
                        None
                    } else {
                        Some(
                            DateTime::<Utc>::from_timestamp_millis(datetime_secondary)
                                .expect("Invalid datetime_secondary from DB"),
                        )
                    },
                    message,
                })
            })
            .optional()?;

        Ok(item)
    }

    fn queue_exists(&self, queue: &str) -> bool {
        self.queues.contains(queue)
    }
}

type InMemoryQueue = BTreeMap<(DateTime<Utc>, Option<DateTime<Utc>>), String>;

pub struct InMemoryStorage {
    // Map queue_name -> BTreeMap<(datetime, datetime_secondary), message>
    queues: RwLock<HashMap<String, InMemoryQueue>>,
    allowed_queues: HashSet<String>,
}

impl InMemoryStorage {
    pub fn new(config: &AppConfig) -> Self {
        let mut queues_map = HashMap::new();
        let mut allowed_queues = HashSet::new();

        for queue in &config.queues {
            queues_map.insert(queue.clone(), BTreeMap::new());
            allowed_queues.insert(queue.clone());
        }

        InMemoryStorage {
            queues: RwLock::new(queues_map),
            allowed_queues,
        }
    }
}

impl Storage for InMemoryStorage {
    fn put_item(&self, queue: &str, item: QueueItem) -> StorageResult<()> {
        if !self.allowed_queues.contains(queue) {
            return Err(StorageError::QueueNotFound(queue.to_string()));
        }

        let mut queues = self.queues.write().map_err(|_| StorageError::LockError)?;
        if let Some(queue_map) = queues.get_mut(queue) {
            queue_map.insert((item.datetime, item.datetime_secondary), item.message);
        }
        Ok(())
    }

    fn get_item(&self, queue: &str) -> StorageResult<Option<QueueItem>> {
        if !self.allowed_queues.contains(queue) {
            return Err(StorageError::QueueNotFound(queue.to_string()));
        }

        let queues = self.queues.read().map_err(|_| StorageError::LockError)?;
        if let Some((key, message)) = queues.get(queue).and_then(|q| q.first_key_value()) {
            return Ok(Some(QueueItem {
                datetime: key.0,
                datetime_secondary: key.1,
                message: message.clone(),
            }));
        }
        Ok(None)
    }

    fn delete_item(&self, queue: &str) -> StorageResult<Option<QueueItem>> {
        if !self.allowed_queues.contains(queue) {
            return Err(StorageError::QueueNotFound(queue.to_string()));
        }

        let mut queues = self.queues.write().map_err(|_| StorageError::LockError)?;
        if let Some((key, message)) = queues.get_mut(queue).and_then(|q| q.pop_first()) {
            return Ok(Some(QueueItem {
                datetime: key.0,
                datetime_secondary: key.1,
                message,
            }));
        }
        Ok(None)
    }

    fn queue_exists(&self, queue: &str) -> bool {
        self.allowed_queues.contains(queue)
    }
}
