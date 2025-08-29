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
    // Only allow alphanumeric and underscore (SQLite doesn't support hyphens in table names)
    if queue.is_empty() || !queue.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(format!("queue_{queue}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_error_response() {
        let response = json_error(StatusCode::BAD_REQUEST, "TestError", "Test error message");
        let (status, _) = response.into_parts();
        assert_eq!(status.status, StatusCode::BAD_REQUEST);

        // Test more status codes
        let response = json_error(StatusCode::NOT_FOUND, "NotFound", "Resource not found");
        let (status, _) = response.into_parts();
        assert_eq!(status.status, StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_valid_queue_names() {
        // Test valid queue names
        assert_eq!(
            sanitize_queue_name("valid"),
            Some("queue_valid".to_string())
        );
        assert_eq!(
            sanitize_queue_name("valid_queue"),
            Some("queue_valid_queue".to_string())
        );
        assert_eq!(
            sanitize_queue_name("valid123"),
            Some("queue_valid123".to_string())
        );
        assert_eq!(
            sanitize_queue_name("123valid"),
            Some("queue_123valid".to_string())
        );
        assert_eq!(
            sanitize_queue_name("_valid"),
            Some("queue__valid".to_string())
        );
    }

    #[test]
    fn test_invalid_queue_names() {
        // Test invalid queue names
        assert_eq!(sanitize_queue_name(""), None);
        assert_eq!(sanitize_queue_name("invalid-queue"), None); // Hyphen should be rejected
        assert_eq!(sanitize_queue_name("invalid!queue"), None);
        assert_eq!(sanitize_queue_name("invalid queue"), None);
        assert_eq!(sanitize_queue_name("$invalid"), None);
    }
}
