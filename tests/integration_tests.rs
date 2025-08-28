use chrono::{Duration, Utc};
use dtqueue::QueueItem;
use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Instant;
use std::{thread, time::Duration as StdDuration};
use uuid::Uuid;

// Keep track of allocated ports to avoid conflicts
static NEXT_PORT: AtomicU16 = AtomicU16::new(9010);

// Test server structure to manage test-specific server instances
struct TestServer {
    port: u16,
    queue_name: String,
    server_process: Child,
    config_path: PathBuf,
    db_path: PathBuf,
    log_path: PathBuf,
}

impl TestServer {
    // Create a new test server with unique queue, config, and database
    fn new(test_name: &str) -> Self {
        // Generate unique identifiers for this test
        let mut buffer = Uuid::encode_buffer();
        let test_id = Uuid::new_v4().simple().encode_lower(&mut buffer);
        let queue_name = format!("test_{}", test_id.replace('-', "_"));

        // Find an available port
        let port = find_available_port();

        // Create directories if they don't exist
        let test_dir = PathBuf::from("tests/tmp");
        fs::create_dir_all(&test_dir).expect("Failed to create test directory");

        // Create paths for test-specific files
        let config_path = test_dir.join(format!("config_{}.toml", test_id));
        let db_path = test_dir.join(format!("queue_{}.sqlite", test_id));
        let log_path = test_dir.join(format!("server_{}.log", test_id));

        // Create test configuration
        create_test_config(&config_path, port, &queue_name, &db_path, &log_path);

        // Start server process
        let server_process = start_test_server(&config_path, port, &queue_name);

        println!(
            "Started test server for '{}' on port {} with queue '{}'",
            test_name, port, queue_name
        );

        TestServer {
            port,
            queue_name,
            server_process,
            config_path,
            db_path,
            log_path,
        }
    }

    // Make a request to this specific test server
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(u16, String), reqwest::Error> {
        // Append queue name if path is empty or just "/"
        let actual_path = if path.is_empty() || path == "/" {
            format!("/{}", self.queue_name)
        } else if path.starts_with("/invalid") || path.starts_with("/nonexistent") {
            path.to_string()
        } else {
            format!("/{}", self.queue_name)
        };

        make_request(method, &actual_path, body, self.port)
    }

    // Clean up all resources even if test fails
    fn cleanup(&mut self) {
        println!("Cleaning up test server resources");

        // Terminate the server process
        match self.server_process.kill() {
            Ok(_) => println!("Server process terminated successfully"),
            Err(e) => println!("Failed to kill server process: {}", e),
        }

        // Wait for the process to fully exit
        match self.server_process.wait() {
            Ok(_) => {}
            Err(e) => println!("Error waiting for server process to exit: {}", e),
        }

        // Remove test files
        let files_to_remove = [&self.config_path, &self.db_path, &self.log_path];
        for file in files_to_remove.iter() {
            if file.exists() {
                match fs::remove_file(file) {
                    Ok(_) => {}
                    Err(e) => println!("Failed to remove file {:?}: {}", file, e),
                }
            }
        }
    }
}

impl Drop for TestServer {
    // Ensure cleanup happens even if test panics
    fn drop(&mut self) {
        self.cleanup();
    }
}

// Find an available port for the test server
fn find_available_port() -> u16 {
    loop {
        let port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
        if port > 65000 {
            // Wrap around if we're getting too high
            NEXT_PORT.store(9010, Ordering::SeqCst);
            continue;
        }

        // Try to bind to the port to check availability
        match TcpListener::bind(format!("127.0.0.1:{}", port)) {
            Ok(_) => return port, // Port is available
            Err(_) => continue,   // Try another port
        }
    }
}

