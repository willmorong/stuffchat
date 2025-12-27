use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub listen: String,
    pub database_path: String,
    pub uploads_dir: String,
    pub jwt_secret: Option<String>,
    pub allowed_origins: Vec<String>,
    pub max_upload_size: usize,
    pub presence_timeout_secs: i64,
    pub invite_only: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "example.org:22800".to_string(),
            database_path: "./stuffchat.sqlite3".to_string(),
            uploads_dir: "./uploads".to_string(),
            jwt_secret: None,
            allowed_origins: vec!["example.org".to_string()],
            max_upload_size: 500 * 1024 * 1024,
            presence_timeout_secs: 60,
            invite_only: false,
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

    pub fn from_env_config() -> Self {
        let mut final_cfg = Self::load();

        if final_cfg.jwt_secret.is_none() {
            final_cfg.jwt_secret = Some(uuid::Uuid::new_v4().to_string());
        }
        std::fs::create_dir_all(&final_cfg.uploads_dir).expect("create uploads dir");
        final_cfg
    }

    pub fn jwt_secret_bytes(&self) -> &[u8] {
        self.jwt_secret
            .as_ref()
            .expect("jwt_secret must be set")
            .as_bytes()
    }
}
