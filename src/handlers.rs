use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use dtqueue::{QueueItem, Storage, utils};
use log::{error, info, warn};
use std::sync::Arc;

pub async fn put_item(
    State(storage): State<Arc<dyn Storage>>,
    Path(queue): Path<String>,
    body: String,
) -> Response {
    if !storage.queue_exists(&queue) {
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

    match storage.put_item(&queue, item.clone()) {
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

pub async fn get_item(
    State(storage): State<Arc<dyn Storage>>,
    Path(queue): Path<String>,
) -> Response {
    if !storage.queue_exists(&queue) {
        warn!("Invalid queue name attempted: {queue}");
        return utils::json_error(
            StatusCode::FORBIDDEN,
            "InvalidQueueName",
            &format!("Invalid queue name attempted: {queue}"),
        );
    }

    match storage.get_item(&queue) {
        Ok(Some(item)) => {
            let body = item.to_json_string().unwrap();
            info!("retrieve from queue {queue}, got {item:?}");
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .header("Content-Length", body.len().to_string())
                .body(body.into())
                .unwrap()
        }
        Ok(None) => {
            info!("retrieve from queue {queue}, the queue is empty");
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            error!("Failed to get item from '{queue}': {e}");
            utils::json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                &format!("Failed to get item from queue {queue}: {e}"),
            )
        }
    }
}

pub async fn delete_item(
    State(storage): State<Arc<dyn Storage>>,
    Path(queue): Path<String>,
) -> Response {
    if !storage.queue_exists(&queue) {
        warn!("Invalid queue name attempted: {queue}");
        return utils::json_error(
            StatusCode::FORBIDDEN,
            "InvalidQueueName",
            &format!("Invalid queue name attempted: {queue}"),
        );
    }

    match storage.delete_item(&queue) {
        Ok(Some(item)) => {
            let body = item.to_json_string().unwrap();
            info!("pop from queue {queue}, got {item:?}");
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .header("Content-Length", body.len().to_string())
                .body(body.into())
                .unwrap()
        }
        Ok(None) => {
            info!("pop from queue {queue}, the queue is empty");
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            error!("Failed to delete item from '{queue}': {e}");
            utils::json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                &format!("Failed to delete item from queue {queue}: {e}"),
            )
        }
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

    fn setup_test_app() -> (Router, Arc<dyn Storage>) {
        let config = AppConfig {
            bind_address: "127.0.0.1".to_string(),
            port: 8080,
            queues: vec!["queue".to_string()],
            log_file: "test.log".to_string(),
            log_level: "info".to_string(),
            database_path: ":memory:".to_string(),
            max_workers: Some(2),
        };

        let storage = Arc::new(dtqueue::InMemoryStorage::new(&config));

        let app = Router::new()
            .route("/{*queue}", get(get_item).put(put_item).delete(delete_item))
            .with_state(storage.clone() as Arc<dyn Storage>);

        (app, storage)
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
