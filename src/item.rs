use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents an item in the queue with primary and optional secondary datetime,
/// along with the message content.
///
/// This struct is used to serialize and deserialize queue items
///
pub struct QueueItem {
    pub datetime: DateTime<Utc>, // Primary datetime
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datetime_secondary: Option<DateTime<Utc>>, // Secondary datetime, optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>, // Message content
}

impl QueueItem {
    /// Serializes the QueueItem to a JSON string.
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserializes a QueueItem from a JSON string.
    pub fn from_json_string(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}
