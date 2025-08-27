# dtqueue

**dtqueue** is a lightweight, HTTP-based date and time priority queue service. It provides persistent storage using SQLite3 and supports multiple named queues with configurable datetime-based prioritization.

## Features

- **Multiple Named Queues**: Define and manage multiple independent queues with separate SQLite tables
- **DateTime Priority**: Items are ordered by primary datetime with optional secondary datetime for tie-breaking
- **HTTP REST API**: Simple RESTful endpoints for enqueue (PUT), dequeue (DELETE), and peek (GET).
- **Persistent Storage**: All data is stored in SQLite3 with proper transaction handling
- **Configurable Logging**: File-based logging with adjustable log levels
- **Thread-Safe**: Built with concurrency in mind using Arc and Mutex
- **Error Handling**: Detailed error responses in JSON format

## Installation

### Building from Source

```bash
# Clone the repository (if applicable)
# git clone <repository-url>
# cd dtqueue

# Build in release mode
cargo build --release

# The binary will be available at ./target/release/dtqueue
```

## Quick Start

1. **Create a configuration file** (`config.toml`):

```toml
bind_address = "127.0.0.1"
port = 8080
queues = ["default", "urgent", "background"]
log_file = "dtqueue.log"
log_level = "info"
database_path = "dtqueue.db"
max_workers = 4
```

2. **Start the server**:

```bash
# Using the built binary
./target/release/dtqueue config.toml

# Or using cargo
cargo run --release -- config.toml
```

3. **Interact with the API**:

```bash
# Enqueue an item
curl -X PUT http://localhost:8080/default \
  -H "Content-Type: application/json" \
  -d '{
    "datetime": "2024-06-01T12:00:00Z",
    "message": "Process this job"
  }'

# Peek at the next item
curl -X GET http://localhost:8080/default

# Dequeue an item
curl -X DELETE http://localhost:8080/default
```

## Configuration

### Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `bind_address` | String | Required | IP address to bind the server to |
| `port` | u16 | Required | Port number to listen on |
| `queues` | Vec<String> | Required | List of queue names to create |
| `log_file` | String | Required | Path to the log file |
| `log_level` | String | "info" | Log level: debug, info, warn, error |
| `database_path` | String | Required | Path to SQLite database file |
| `max_workers` | Option<usize> | None | Maximum worker threads (default: 1) |

### Queue Naming Rules

Queue names must:
- Contain only alphanumeric characters, underscores (`_`), or dashes (`-`). Underscores and dashes are treated equivalently.
- Not be empty
- Be defined in the configuration file before use

## API Reference

All endpoints are available at `/{queue}` where `{queue}` is one of the configured queue names.

### Enqueue Item (PUT)

Adds an item to the specified queue.

**Endpoint**: `PUT /{queue}`

**Request Body**:
```json
{
  "datetime": "2024-06-01T12:00:00Z",
  "datetime_secondary": "2024-06-01T12:30:00Z",
  "message": "Your message content"
}
```

**Responses**:
- `201 Created`: Item successfully enqueued
- `400 Bad Request`: Invalid input or malformed JSON
- `403 Forbidden`: Invalid queue name
- `500 Internal Server Error`: Database or server error

### Peek Item (GET)

Retrieves the next item from the queue without removing it.

**Endpoint**: `GET /{queue}`

**Responses**:
- `200 OK`: Returns the next item as JSON
- `204 No Content`: Queue is empty
- `400 Bad Request`: Invalid queue name
- `500 Internal Server Error`: Database or server error

**Response Body** (200 OK):
```json
{
  "datetime": "2024-06-01T12:00:00Z",
  "datetime_secondary": "2024-06-01T12:30:00Z",
  "message": "Your message content"
}
```

### Dequeue Item (DELETE)

Removes and returns the next item from the queue.

**Endpoint**: `DELETE /{queue}`

**Responses**:
- `200 OK`: Returns the dequeued item as JSON
- `204 No Content`: Queue is empty
- `400 Bad Request`: Invalid queue name
- `500 Internal Server Error`: Database or server error

**Response Body** (200 OK):
```json
{
  "datetime": "2024-06-01T12:00:00Z",
  "datetime_secondary": "2024-06-01T12:30:00Z",
  "message": "Your message content"
}
```

## Queue Item Structure

### Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `datetime` | RFC3339 DateTime | Yes | Primary sort key. Must be in RFC3339 format |
| `datetime_secondary` | RFC3339 DateTime | No | Secondary sort key for tie-breaking |
| `message` | String | No | Message content (default: empty string) |

### Examples

**Basic item**:
```json
{
  "datetime": "2024-06-01T12:00:00Z",
  "message": "Process this job"
}
```

**Item with secondary datetime**:
```json
{
  "datetime": "2024-06-01T12:00:00Z",
  "datetime_secondary": "2024-06-01T12:30:00Z",
  "message": "Urgent processing required"
}
```

**Minimal item**:
```json
{
  "datetime": "2024-06-01T12:00:00Z"
}
```

## Error Handling

All error responses follow a consistent JSON format:

```json
{
  "code": "ErrorCode",
  "message": "Human-readable error description"
}
```

### Common Error Codes

- `InvalidQueueName`: Attempted to access a non-existent or invalid queue
- `BadRequest`: Malformed JSON or invalid datetime format
- `InternalError`: Server or database error

### Example Error Response

```bash
HTTP/1.1 400 Bad Request
Content-Type: application/json

{
  "code": "InvalidQueueName",
  "message": "Invalid queue name attempted: invalid_queue!"
}
```


## Notes

- Only queue names with alphanumeric, `_`, or `-` are allowed.
- The server logs all operations to the configured log file.
- Items are unique by their `datetime` and `datetime_secondary` combination. If an item with the same combination already exists, a PUT request will replace it.
