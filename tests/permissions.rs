mod common;

use actix_web::{test, web, App, http::StatusCode};
use common::{auth_token, insert_user, test_context};
use serde_json::Value;
use stuffchat::db::Db;
use stuffchat::models::role::{
    PERM_ADMIN_ALL, PERM_CREATE_CHANNELS, PERM_INVITE_USERS, PERM_MANAGE_EMOJIS,
    PERM_POST_MESSAGES, PERM_UPLOAD_FILES,
};

const MULTIPART_BOUNDARY: &str = "----stuffchat-perms-boundary";
const SMALL_PNG_BYTES: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2,
    0, 0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 80, 76, 84, 69, 0, 0, 0, 255, 255, 255, 255, 33,
    33, 33, 33, 0, 0, 0, 0, 73, 68, 65, 84, 120, 156, 99, 96, 0, 0, 0, 2, 0, 1, 113, 47, 230,
    173, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

enum MultipartPart<'a> {
    Text {
        name: &'a str,
        value: &'a str,
    },
    File {
        name: &'a str,
        filename: &'a str,
        content_type: &'a str,
        content: &'a [u8],
    },
}

fn build_multipart_body(boundary: &str, parts: &[MultipartPart<'_>]) -> Vec<u8> {
    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match part {
            MultipartPart::Text { name, value } => {
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"\r\n\r\n"
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(value.as_bytes());
            }
            MultipartPart::File {
                name,
                filename,
                content_type,
                content,
            } => {
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(content);
            }
        }
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

async fn insert_permission_role(db: &Db, id: &str, name: &str, permissions: i64) {
    sqlx::query(
        "INSERT INTO roles(id, name, permissions, created_at) VALUES (?, ?, ?, ?)
         ON CONFLICT(name) DO UPDATE SET id = excluded.id, permissions = excluded.permissions",
    )
        .bind(id)
        .bind(name)
        .bind(permissions)
        .bind(chrono::Utc::now())
        .execute(&db.0)
        .await
        .expect("insert permission role");
}

fn assert_is_owner_flag(payload: Value, expected: bool) {
    assert_eq!(payload["is_owner"].as_bool(), Some(expected));
}

#[actix_web::test]
async fn api_channel_creation_requires_create_channel_permission() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "user-create", "creator").await;

    let token = auth_token(&ctx.cfg, "user-create");
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.cfg.clone()))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .app_data(web::Data::new(None::<stuffchat::push::PushRelayRuntime>))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;

    let without_permission = test::TestRequest::post()
        .uri("/api/channels")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(serde_json::json!({
            "name": "no-perm-room",
            "is_private": false,
            "is_voice": false,
        }))
        .to_request();
    let response = test::call_service(&app, without_permission).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    insert_permission_role(&ctx.db, "role-create", "can-create-channels", PERM_CREATE_CHANNELS).await;
    sqlx::query("INSERT INTO user_roles(user_id, role_id) VALUES (?, ?)")
        .bind("user-create")
        .bind("role-create")
        .execute(&ctx.db.0)
        .await
        .expect("assign role");

    let with_permission = test::TestRequest::post()
        .uri("/api/channels")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(serde_json::json!({
            "name": "with-perm-room",
            "is_private": false,
            "is_voice": false,
        }))
        .to_request();
    let response = test::call_service(&app, with_permission).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[actix_web::test]
async fn api_admin_routes_require_admin_permission_bit_not_name_only() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "admin-name-only", "admin-name-only").await;

    insert_permission_role(&ctx.db, "legacy-admin", "admin", 0).await;
    sqlx::query("INSERT INTO user_roles(user_id, role_id) VALUES (?, ?)")
        .bind("admin-name-only")
        .bind("legacy-admin")
        .execute(&ctx.db.0)
        .await
        .expect("assign legacy admin role");

    let token = auth_token(&ctx.cfg, "admin-name-only");
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.cfg.clone()))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .app_data(web::Data::new(None::<stuffchat::push::PushRelayRuntime>))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;

    let blocked_request = test::TestRequest::get()
        .uri("/api/admin/logs?limit=1")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let response = test::call_service(&app, blocked_request).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    insert_permission_role(&ctx.db, "strong-admin", "super-admin", PERM_ADMIN_ALL).await;
    sqlx::query("INSERT INTO user_roles(user_id, role_id) VALUES (?, ?)")
        .bind("admin-name-only")
        .bind("strong-admin")
        .execute(&ctx.db.0)
        .await
        .expect("assign admin bit role");

    let allowed_request = test::TestRequest::get()
        .uri("/api/admin/logs?limit=1")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let response = test::call_service(&app, allowed_request).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[actix_web::test]
