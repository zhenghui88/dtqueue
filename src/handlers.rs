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

    let datetime_str = item.datetime.to_rfc3339();
    let datetime_secondary_str = item
        .datetime_secondary
        .map(|d| d.to_rfc3339())
        .unwrap_or_default();

    // Now perform the actual INSERT OR REPLACE
    // Get the SQL statement once
    let put_sql = db.put_item_sqls.get(&queue).unwrap();
    let mut stmt = conn.prepare_cached(put_sql).expect("invalid SQL statement");

    match stmt.execute(params![
        datetime_str,
        datetime_secondary_str,
        item.message.clone(),
    ]) {
        Ok(_) => {
            info!("append to queue {queue} successful, the item is {item:?}");
            StatusCode::OK.into_response()
        }
        Err(e) => {
            error!("Failed to append {item:?} to '{queue}': {e}");
            utils::json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                &format!("Failed to append item to queue {queue}: {e}"),
            )
        }
    }
}

pub async fn get_item(State(db): State<Arc<AppDb>>, Path(queue): Path<String>) -> Response {
    if !db.queues.contains(&queue) {
        warn!("Invalid queue name attempted: {queue}");
        return utils::json_error(
            StatusCode::FORBIDDEN,
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

    let sql = db.get_item_sqls.get(&queue).unwrap();

    // Use prepare_cached to reuse statements
    let mut stmt = conn.prepare_cached(sql).expect("invalid SQL statement");

    let item = stmt
        .query_row(params![], |row| {
            let datetime: String = row.get(0).expect("Failed to get datetime");
            let datetime_secondary: String = row.get(1).expect("Failed to get datetime_secondary");
            let message: String = row.get(2).expect("Failed to get message");
            Ok(QueueItem {
                datetime: chrono::DateTime::parse_from_rfc3339(&datetime)
                    .unwrap()
                    .into(),
                datetime_secondary: chrono::DateTime::parse_from_rfc3339(&datetime_secondary)
                    .map(|d| d.into())
                    .ok(),
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
            StatusCode::FORBIDDEN,
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

    let delete_sql = db.delete_item_sqls.get(&queue).unwrap();

    // Use prepare_cached to reuse statements
    let mut stmt = conn
        .prepare_cached(delete_sql)
        .expect("invalid SQL statement");

    let item = stmt
        .query_row(params![], |row| {
            let datetime: String = row.get(0).expect("Failed to get datetime");
            let datetime_secondary: String = row.get(1).expect("Failed to get datetime_secondary");
            let message: String = row.get(2).expect("Failed to get message");
            Ok(QueueItem {
                datetime: chrono::DateTime::parse_from_rfc3339(&datetime)
                    .unwrap()
                    .into(),
                datetime_secondary: chrono::DateTime::parse_from_rfc3339(&datetime_secondary)
                    .map(|d| d.into())
                    .ok(),
                message,
            })
        })
        .ok();

    if let Some(item) = item {
        let body = item.to_json_string().unwrap();
        info!("pop from queue {queue}, got {item:?}");
        Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len().to_string())
            .body(body.into())
            .unwrap()
    } else {
        // No valid item found
        info!("pop from queue {queue}, the queue is empty");
        StatusCode::NO_CONTENT.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use chrono::Utc;
    use tower::ServiceExt;

    fn setup_test_app() -> (Router, Arc<AppDb>) {
        let config = AppConfig {
            bind_address: "127.0.0.1".to_string(),
            port: 8080,
            queues: vec!["queue".to_string()],
            log_file: "test.log".to_string(),
            log_level: "info".to_string(),
            database_path: ":memory:".to_string(),
            max_workers: Some(2),
        };

        let db = Arc::new(AppDb::new(&config).unwrap());

        let app = Router::new()
            .route("/{*queue}", get(get_item).put(put_item).delete(delete_item))
            .with_state(db.clone());

        (app, db)
    }

    #[tokio::test]
    async fn test_put_item_handler() {
        let (app, _) = setup_test_app();

        let now = Utc::now();
        let item = QueueItem {
            datetime: now,
            datetime_secondary: None,
            message: "test message".to_string(),
        };

        let json = item.to_json_string().unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/queue")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", json.len().to_string())
                    .body(Body::from(json))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_put_invalid_queue() {
        let (app, _) = setup_test_app();

        let now = Utc::now();
        let item = QueueItem {
            datetime: now,
            datetime_secondary: None,
            message: "test message".to_string(),
        };

        let json = item.to_json_string().unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/queue/invalid_queue")
                    .header("Content-Type", "application/json")
                    .body(Body::from(json))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
