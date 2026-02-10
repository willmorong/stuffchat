mod auth;
mod config;
mod db;
mod errors;
mod models;
mod permissions;
mod routes;
mod shareplay;
mod utils;
mod ws;

use crate::config::Config;
use crate::db::Db;
use crate::routes::{
    auth as auth_routes, channels as channels_routes, emojis as emojis_routes,
    files as files_routes, invites as invites_routes, messages as messages_routes,
    users as users_routes,
};
use actix::Actor;
use actix_cors::Cors;
use actix_web::http::header;
use actix_web::middleware::Logger;
use actix_web::web::Data;
use actix_web::{App, HttpServer, web};
use env_logger::Env;
use ws::server::ChatServer;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Init logger to show info by default, but can be overridden by RUST_LOG
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    let cfg = Config::from_env_config();

    let db = Db::connect_and_migrate(&cfg.database_path)
        .await
        .expect("database init failed");

    let chat_server = ChatServer::new().start();
    log::info!("Starting server at {}", cfg.listen);

    // Background task: Cleanup refresh tokens
    let db_clone = db.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600)); // Every hour
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

    // Clean up temp folder on startup
    let temp_dir = std::path::Path::new("temp");
    if temp_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(temp_dir) {
            log::warn!("Failed to clean temp directory on startup: {}", e);
        } else {
            log::info!("Cleaned temp directory on startup");
        }
    }
    // Recreate empty temp directory
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
                    .map(|s| allowed_origins.iter().any(|o| o == s))
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

        App::new()
            .wrap(Logger::default())
            .wrap(cors)
            .app_data(Data::new(cfg.clone()))
            .app_data(Data::new(db.clone()))
            .app_data(Data::new(chat_server.clone()))
            .service(
                web::scope("/api")
                    .route("/health", web::get().to(routes::health::health_check))
                    .service(
                        web::scope("/auth")
                            .route("/register", web::post().to(auth_routes::register))
                            .route("/login", web::post().to(auth_routes::login))
                            .route("/refresh", web::post().to(auth_routes::refresh))
                            .route("/logout", web::post().to(auth_routes::logout)),
                    )
                    .service(
                        web::scope("/users")
                            .route("", web::get().to(users_routes::list_users))
                            .route("/me", web::get().to(users_routes::me))
                            .route("/me", web::patch().to(users_routes::update_me))
                            .route("/me/password", web::put().to(users_routes::change_password))
                            .route("/me/avatar", web::put().to(users_routes::upload_avatar))
                            .route("/{id}", web::get().to(users_routes::get_user))
                            .route("/{id}/avatar", web::get().to(users_routes::get_user_avatar)),
                    )
                    .service(
                        web::scope("/channels")
                            .route("", web::get().to(channels_routes::list_channels))
                            .route("", web::post().to(channels_routes::create_channel))
                            .route("/unread", web::get().to(channels_routes::get_unread))
                            .route("/{id}", web::patch().to(channels_routes::edit_channel))
                            .route("/{id}", web::delete().to(channels_routes::delete_channel))
                            .route("/{id}/read", web::post().to(channels_routes::mark_read))
                            .route(
                                "/{id}/notified",
                                web::post().to(channels_routes::mark_notified),
                            )
                            .route(
                                "/{id}/ownership",
                                web::get().to(channels_routes::check_ownership),
                            )
                            .route("/{id}/join", web::post().to(channels_routes::join_channel))
                            .route(
                                "/{id}/leave",
                                web::post().to(channels_routes::leave_channel),
                            )
                            .route(
                                "/{id}/members",
                                web::get().to(channels_routes::list_members),
                            )
                            .route(
                                "/{id}/members",
                                web::post().to(channels_routes::modify_members),
                            )
                            .route(
                                "/{id}/info",
                                web::get().to(channels_routes::get_channel_info),
                            )
                            .route(
                                "/{id}/messages",
                                web::get().to(messages_routes::list_messages),
                            )
                            .route(
                                "/{id}/messages",
                                web::post().to(messages_routes::post_message),
                            ),
                    )
                    // Add top-level messages edit/delete endpoints
                    .route(
                        "/messages/{id}",
                        web::patch().to(messages_routes::edit_message),
                    )
                    .route(
                        "/messages/{id}",
                        web::delete().to(messages_routes::delete_message),
                    )
                    // Presence API
                    .service(
                        web::scope("/presence")
                            .route("/heartbeat", web::post().to(routes::presence::heartbeat))
                            .route(
                                "/users",
                                web::get().to(routes::presence::get_users_presence),
                            ),
                    )
                    .service(
                        web::scope("/invites")
                            .route("", web::post().to(invites_routes::create_invite))
                            .route("", web::get().to(invites_routes::list_my_invites)),
                    )
                    .service(
                        web::scope("/files").route("", web::post().to(files_routes::upload_file)),
                    )
                    .service(
                        web::scope("/emojis")
                            .route("", web::get().to(emojis_routes::list_emojis))
                            .route("", web::post().to(emojis_routes::upload_emoji))
                            .route("/{name}", web::delete().to(emojis_routes::delete_emoji)),
                    )
                    .route(
                        "/shareplay/{channel_id}/current",
                        web::get().to(routes::shareplay::get_current_track),
                    )
                    .route(
                        "/shareplay/song/{song_id}",
                        web::get().to(routes::shareplay::get_song_by_id),
                    )
                    .route(
                        "/shareplay/thumbnail/{item_id}",
                        web::get().to(routes::shareplay::get_thumbnail_by_id),
                    ),
            )
            .route("/ws", web::get().to(ws::session::ws_route))
            .service(
                web::resource("/files/{id}/{filename:.*}")
                    .route(web::get().to(files_routes::get_file))
                    .route(web::head().to(files_routes::get_file)),
            )
            .route(
                "/emojis/{name}/image",
                web::get().to(emojis_routes::get_emoji_image),
            )
    })
    .bind(listen_addr)?
    .run()
    .await
}
