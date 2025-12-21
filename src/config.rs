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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_config_from_file() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        let config_content = r#"
           bind_address = "127.0.0.1"
           port = 8000
           queues = ["queue1", "queue2"]
           log_file = "app.log"
           log_level = "info"
           database_path = "db.sqlite"
           max_workers = 4
           "#;

        let mut file = File::create(&config_path).unwrap();
        file.write_all(config_content.as_bytes()).unwrap();

        let config = AppConfig::from_file(config_path.to_str().unwrap()).unwrap();

        assert_eq!(config.bind_address, "127.0.0.1");
        assert_eq!(config.port, 8000);
        assert_eq!(config.queues, vec!["queue1", "queue2"]);
        assert_eq!(config.log_file, "app.log");
        assert_eq!(config.log_level, "info");
        assert_eq!(config.database_path, "db.sqlite");
        assert_eq!(config.max_workers, Some(4));
    }

    #[test]
    fn test_invalid_config() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("invalid_config.toml");

        let config_content = r#"
           bind_address = 12345
           port = "not_a_number"
           "#;

        let mut file = File::create(&config_path).unwrap();
        file.write_all(config_content.as_bytes()).unwrap();

        let result = AppConfig::from_file(config_path.to_str().unwrap());
        assert!(result.is_err());
    }
}
