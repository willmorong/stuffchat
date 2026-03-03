use crate::routes::{
    admin as admin_routes, auth as auth_routes, bridge as bridge_routes,
    channels as channels_routes, emojis as emojis_routes, files as files_routes,
    invites as invites_routes, messages as messages_routes, reactions as reactions_routes,
    users as users_routes,
};
use crate::ws;
use actix_web::web;

pub fn configure(cfg: &mut web::ServiceConfig, bridge_enabled: bool) {
    cfg.service(web::scope("/api").configure(|api| configure_api(api, bridge_enabled)))
        .route("/ws", web::get().to(ws::session::ws_route))
        .service(
            web::resource("/files/{id}/{filename:.*}")
                .route(web::get().to(files_routes::get_file))
                .route(web::head().to(files_routes::get_file)),
        )
        .route(
            "/emojis/{name}/image",
            web::get().to(emojis_routes::get_emoji_image),
        );
}

fn configure_api(cfg: &mut web::ServiceConfig, bridge_enabled: bool) {
    cfg.route(
        "/health",
        web::get().to(crate::routes::health::health_check),
    )
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
        web::scope("/admin")
            .route("/users", web::get().to(admin_routes::list_users))
            .route("/users/{id}", web::patch().to(admin_routes::update_user))
            .route(
                "/users/{id}/password",
                web::put().to(admin_routes::set_user_password),
            )
            .route(
                "/users/{id}/avatar",
                web::put().to(admin_routes::upload_user_avatar),
            )
            .route(
                "/users/{id}/roles",
                web::put().to(admin_routes::update_user_roles),
            )
            .route("/roles", web::get().to(admin_routes::list_roles))
            .route("/roles", web::post().to(admin_routes::create_role))
            .route("/roles/{id}", web::delete().to(admin_routes::delete_role)),
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
    .route(
        "/messages/search",
        web::get().to(messages_routes::search_messages),
    )
    .route(
        "/messages/{id}/context",
        web::get().to(messages_routes::get_message_context),
    )
    .route(
        "/messages/{id}",
        web::get().to(messages_routes::get_message),
    )
    .route(
        "/messages/{id}",
        web::patch().to(messages_routes::edit_message),
    )
    .route(
        "/messages/{id}",
        web::delete().to(messages_routes::delete_message),
    )
    .route(
        "/messages/{id}/reactions",
        web::get().to(reactions_routes::list_reactions),
    )
    .route(
        "/messages/{id}/reactions/{emoji}",
        web::put().to(reactions_routes::toggle_reaction),
    )
    .service(
        web::scope("/presence")
            .route(
                "/heartbeat",
                web::post().to(crate::routes::presence::heartbeat),
            )
            .route(
                "/users",
                web::get().to(crate::routes::presence::get_users_presence),
            ),
    )
    .service(
        web::scope("/invites")
            .route("", web::post().to(invites_routes::create_invite))
            .route("", web::get().to(invites_routes::list_my_invites)),
    )
    .service(web::scope("/files").route("", web::post().to(files_routes::upload_file)))
    .service(
        web::scope("/emojis")
            .route("", web::get().to(emojis_routes::list_emojis))
            .route("", web::post().to(emojis_routes::upload_emoji))
            .route("/{name}", web::delete().to(emojis_routes::delete_emoji)),
    )
    .route(
        "/shareplay/{channel_id}/current",
        web::get().to(crate::routes::shareplay::get_current_track),
    )
    .route(
        "/shareplay/song/{song_id}",
        web::get().to(crate::routes::shareplay::get_song_by_id),
    )
    .route(
        "/shareplay/thumbnail/{item_id}",
        web::get().to(crate::routes::shareplay::get_thumbnail_by_id),
    );

    if bridge_enabled {
        cfg.service(web::scope("/bridge").configure(bridge_routes::configure));
    }
}
