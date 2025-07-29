use actix_web::{App, HttpServer, web};
use dtqueue::{AppConfig, AppDb};
use log::info;
use std::env;
use std::fs::OpenOptions;
use std::io::Write;
mod handlers;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let config_path = env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());

    let app_config = AppConfig::from_file(&config_path).expect("Failed to load config");

    // Parse log level from config
    let log_level = match app_config.log_level.parse::<log::LevelFilter>() {
        Ok(level) => level,
        Err(_) => log::LevelFilter::Debug,
    };

    // Setup logging to file
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&app_config.log_file)
        .expect("Failed to open log file");
    let log_file = std::sync::Mutex::new(log_file);
    let logger = env_logger::Builder::new()
        .format(move |buf, record| {
            let mut log_file = log_file.lock().unwrap();
            let log_line = format!(
                "{} [{}] - {}\n",
                chrono::Utc::now().to_rfc3339(),
                record.level(),
                record.args()
            );
            let _ = log_file.write_all(log_line.as_bytes());
            writeln!(
                buf,
                "{} [{}] - {}",
                chrono::Utc::now().to_rfc3339(),
                record.level(),
                record.args()
            )
        })
        .filter_level(log_level)
        .build();
    log::set_boxed_logger(Box::new(logger)).unwrap();
    log::set_max_level(log_level);

    info!(
        "Starting server at {}:{}",
        app_config.bind_address, app_config.port
    );

    let db = AppDb::new(&app_config).expect("Failed to initialize database");
    let db_data = web::Data::new(db);

    let server = HttpServer::new(move || {
        App::new()
            .app_data(db_data.clone())
            .configure(handlers::init_routes)
    })
    .bind((app_config.bind_address.as_str(), app_config.port))?;

    let server = if let Some(max_workers) = app_config.max_workers {
        server.workers(max_workers)
    } else {
        server.workers(1)
    };

    server.run().await
}
