use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use dtqueue::{AppDb, QueueItem, utils};
use log::{error, info, warn};
use rusqlite::params;
use std::sync::Arc;

pub async fn put_item(
    State(db): State<Arc<AppDb>>,
    Path(queue): Path<String>,
    body: String,
) -> Response {
    if !db.queues.contains(&queue) {
        warn!("Invalid queue name attempted: {queue}");
        return utils::json_error(
            StatusCode::FORBIDDEN,
            "InvalidQueueName",
            &format!("Invalid queue name attempted: {queue}"),
        );
    }

    // parse item from the body
    let item = match QueueItem::from_json_string(&body) {
        Ok(body) => body,
        Err(e) => {
            warn!("Failed to parse request body: {e}");
            return utils::json_error(
                StatusCode::BAD_REQUEST,
                "BadRequest",
                &format!("Failed to parse request body due to: {e}\nRequest body:\n{body}"),
            );
        }
    };

    let conn = match db.conn.lock() {
        Ok(conn) => conn,
        Err(e) => {
            error!("Failed to lock database: {e}");
            return utils::json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                &format!("Failed to lock database: {e}"),
            );
        }
    };

    let table = utils::sanitize_queue_name(&queue).unwrap();

    // Check if the item exists before upsert
    let mut check_stmt = conn
        .prepare(&format!(
            "SELECT COUNT(*) FROM {table} WHERE datetime = ?1 AND datetime_secondary IS ?2"
        ))
        .expect("invalid SQL statement");
    let exists: i64 = check_stmt
        .query_row(
            params![
                item.datetime.to_rfc3339(),
                item.datetime_secondary.map(|d| d.to_rfc3339())
            ],
            |row| row.get(0),
        )
        .expect("Failed to query item existence");

    // insert or replace the item
    let mut stmt = conn
        .prepare(&format!(
            "INSERT OR REPLACE INTO {table} (datetime, datetime_secondary, message) VALUES (?1, ?2, ?3)"
        ))
        .expect("invalid SQL statement");
    match stmt.execute(params![
        item.datetime.to_rfc3339(),
        item.datetime_secondary.map(|d| d.to_rfc3339()),
        item.message.clone()
    ]) {
        Ok(_) => {
            if exists == 0 {
                info!("insert to queue {queue} successful, the item is {item:?}");
                StatusCode::CREATED.into_response()
            } else {
                info!("replace in queue {queue} successful, the item is {item:?}");
                StatusCode::NO_CONTENT.into_response()
            }
        }
        Err(e) => {
            error!("Failed to insert or replace {item:?} to '{queue}': {e}");
            utils::json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                &format!("Failed to insert or replace item to queue {queue}: {e}"),
            )
        }
    }
}

pub async fn get_item(State(db): State<Arc<AppDb>>, Path(queue): Path<String>) -> Response {
    if !db.queues.contains(&queue) {
        warn!("Invalid queue name attempted: {queue}");
        return utils::json_error(
            StatusCode::BAD_REQUEST,
            "InvalidQueueName",
            &format!("Invalid queue name attempted: {queue}"),
        );
    }

    let conn = match db.conn.lock() {
        Ok(conn) => conn,
        Err(e) => {
            error!("Failed to lock database: {e}");
            return utils::json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                &format!("Failed to lock database: {e}"),
            );
        }
    };

    let table = utils::sanitize_queue_name(&queue).unwrap();

    // retrieve the item, ordered by datetime and datetime_secondary
    let mut stmt = conn
        .prepare(&format!(
            "SELECT datetime, datetime_secondary, message FROM {table} WHERE valid = 1 ORDER BY datetime ASC, datetime_secondary ASC LIMIT 1"
        ))
        .expect("invalid SQL statement");
    let item = stmt
        .query_row(params![], |row| {
            let datetime: String = row.get(0).expect("Failed to get datetime");
            let datetime_secondary: Option<String> =
                row.get(1).expect("Failed to get datetime_secondary");
            let message: String = row.get(2).expect("Failed to get message");
            Ok(QueueItem {
                datetime: chrono::DateTime::parse_from_rfc3339(&datetime)
                    .unwrap()
                    .into(),
                datetime_secondary: datetime_secondary
                    .map(|d| chrono::DateTime::parse_from_rfc3339(&d).unwrap().into()),
                message,
            })
        })
        .ok();

    match item {
        Some(item) => {
            let body = item.to_json_string().unwrap();
            info!("retrieve from queue {queue}, got {item:?}");
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .header("Content-Length", body.len().to_string())
                .body(body.into())
                .unwrap()
        }
        None => {
            info!("retrieve from queue {queue}, the queue is empty");
            StatusCode::NO_CONTENT.into_response()
        }
    }
}

pub async fn delete_item(State(db): State<Arc<AppDb>>, Path(queue): Path<String>) -> Response {
    if !db.queues.contains(&queue) {
        warn!("Invalid queue name attempted: {queue}");
        return utils::json_error(
            StatusCode::BAD_REQUEST,
            "InvalidQueueName",
            &format!("Invalid queue name attempted: {queue}"),
        );
    }

    let conn = match db.conn.lock() {
        Ok(conn) => conn,
        Err(e) => {
            error!("Failed to lock database: {e}");
            return utils::json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                &format!("Failed to lock database: {e}"),
            );
        }
    };

    let table = utils::sanitize_queue_name(&queue).unwrap();

    // Find the first valid item
    let mut stmt = conn
        .prepare(&format!(
            "SELECT datetime, datetime_secondary, message FROM {table} WHERE valid = 1 ORDER BY datetime ASC, datetime_secondary ASC LIMIT 1"
        ))
        .expect("invalid SQL statement");
    let result: Option<(String, Option<String>, String)> = stmt
        .query_row(params![], |row| {
            let datetime = row.get(0).expect("Failed to get datetime");
            let datetime_secondary = row.get(1).expect("Failed to get datetime_secondary");
            let message = row.get(2).expect("Failed to get message");
            Ok((datetime, datetime_secondary, message))
        })
        .ok();

    if let Some((datetime, datetime_secondary, message)) = result {
        let item = QueueItem {
            datetime: chrono::DateTime::parse_from_rfc3339(&datetime)
                .unwrap()
                .into(),
            datetime_secondary: datetime_secondary
                .clone()
                .map(|d| chrono::DateTime::parse_from_rfc3339(&d).unwrap().into()),
            message,
        };
        // Set valid to 0 for this item
        let mut update_stmt = conn
            .prepare(&format!(
                "UPDATE {table} SET valid = 0 WHERE (datetime = ?1) AND (datetime_secondary IS ?2);"
            ))
            .expect("invalid SQL statement");
        let updated = match update_stmt.execute(params![datetime, datetime_secondary]) {
            Ok(rows) => rows,
            Err(e) => {
                error!("Failed to update item in queue {queue}: {e}");
                0
            }
        };
        let body = item.to_json_string().unwrap();
        if updated > 0 {
            info!("pop from queue {queue}, got {item:?}");
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .header("Content-Length", body.len().to_string())
                .body(body.into())
                .unwrap()
        } else {
            info!("pop from queue {queue}, got {item:?}, but failed to mark as invalid");
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .header("Content-Length", body.len().to_string())
                .body(body.into())
                .unwrap()
        }
    } else {
        // No valid item found
        info!("pop from queue {queue}, the queue is empty");
        StatusCode::NO_CONTENT.into_response()
    }
}