async fn api_posting_requires_post_message_capability() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "channel-owner", "channel-owner").await;
    insert_user(&ctx.db, "poster", "poster").await;
    insert_permission_role(&ctx.db, "role-create", "can-create", PERM_CREATE_CHANNELS).await;
    insert_permission_role(&ctx.db, "role-post", "can-post", PERM_POST_MESSAGES).await;

    sqlx::query("INSERT INTO user_roles(user_id, role_id) VALUES (?, ?)")
        .bind("channel-owner")
        .bind("role-create")
        .execute(&ctx.db.0)
        .await
        .expect("assign channel owner role");

    let owner_token = auth_token(&ctx.cfg, "channel-owner");
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.cfg.clone()))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .app_data(web::Data::new(None::<stuffchat::push::PushRelayRuntime>))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;

    let create_channel = test::TestRequest::post()
        .uri("/api/channels")
        .insert_header(("Authorization", format!("Bearer {owner_token}")))
        .set_json(serde_json::json!({
            "name": "chat",
            "is_private": true,
            "is_voice": false,
        }))
        .to_request();
    let response: Value = test::call_and_read_body_json(&app, create_channel).await;
    let channel_id = response["id"].as_str().expect("channel id").to_string();

    sqlx::query(
        "INSERT INTO channel_members(channel_id, user_id, can_read, can_write, can_manage) VALUES (?, ?, 1, 1, 0)",
    )
    .bind(&channel_id)
    .bind("poster")
    .execute(&ctx.db.0)
    .await
    .expect("insert poster membership");

    let poster_token = auth_token(&ctx.cfg, "poster");
    let denied = test::TestRequest::post()
        .uri(&format!("/api/channels/{channel_id}/messages"))
        .insert_header(("Authorization", format!("Bearer {poster_token}")))
        .set_json(serde_json::json!({
            "content": "no permission"
        }))
        .to_request();
    let denied_response = test::call_service(&app, denied).await;
    assert_eq!(denied_response.status(), StatusCode::FORBIDDEN);

    sqlx::query("INSERT INTO user_roles(user_id, role_id) VALUES (?, ?)")
        .bind("poster")
        .bind("role-post")
        .execute(&ctx.db.0)
        .await
        .expect("assign post capability");

    let allowed = test::TestRequest::post()
        .uri(&format!("/api/channels/{channel_id}/messages"))
        .insert_header(("Authorization", format!("Bearer {poster_token}")))
        .set_json(serde_json::json!({
            "content": "allowed"
        }))
        .to_request();
    let allowed_response = test::call_service(&app, allowed).await;
    assert_ne!(allowed_response.status(), StatusCode::FORBIDDEN);
}

#[actix_web::test]
async fn api_file_upload_requires_upload_permission() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "uploader", "uploader").await;
    let token = auth_token(&ctx.cfg, "uploader");
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.cfg.clone()))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .app_data(web::Data::new(None::<stuffchat::push::PushRelayRuntime>))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;

    let body = build_multipart_body(
        MULTIPART_BOUNDARY,
        &[MultipartPart::File {
            name: "file",
            filename: "notes.txt",
            content_type: "text/plain",
            content: b"hello world",
        }],
    );
    let denied = test::TestRequest::post()
        .uri("/api/files")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .insert_header((
            "Content-Type",
            format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
        ))
        .set_payload(body.clone())
        .to_request();
    let denied_response = test::call_service(&app, denied).await;
    assert_eq!(denied_response.status(), StatusCode::FORBIDDEN);

    std::fs::create_dir_all(&ctx.cfg.uploads_dir).expect("create uploads dir");
    insert_permission_role(&ctx.db, "upload-role", "can-upload", PERM_UPLOAD_FILES).await;
    sqlx::query("INSERT INTO user_roles(user_id, role_id) VALUES (?, ?)")
        .bind("uploader")
        .bind("upload-role")
        .execute(&ctx.db.0)
        .await
        .expect("assign upload capability");

    let allowed = test::TestRequest::post()
        .uri("/api/files")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .insert_header((
            "Content-Type",
            format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
        ))
        .set_payload(body)
        .to_request();
    let allowed_response = test::call_service(&app, allowed).await;
    assert_eq!(allowed_response.status(), StatusCode::OK);
}

#[actix_web::test]
async fn api_invite_creation_requires_invite_permission() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "inviter", "inviter").await;
    let token = auth_token(&ctx.cfg, "inviter");

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.cfg.clone()))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .app_data(web::Data::new(None::<stuffchat::push::PushRelayRuntime>))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;

    let denied = test::TestRequest::post()
        .uri("/api/invites")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let denied_response = test::call_service(&app, denied).await;
    assert_eq!(denied_response.status(), StatusCode::FORBIDDEN);

    insert_permission_role(&ctx.db, "invite-role", "can-invite", PERM_INVITE_USERS).await;
    sqlx::query("INSERT INTO user_roles(user_id, role_id) VALUES (?, ?)")
        .bind("inviter")
        .bind("invite-role")
        .execute(&ctx.db.0)
        .await
        .expect("assign invite capability");

    let allowed = test::TestRequest::post()
        .uri("/api/invites")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let allowed_json: Value = test::call_and_read_body_json(&app, allowed).await;
    assert!(allowed_json["code"].as_str().is_some());
}

