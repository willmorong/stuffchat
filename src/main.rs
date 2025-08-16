mod config;
mod errors;
mod db;
mod auth;
mod models;
mod routes;
mod ws;
mod permissions;
mod utils;

use actix_cors::Cors;
use actix_web::{App, HttpServer, web};
use crate::config::Config;
use crate::db::Db;
use crate::routes::{auth as auth_routes, users as users_routes, channels as channels_routes, messages as messages_routes, files as files_routes};
use actix_web::middleware::Logger;
use actix_web::http::header;
use actix_web::web::Data;
use env_logger::Env;
use ws::server::ChatServer;
use actix::Actor;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Init logger to show info by default, but can be overridden by RUST_LOG
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    let cfg = Config::from_args_env();

    let db = Db::connect_and_migrate(&cfg.database_path).await
        .expect("database init failed");

    let chat_server = ChatServer::new().start();
    log::info!("Starting server at {}", cfg.listen);

    let listen_addr = cfg.listen.clone();
    HttpServer::new(move || {
        let cors = Cors::permissive() // change later
            .allowed_methods(vec!["GET", "POST", "PATCH", "PUT", "DELETE"])
            .allowed_headers(vec![header::AUTHORIZATION, header::ACCEPT, header::CONTENT_TYPE])
            .supports_credentials()
            .max_age(3600);
        // for origin in &cfg.allowed_origins {
        //     cors = cors.allowed_origin(origin);
        // }

        App::new()
            .wrap(Logger::default())
            .wrap(cors)
            .app_data(Data::new(cfg.clone()))
            .app_data(Data::new(db.clone()))
            .app_data(Data::new(chat_server.clone()))
            .service(
                web::scope("/api")
                    .service(web::scope("/auth")
                        .route("/register", web::post().to(auth_routes::register))
                        .route("/login", web::post().to(auth_routes::login))
                        .route("/refresh", web::post().to(auth_routes::refresh))
                        .route("/logout", web::post().to(auth_routes::logout))
                    )
                    .service(web::scope("/users")
                        .route("/me", web::get().to(users_routes::me))
                        .route("/me", web::patch().to(users_routes::update_me))
                        .route("/me/password", web::put().to(users_routes::change_password))
                        .route("/me/avatar", web::put().to(users_routes::upload_avatar))
                    )
                    .service(web::scope("/channels")
                        .route("", web::get().to(channels_routes::list_channels))
                        .route("", web::post().to(channels_routes::create_channel))
                        .route("/{id}", web::delete().to(channels_routes::delete_channel))
                        .route("/{id}/join", web::post().to(channels_routes::join_channel))
                        .route("/{id}/leave", web::post().to(channels_routes::leave_channel))
                        .route("/{id}/members", web::get().to(channels_routes::list_members))
                        .route("/{id}/members", web::post().to(channels_routes::modify_members))
                        .route("/{id}/messages", web::get().to(messages_routes::list_messages))
                        .route("/{id}/messages", web::post().to(messages_routes::post_message))

                    )
                    // Add top-level messages edit/delete endpoints
                    .route("/messages/{id}", web::patch().to(messages_routes::edit_message))
                    .route("/messages/{id}", web::delete().to(messages_routes::delete_message))
                    // Presence API
                    .service(
                        web::scope("/presence")
                            .route("/heartbeat", web::post().to(routes::presence::heartbeat))
                            .route("/users", web::get().to(routes::presence::get_users_presence))
                    )
                    .service(
                        web::scope("/files")
                            .route("", web::post().to(files_routes::upload_file))
                    )
            )
            .route("/ws", web::get().to(ws::session::ws_route))
            .route("/files/{id}/{filename:.*}", web::get().to(files_routes::get_file))
    })
    .bind(listen_addr)?
    .run()
    .await
}