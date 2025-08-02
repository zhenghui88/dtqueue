# dtqueue

**dtqueue** is a lightweight, HTTP-based date and time priority queue service, backed by SQLite3. It allows clients to enqueue, dequeue, and peek at messages with primary and optional secondary datetimes, supporting multiple named queues with configurable access and logging.

---

## Features

- **Multiple Named Queues:** Define queues in the configuration file; each queue is mapped to a separate SQLite table.
- **Date/Time Priority:** Items are ordered by primary and optional secondary datetime fields.
- **HTTP API:** Simple RESTful endpoints for enqueue (PUT), dequeue (DELETE), and peek (GET).
- **Persistent Storage:** All data is stored in a local SQLite3 database.
- **Configurable Logging:** Logs to a file with adjustable log levels.
- **Concurrency:** Configurable worker pool for handling concurrent requests.

---

## Quick Start

### 1. Build

```sh
cargo build --release
```

### 2. Configuration

Create a `config.toml` file in the working directory. Example:

```toml
bind_address = "127.0.0.1"
port = 8080
queues = ["default", "urgent"]
log_file = "dtqueue.log"
log_level = "info"
database_path = "dtqueue.db"
max_workers = 1
```

- `bind_address`: Address to bind the HTTP server.
- `port`: Port to listen on.
- `queues`: List of queue names (alphanumeric, `_`, or `-` only).
- `log_file`: Path to the log file.
- `log_level`: Logging level (`debug`, `info`, `warn`, `error`).
- `database_path`: Path to the SQLite database file.
- `max_workers`: Number of worker threads (optional).

### 3. Run

```sh
./target/release/dtqueue config.toml
```
or

```sh
cargo run --release -- config.toml
```

---

## HTTP API

All endpoints are under `/{queue}` where `{queue}` is the name of a configured queue.

### Enqueue (PUT)

- **URL:** `PUT /{queue}`
- **Body:** JSON representing a `QueueItem`:
  ```json
  {
    "datetime": "2024-06-01T12:00:00Z",
    "datetime_secondary": "2024-06-01T12:30:00Z", // optional
    "message": "your message" // default to empty string if not present
  }
  ```
- **Responses:**
  - `201 Created` on success
  - `409 Conflict` if item has been previously enqueued with the same primary and secondary datetime
  - `400 Bad Request` for invalid input
  - `403 Forbidden` if queue name is invalid

### Peek (GET)

- **URL:** `GET /{queue}`
- **Response:** `200 OK` with JSON of the next item, or `204 No Content` if empty.
- **Errors:** `400 Bad Request` if queue name is invalid

### Dequeue (DELETE)

- **URL:** `DELETE /{queue}`
- **Response:** `200 OK` with JSON of the removed item, or `204 No Content` if empty.
- **Errors:** `400 Bad Request` if queue name is invalid

---

## QueueItem Structure

- `datetime` (RFC3339, required): Primary sort key.
- `datetime_secondary` (RFC3339, optional): Secondary sort key.
- `message` (string, optional): Message content.

Example:

```json
{
  "datetime": "2024-06-01T12:00:00Z",
  "datetime_secondary": "2024-06-01T12:30:00Z", // optional
  "message": "Process this job" // default to empty string if not present
}
```

---

## Error Handling

- All errors are returned as XML for compatibility with some queueing clients.
- Example error response:
  ```xml
  <?xml version="1.0" encoding="UTF-8"?>
  <Error>
      <Code>InvalidQueueName</Code>
      <Message>Invalid queue name attempted: urgent!</Message>
  </Error>
  ```

---

## Notes

- Only queue names with alphanumeric, `_`, or `-` are allowed.
- The server logs all operations to the configured log file.
- Items are unique by their `datetime` and `datetime_secondary` combination.
