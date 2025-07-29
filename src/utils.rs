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
