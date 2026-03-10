use actix::Actor;
use actix_cors::Cors;
use actix_web::http::header;
use actix_web::middleware::Logger;
use actix_web::web::Data;
use actix_web::{App, HttpServer};
use env_logger::Env;
use sqlx::Row;
use stuffchat::app;
use stuffchat::auth;
use stuffchat::bridge::{BridgeRuntime, bridge_secret_path, load_or_create_bridge_secret};
use stuffchat::config::Config;
use stuffchat::db::Db;
use stuffchat::errors;
use stuffchat::permissions;
use stuffchat::push::PushRelayRuntime;
use stuffchat::ws::server::ChatServer;

fn parse_admin_arg() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--admin" {
            return args.next();
        }
    }
    None
}

async fn bootstrap_admin(db: &Db, ident: &str) -> Result<(), errors::ApiError> {
    let role_id = permissions::seed_admin_role(db).await?;

    let user_row = sqlx::query(
        "SELECT id, username, email FROM users WHERE id = ? OR username = ? OR email = ? LIMIT 1",
    )
    .bind(ident)
    .bind(ident)
    .bind(ident)
    .fetch_optional(&db.0)
    .await?;

    let Some(user_row) = user_row else {
        log::warn!("Admin bootstrap: user not found for ident={}", ident);
        return Ok(());
    };

    let user_id: String = user_row.get("id");
    let username: String = user_row.get("username");

    sqlx::query("INSERT OR IGNORE INTO user_roles(user_id, role_id) VALUES (?, ?)")
        .bind(&user_id)
        .bind(&role_id)
        .execute(&db.0)
        .await?;

    log::info!(
        "Admin bootstrap: granted admin role to user_id={} username={} ident={}",
        user_id,
        username,
        ident
    );
    Ok(())
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    let cfg = Config::from_env_config();

    let db = Db::connect_and_migrate(&cfg.database_path)
        .await
        .expect("database init failed");

    if let Err(e) = permissions::seed_default_roles(&db).await {
        log::error!("Role seed failed: {}", e);
    }

    let bridge_runtime = if cfg.bridge_enabled {
        match cfg.bridge_url.as_deref().map(str::trim) {
            Some(url) if !url.is_empty() => {
                let secret = load_or_create_bridge_secret(bridge_secret_path())
                    .expect("bridge secret init failed");
                Some(BridgeRuntime::new(secret, url.to_string(), db.clone()))
            }
            _ => {
                log::warn!("Bridge is enabled but bridge_url is not set; bridge delivery disabled");
                None
            }
        }
    } else {
        None
    };

    let push_runtime = if cfg.push_relay_enabled {
        match PushRelayRuntime::new(&cfg, db.clone()) {
            Ok(runtime) => Some(runtime),
            Err(err) => {
                log::warn!("Push relay is enabled but not configured correctly: {err}");
                None
            }
        }
    } else {
        None
    };

    if let Some(admin_ident) = parse_admin_arg() {
        if let Err(e) = bootstrap_admin(&db, &admin_ident).await {
            log::error!("Admin bootstrap failed: {}", e);
        }
    }

    let chat_server = ChatServer::new(bridge_runtime.clone(), push_runtime.clone()).start();
    log::info!("Starting server at {}", cfg.listen);

    let db_clone = db.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        match auth::cleanup_refresh_tokens(&db_clone).await {
            Ok(count) => {
                if count > 0 {
                    log::info!(
                        "Startup: Cleaned up {} expired/revoked refresh tokens",
                        count
                    );
                }
            }
            Err(e) => {
                log::error!("Startup: Failed to cleanup refresh tokens: {}", e);
            }
        }
        loop {
            interval.tick().await;
            match auth::cleanup_refresh_tokens(&db_clone).await {
                Ok(count) => {
                    if count > 0 {
                        log::info!("Cleaned up {} expired/revoked refresh tokens", count);
                    }
                }
                Err(e) => {
                    log::error!("Failed to cleanup refresh tokens: {}", e);
                }
            }
        }
    });

    let temp_dir = std::path::Path::new("temp");
    if temp_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(temp_dir) {
            log::warn!("Failed to clean temp directory on startup: {}", e);
        } else {
            log::info!("Cleaned temp directory on startup");
        }
    }
    if let Err(e) = std::fs::create_dir_all(temp_dir) {
        log::warn!("Failed to create temp directory: {}", e);
    }

    let listen_addr = cfg.listen.clone();
    HttpServer::new(move || {
        let allowed_origins = cfg.allowed_origins.clone();
        let cors = Cors::default()
            .allowed_origin_fn(move |origin, _req| {
                origin
                    .to_str()
                    .map(|value| allowed_origins.iter().any(|allowed| allowed == value))
                    .unwrap_or(false)
            })
            .allowed_methods(vec!["GET", "POST", "PATCH", "PUT", "DELETE"])
            .allowed_headers(vec![
                header::AUTHORIZATION,
                header::ACCEPT,
                header::CONTENT_TYPE,
            ])
            .supports_credentials()
            .max_age(3600);

        let mut app = App::new()
            .wrap(Logger::default())
            .wrap(cors)
            .app_data(Data::new(cfg.clone()))
            .app_data(Data::new(db.clone()))
            .app_data(Data::new(chat_server.clone()))
            .app_data(Data::new(push_runtime.clone()));

        if let Some(bridge_runtime) = &bridge_runtime {
            app = app.app_data(Data::new(bridge_runtime.clone()));
        }

        app.configure(|service_cfg| app::configure(service_cfg, bridge_runtime.is_some()))
    })
    .bind(listen_addr)?
    .run()
    .await
}
