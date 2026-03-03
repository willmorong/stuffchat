#![allow(dead_code)]

use actix::Actor;
use actix_web::test::TestRequest;
use std::path::PathBuf;

use stuffchat::bridge::BridgeRuntime;
use stuffchat::config::Config;
use stuffchat::db::Db;
use stuffchat::ws::server::ChatServer;

pub struct TestContext {
    pub root_dir: PathBuf,
    pub cfg: Config,
    pub db: Db,
    pub bridge_runtime: BridgeRuntime,
    pub bridge_secret: String,
    pub chat_server: actix::Addr<ChatServer>,
}

pub async fn test_context() -> TestContext {
    let root_dir =
        std::env::temp_dir().join(format!("stuffchat-bridge-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root_dir).expect("create temp test dir");

    let database_path = root_dir.join("test.sqlite3");
    let uploads_dir = root_dir.join("uploads");
    let db = Db::connect_and_migrate(database_path.to_str().expect("db path str"))
        .await
        .expect("db init");
    let bridge_secret = "bridge-test-secret".to_string();
    let bridge_runtime = BridgeRuntime::new(bridge_secret.clone());
    let chat_server = ChatServer::new(Some(bridge_runtime.clone())).start();

    TestContext {
        root_dir,
        cfg: Config {
            listen: "127.0.0.1:0".to_string(),
            database_path: database_path.to_string_lossy().into_owned(),
            uploads_dir: uploads_dir.to_string_lossy().into_owned(),
            jwt_secret: Some("jwt-test-secret".to_string()),
            allowed_origins: vec!["http://localhost".to_string()],
            max_upload_size: 1024 * 1024,
            presence_timeout_secs: 60,
            invite_only: false,
            bridge_enabled: true,
        },
        db,
        bridge_runtime,
        bridge_secret,
        chat_server,
    }
}

pub async fn insert_user(db: &Db, id: &str, username: &str) {
    let now = chrono::Utc::now();
    let password_hash = "$argon2id$v=19$m=19456,t=2,p=1$abcdefghijklmnop$abcdefghijklmnop";
    sqlx::query(
        "INSERT INTO users(id, username, email, password_hash, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(username)
    .bind(format!("{username}@example.com"))
    .bind(password_hash)
    .bind(now)
    .bind(now)
    .execute(&db.0)
    .await
    .expect("insert user");
}

pub async fn insert_channel(db: &Db, id: &str, name: &str, created_by: &str, is_voice: bool) {
    let now = chrono::Utc::now();
    sqlx::query(
        "INSERT INTO channels(id, name, is_voice, is_private, created_by, created_at) VALUES (?, ?, ?, 0, ?, ?)",
    )
    .bind(id)
    .bind(name)
    .bind(is_voice)
    .bind(created_by)
    .bind(now)
    .execute(&db.0)
    .await
    .expect("insert channel");

    sqlx::query(
        "INSERT INTO channel_members(channel_id, user_id, can_read, can_write, can_manage) VALUES (?, ?, 1, 1, 1)",
    )
    .bind(id)
    .bind(created_by)
    .execute(&db.0)
    .await
    .expect("insert channel member");
}

pub fn bridge_get(path: &str, secret: Option<&str>) -> TestRequest {
    let mut request = TestRequest::get().uri(path);
    if let Some(secret) = secret {
        request = request.insert_header(("Authorization", format!("Bearer {secret}")));
    }
    request
}

pub fn bridge_post_json(path: &str, secret: Option<&str>, body: serde_json::Value) -> TestRequest {
    let mut request = TestRequest::post().uri(path).set_json(body);
    if let Some(secret) = secret {
        request = request.insert_header(("Authorization", format!("Bearer {secret}")));
    }
    request
}
