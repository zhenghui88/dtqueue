use serde::Deserialize;
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub bind_address: String,
    pub port: u16,
    pub queues: Vec<String>,
    pub log_file: String,
    pub log_level: String,
    pub database_path: String,
    pub max_workers: Option<usize>,
}

impl AppConfig {
    pub fn from_file(path: &str) -> Result<Self, config::ConfigError> {
        let settings = config::Config::builder()
            .add_source(config::File::with_name(path))
            .build()?;
        settings.try_deserialize()
    }
}
