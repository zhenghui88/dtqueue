use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Serialize)]
pub struct JsonError {
    pub code: String,
    pub message: String,
}

/// Build a JSON error Response with status code.
pub fn json_error(status: StatusCode, code: &str, message: &str) -> Response {
    let error = JsonError {
        code: code.to_string(),
        message: message.to_string(),
    };
    (status, Json(error)).into_response()
}

/// Sanitize queue name to be a valid SQLite table name.
/// Returns Some(table_name) if valid, None if invalid.
pub fn sanitize_queue_name(queue: &str) -> Option<String> {
    // Only allow alphanumeric, underscore, and dash
    if queue.is_empty()
        || !queue
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some(format!("queue_{queue}"))
}