// Create a test configuration file
fn create_test_config(
    config_path: &Path,
    port: u16,
    queue_name: &str,
    db_path: &Path,
    log_path: &Path,
) {
    let config_content = format!(
        r#"bind_address = "127.0.0.1"
port = {}
queues = ["{}"]
log_file = "{}"
log_level = "debug"
database_path = "{}"
max_workers = 1
"#,
        port,
        queue_name,
        log_path.to_string_lossy(),
        db_path.to_string_lossy()
    );

    let mut file = File::create(config_path).expect("Failed to create config file");
    file.write_all(config_content.as_bytes())
        .expect("Failed to write config file");
}

// Start a test server with the given configuration
fn start_test_server(config_path: &Path, port: u16, queue_name: &str) -> Child {
    // Start the server process
    let mut child = Command::new("cargo")
        .arg("run")
        .arg("--release")
        .arg("--")
        .arg(config_path)
        .spawn()
        .expect("Failed to start test server process");

    // Wait for the server to be ready
    let client = reqwest::blocking::Client::new();
    let start_time = Instant::now();
    let timeout = StdDuration::from_secs(5);

    while start_time.elapsed() < timeout {
        thread::sleep(StdDuration::from_millis(100));

        // Try to connect to verify the server is up
        match client
            .get(format!("http://127.0.0.1:{}/{}", port, queue_name))
            .send()
        {
            Ok(_) => return child, // Server is ready
            Err(_) => continue,    // Keep waiting
        }
    }

    // Kill the child process if we're timing out
    match child.kill() {
        Ok(_) => println!("Killed hanging test server process"),
        Err(e) => println!("Failed to kill test server process: {}", e),
    }

    // Wait for the process to fully exit
    let _ = child.wait();

    panic!("Test server failed to start within timeout");
}

