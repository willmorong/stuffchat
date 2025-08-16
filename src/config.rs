use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name="stuffchat")]
pub struct Config {
    #[arg(long, env="STUFFCHAT_LISTEN", default_value="punkhazard.local:22800")]
    pub listen: String,

    #[arg(long, env="STUFFCHAT_DATABASE", default_value="./stuffchat.sqlite3")]
    pub database_path: String,

    #[arg(long, env="STUFFCHAT_UPLOADS_DIR", default_value="./uploads")]
    pub uploads_dir: String,

    #[arg(long, env="STUFFCHAT_JWT_SECRET", hide_env_values=true)]
    pub jwt_secret: Option<String>,

    #[arg(long, env="STUFFCHAT_ALLOW_ORIGIN", default_value="punkhazard.local", value_delimiter=',')]
    pub allowed_origins: Vec<String>,

    #[arg(long, env="STUFFCHAT_MAX_UPLOAD_SIZE", default_value_t=500*1024*1024)]
    pub max_upload_size: usize,

    #[arg(long, env="STUFFCHAT_PRESENCE_TIMEOUT_SECS", default_value_t=60)]
    pub presence_timeout_secs: i64,

}

impl Config {
    pub fn from_args_env() -> Self {
        let mut cfg = Config::parse();
        if cfg.jwt_secret.is_none() {
            // In dev, generate ephemeral secret. In prod, require it.
            #[cfg(debug_assertions)]
            {
                cfg.jwt_secret = Some(uuid::Uuid::new_v4().to_string());
                log::warn!("No JWT secret provided; using ephemeral secret (dev only).");
            }
            #[cfg(not(debug_assertions))]
            {
                panic!("--jwt-secret is required in release");
            }
        }
        std::fs::create_dir_all(&cfg.uploads_dir).expect("create uploads dir");
        cfg
    }

    pub fn jwt_secret_bytes(&self) -> &[u8] {
        self.jwt_secret.as_ref().unwrap().as_bytes()
    }
}