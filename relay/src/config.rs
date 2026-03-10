use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub listen: String,
    pub database_path: String,
    pub apns_team_id: Option<String>,
    pub apns_key_id: Option<String>,
    pub apns_private_key_path: Option<String>,
    pub apns_topic: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:22900".to_string(),
            database_path: "./relay.sqlite3".to_string(),
            apns_team_id: None,
            apns_key_id: None,
            apns_private_key_path: None,
            apns_topic: None,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let config_path = Path::new("config.toml");
        if config_path.exists() {
            let mut file = std::fs::File::open(config_path).expect("failed to open config.toml");
            let mut contents = String::new();
            file.read_to_string(&mut contents)
                .expect("failed to read config.toml");
            toml::from_str(&contents).expect("failed to parse config.toml")
        } else {
            let default_config = Config::default();
            let toml_string = toml::to_string_pretty(&default_config)
                .expect("failed to serialize default config");
            let mut file =
                std::fs::File::create(config_path).expect("failed to create config.toml");
            file.write_all(toml_string.as_bytes())
                .expect("failed to write config.toml");
            default_config
        }
    }
}