fn make_request(
    method: &str,
    path: &str,
    body: Option<&str>,
    port: u16,
) -> Result<(u16, String), reqwest::Error> {
    const MAX_RETRIES: u32 = 3;
    const RETRY_DELAY_MS: u64 = 500;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let url = format!("http://127.0.0.1:{}{}", port, path);

    let request = match method {
        "GET" => client.get(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        _ => panic!("Unsupported method"),
    };

    let request = if let Some(body) = body {
        request.body(body.to_string())
    } else {
        request
    };

    // Implement retry logic
    let mut last_error = None;
    for attempt in 0..MAX_RETRIES {
        match request.try_clone().unwrap().send() {
            Ok(response) => {
                let status = response.status().as_u16();
                let body = response.text()?;
                return Ok((status, body));
            }
            Err(e) => {
                last_error = Some(e);
                if attempt < MAX_RETRIES - 1 {
                    eprintln!(
                        "Request failed (attempt {}), retrying in {} ms: {:?}",
                        attempt + 1,
                        RETRY_DELAY_MS,
                        last_error
                    );
                    thread::sleep(std::time::Duration::from_millis(RETRY_DELAY_MS));
                }
            }
        }
    }

    // If we get here, all retries failed
    Err(last_error.unwrap())
}

fn create_queue_item(offset_seconds: i64, message: &str) -> String {
    let dt = Utc::now() + Duration::seconds(offset_seconds);
    let item = QueueItem {
        datetime: dt,
        datetime_secondary: None,
        message: message.to_string(),
    };
    serde_json::to_string(&item).unwrap()
}

// Basic queue operations tests
#[test]
fn test_put_get_delete_operations() {
    let server = TestServer::new("put_get_delete_operations");

    // Test PUT operation
    let item = create_queue_item(0, "test message");
    let (status, _) = server.request("PUT", "/", Some(&item)).unwrap();
    assert_eq!(status, 200, "PUT should return 200 OK");

    // Test GET operation
    let (status, body) = server.request("GET", "/", None).unwrap();
    assert_eq!(status, 200, "GET should return 200 OK");
    let retrieved: QueueItem = serde_json::from_str(&body).unwrap();
    assert_eq!(retrieved.message, "test message");

    // Test DELETE operation
    let (status, body) = server.request("DELETE", "/", None).unwrap();
    assert_eq!(status, 200, "DELETE should return 200 OK");
    let deleted: QueueItem = serde_json::from_str(&body).unwrap();
    assert_eq!(deleted.message, "test message");

    // Queue should be empty now
    let (status, _) = server.request("GET", "/", None).unwrap();
    assert_eq!(
        status, 204,
        "GET on empty queue should return 204 No Content"
    );
}

#[test]
fn test_priority_ordering() {
    let server = TestServer::new("priority_ordering");

    // Add items in reverse chronological order
    let item1 = create_queue_item(10, "message 1"); // Nearest in the future
    let item2 = create_queue_item(20, "message 2");
    let item3 = create_queue_item(30, "message 3"); // Furthest in the future

    server.request("PUT", "/", Some(&item3)).unwrap();
    server.request("PUT", "/", Some(&item2)).unwrap();
    server.request("PUT", "/", Some(&item1)).unwrap();

    // Items should be retrieved in chronological order
    let (_, body1) = server.request("GET", "/", None).unwrap();
    let retrieved1: QueueItem = serde_json::from_str(&body1).unwrap();
    assert_eq!(
        retrieved1.message, "message 1",
        "First item should be message 1"
    );

    server.request("DELETE", "/", None).unwrap(); // Delete the first item

    let (_, body2) = server.request("GET", "/", None).unwrap();
    let retrieved2: QueueItem = serde_json::from_str(&body2).unwrap();
    assert_eq!(
        retrieved2.message, "message 2",
        "Second item should be message 2"
    );

    server.request("DELETE", "/", None).unwrap(); // Delete the second item

    let (_, body3) = server.request("GET", "/", None).unwrap();
    let retrieved3: QueueItem = serde_json::from_str(&body3).unwrap();
    assert_eq!(
        retrieved3.message, "message 3",
        "Third item should be message 3"
    );
}

#[test]
fn test_idempotent_puts() {
    let server = TestServer::new("idempotent_puts");

    // Create an item with a specific timestamp to avoid conflicts
    let now = Utc::now();
    let item = QueueItem {
        datetime: now,
        datetime_secondary: None,
        message: "original message".to_string(),
    };
    let item_json = serde_json::to_string(&item).unwrap();

    // First PUT - should create the item
    let (status, _) = server.request("PUT", "/", Some(&item_json)).unwrap();
    assert_eq!(status, 200, "First PUT should return 200 OK");

    // Update the same item (same datetime key)
    let updated_item = QueueItem {
        datetime: now,
        datetime_secondary: None,
        message: "updated message".to_string(),
    };
    let updated_json = serde_json::to_string(&updated_item).unwrap();

    // Second PUT - should update the item
    let (status, _) = server.request("PUT", "/", Some(&updated_json)).unwrap();
    assert_eq!(status, 200, "Second PUT should return 200 OK");

    // Check that the item was updated
    let (_, body) = server.request("GET", "/", None).unwrap();
    let retrieved: QueueItem = serde_json::from_str(&body).unwrap();
    assert_eq!(
        retrieved.message, "updated message",
        "Message should be updated"
    );
}

#[test]
fn test_queue_throughput() {
    let server = TestServer::new("queue_throughput");

    // Number of items to test with
    let item_count = 1000;

    // Measure PUT throughput
    let start = Instant::now();
    for i in 0..item_count {
        let item = create_queue_item(i as i64, &format!("message {}", i));
        server.request("PUT", "/", Some(&item)).unwrap();
    }
    let put_duration = start.elapsed();
    let put_throughput = item_count as f64 / put_duration.as_secs_f64();
    println!("PUT throughput: {:.2} items/second", put_throughput);

    // Measure GET throughput
    let start = Instant::now();
    for _ in 0..item_count {
        server.request("GET", "/", None).unwrap();
    }
    let get_duration = start.elapsed();
    let get_throughput = item_count as f64 / get_duration.as_secs_f64();
    println!("GET throughput: {:.2} items/second", get_throughput);

    // Measure DELETE throughput
    let start = Instant::now();
    for _ in 0..item_count {
        server.request("DELETE", "/", None).unwrap();
    }
    let delete_duration = start.elapsed();
    let delete_throughput = item_count as f64 / delete_duration.as_secs_f64();
    println!("DELETE throughput: {:.2} items/second", delete_throughput);

    // Print summary
    println!("\nThroughput Summary:");
    println!("PUT: {:.2} items/second", put_throughput);
    println!("GET: {:.2} items/second", get_throughput);
    println!("DELETE: {:.2} items/second", delete_throughput);
}

#[test]
fn test_secondary_datetime_ordering() {
    let server = TestServer::new("secondary_datetime_ordering");

    // Create base time
    let now = Utc::now();

    // Create items with the same primary datetime but different secondary datetimes
    let item1 = QueueItem {
        datetime: now,
        datetime_secondary: Some(now + Duration::seconds(10)),
        message: "secondary 1".to_string(),
    };

    let item2 = QueueItem {
        datetime: now,
        datetime_secondary: Some(now + Duration::seconds(5)),
        message: "secondary 2".to_string(),
    };

    let item3 = QueueItem {
        datetime: now,
        datetime_secondary: None, // None should come first in ordering
        message: "secondary 3".to_string(),
    };

    // Add items in reverse order
    server
        .request("PUT", "/", Some(&serde_json::to_string(&item1).unwrap()))
        .unwrap();
    server
        .request("PUT", "/", Some(&serde_json::to_string(&item2).unwrap()))
        .unwrap();
    server
        .request("PUT", "/", Some(&serde_json::to_string(&item3).unwrap()))
        .unwrap();

    // Items should be retrieved in order of secondary datetime
    // None values should come first, then ascending order

    let (_, body1) = server.request("GET", "/", None).unwrap();
    let retrieved1: QueueItem = serde_json::from_str(&body1).unwrap();
    assert_eq!(
        retrieved1.message, "secondary 3",
        "Item with no secondary datetime should be first"
    );
    server.request("DELETE", "/", None).unwrap();

    let (_, body2) = server.request("GET", "/", None).unwrap();
    let retrieved2: QueueItem = serde_json::from_str(&body2).unwrap();
    assert_eq!(
        retrieved2.message, "secondary 2",
        "Item with earlier secondary datetime should be second"
    );
    server.request("DELETE", "/", None).unwrap();

    let (_, body3) = server.request("GET", "/", None).unwrap();
    let retrieved3: QueueItem = serde_json::from_str(&body3).unwrap();
    assert_eq!(
        retrieved3.message, "secondary 1",
        "Item with later secondary datetime should be third"
    );
}

// Note: The clean_test_queue function is no longer needed since each test has its own isolated server and queue

#[test]
fn test_invalid_queue_name() {
    let server = TestServer::new("invalid_queue_name");

    // First ensure server is up by hitting the valid endpoint
    match server.request("GET", "/", None) {
        Ok(_) => {
            // Server is up, now try an invalid queue name
            match server.request("GET", "/nonexistent_queue", None) {
                Ok((status, body)) => {
                    assert_eq!(
                        status, 403,
                        "Invalid queue name should return 403 Forbidden"
                    );

                    let error: Value = match serde_json::from_str(&body) {
                        Ok(v) => v,
                        Err(e) => {
                            panic!(
                                "Failed to parse error response as JSON: {}\nBody: {}",
                                e, body
                            );
                        }
                    };

                    assert_eq!(
                        error["code"], "InvalidQueueName",
                        "Error code should be InvalidQueueName"
                    );
                }
                Err(e) => {
                    panic!("Failed to make request to invalid queue: {}", e);
                }
            }
        }
        Err(e) => {
            panic!(
                "Server does not appear to be running, health check failed: {}",
                e
            );
        }
    }
}
