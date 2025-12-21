use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
/// Represents an item in the queue with primary and optional secondary datetime,
/// along with the message content.
///
/// This struct is used to serialize and deserialize queue items
///
pub struct QueueItem {
    pub datetime: DateTime<Utc>, // Primary datetime
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datetime_secondary: Option<DateTime<Utc>>, // Secondary datetime, optional
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub message: String, // Message content
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_queue_item_serialization() {
        let now = Utc::now();
        let item = QueueItem {
            datetime: now,
            datetime_secondary: None,
            message: "test message".to_string(),
        };

        let json = item.to_json_string().unwrap();
        let deserialized = QueueItem::from_json_string(&json).unwrap();

        assert_eq!(item.datetime, deserialized.datetime);
        assert_eq!(item.datetime_secondary, deserialized.datetime_secondary);
        assert_eq!(item.message, deserialized.message);
    }

    #[test]
    fn test_queue_item_with_secondary_datetime() {
        let now = Utc::now();
        let secondary = now + chrono::Duration::hours(1);
        let item = QueueItem {
            datetime: now,
            datetime_secondary: Some(secondary),
            message: "test message".to_string(),
        };

        let json = item.to_json_string().unwrap();
        let deserialized = QueueItem::from_json_string(&json).unwrap();

        assert_eq!(item.datetime, deserialized.datetime);
        assert_eq!(item.datetime_secondary, deserialized.datetime_secondary);
        assert_eq!(item.message, deserialized.message);
    }

    #[test]
    fn test_empty_message_skips_serializing() {
        let now = Utc::now();
        let item = QueueItem {
            datetime: now,
            datetime_secondary: None,
            message: "".to_string(),
        };

        let json = item.to_json_string().unwrap();
        assert!(!json.contains("message"));
    }

    #[test]
    fn test_invalid_json_deserialization() {
        let result = QueueItem::from_json_string("{invalid_json}");
        assert!(result.is_err());
    }
}