#[actix_web::test]
async fn api_emoji_upload_requires_manage_emojis_permission() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "emoji-creator", "emoji-creator").await;
    let token = auth_token(&ctx.cfg, "emoji-creator");

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.cfg.clone()))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .app_data(web::Data::new(None::<stuffchat::push::PushRelayRuntime>))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;

    let body = build_multipart_body(
        MULTIPART_BOUNDARY,
        &[
            MultipartPart::Text {
                name: "name",
                value: "smile",
            },
            MultipartPart::File {
                name: "file",
                filename: "smile.png",
                content_type: "image/png",
                content: SMALL_PNG_BYTES,
            },
        ],
    );

    let denied = test::TestRequest::post()
        .uri("/api/emojis")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .insert_header((
            "Content-Type",
            format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
        ))
        .set_payload(body.clone())
        .to_request();
    let denied_response = test::call_service(&app, denied).await;
    assert_eq!(denied_response.status(), StatusCode::FORBIDDEN);

    std::fs::create_dir_all(&ctx.cfg.uploads_dir).expect("create uploads dir");
    insert_permission_role(&ctx.db, "emoji-role", "can-emojis", PERM_MANAGE_EMOJIS).await;
    sqlx::query("INSERT INTO user_roles(user_id, role_id) VALUES (?, ?)")
        .bind("emoji-creator")
        .bind("emoji-role")
        .execute(&ctx.db.0)
        .await
        .expect("assign emoji capability");

    let allowed = test::TestRequest::post()
        .uri("/api/emojis")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .insert_header((
            "Content-Type",
            format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
        ))
        .set_payload(body)
        .to_request();
    let allowed_response = test::call_service(&app, allowed).await;
    assert_ne!(allowed_response.status(), StatusCode::FORBIDDEN);
}

#[actix_web::test]
async fn api_channel_owners_can_manage_without_global_manage_channel_permissions() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "channel-owner", "channel-owner").await;
    insert_user(&ctx.db, "outsider", "outsider").await;

    insert_permission_role(
        &ctx.db,
        "owner-role",
        "owner-role",
        PERM_CREATE_CHANNELS,
    )
    .await;
    sqlx::query("INSERT INTO user_roles(user_id, role_id) VALUES (?, ?)")
        .bind("channel-owner")
        .bind("owner-role")
        .execute(&ctx.db.0)
        .await
        .expect("assign owner role");

    let owner_token = auth_token(&ctx.cfg, "channel-owner");
    let outsider_token = auth_token(&ctx.cfg, "outsider");

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.cfg.clone()))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .app_data(web::Data::new(None::<stuffchat::push::PushRelayRuntime>))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;

    let create_request = test::TestRequest::post()
        .uri("/api/channels")
        .insert_header(("Authorization", format!("Bearer {owner_token}")))
        .set_json(serde_json::json!({
            "name": "owner-room",
            "is_private": false,
            "is_voice": false,
        }))
        .to_request();
    let response: Value = test::call_and_read_body_json(&app, create_request).await;
    let channel_id = response["id"].as_str().expect("channel id").to_string();

    let ownership = test::TestRequest::get()
        .uri(&format!("/api/channels/{channel_id}/ownership"))
        .insert_header(("Authorization", format!("Bearer {owner_token}")))
        .to_request();
    let owner_payload: Value = test::call_and_read_body_json(&app, ownership).await;
    assert_is_owner_flag(owner_payload, true);

    let outsider_ownership = test::TestRequest::get()
        .uri(&format!("/api/channels/{channel_id}/ownership"))
        .insert_header(("Authorization", format!("Bearer {outsider_token}")))
        .to_request();
    let outsider_payload: Value = test::call_and_read_body_json(&app, outsider_ownership).await;
    assert_is_owner_flag(outsider_payload, false);

    let outsider_edit = test::TestRequest::patch()
        .uri(&format!("/api/channels/{channel_id}"))
        .insert_header(("Authorization", format!("Bearer {outsider_token}")))
        .set_json(serde_json::json!({
            "name": "edited-by-outsider",
        }))
        .to_request();
    let outsider_edit_response = test::call_service(&app, outsider_edit).await;
    assert_eq!(outsider_edit_response.status(), StatusCode::FORBIDDEN);

    let owner_edit = test::TestRequest::patch()
        .uri(&format!("/api/channels/{channel_id}"))
        .insert_header(("Authorization", format!("Bearer {owner_token}")))
        .set_json(serde_json::json!({
            "name": "owner-edited",
            "is_private": true,
        }))
        .to_request();
    let owner_edit_response = test::call_service(&app, owner_edit).await;
    assert_eq!(owner_edit_response.status(), StatusCode::OK);

    let owner_remove_member = test::TestRequest::post()
        .uri(&format!("/api/channels/{channel_id}/members"))
        .insert_header(("Authorization", format!("Bearer {owner_token}")))
        .set_json(serde_json::json!({
            "remove": ["outsider"]
        }))
        .to_request();
    let owner_remove_member_response = test::call_service(&app, owner_remove_member).await;
    assert_eq!(owner_remove_member_response.status(), StatusCode::OK);

    let owner_delete = test::TestRequest::delete()
        .uri(&format!("/api/channels/{channel_id}"))
        .insert_header(("Authorization", format!("Bearer {owner_token}")))
        .to_request();
    let owner_delete_response = test::call_service(&app, owner_delete).await;
    assert_eq!(owner_delete_response.status(), StatusCode::OK);
}
