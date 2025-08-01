use actix_web::{HttpResponse, http::StatusCode};
use serde::Serialize;

#[derive(Serialize)]
pub struct JsonError {
    pub code: String,
    pub message: String,
}

/// Build a JSON error HttpResponse with status code.
pub fn json_error(status: StatusCode, code: &str, message: &str) -> HttpResponse {
    let error = JsonError {
        code: code.to_string(),
        message: message.to_string(),
    };
    let body = serde_json::to_string(&error).unwrap();
    HttpResponse::build(status)
        .content_type("application/json")
        .insert_header(("Content-Length", body.len().to_string()))
        .body(body)
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
