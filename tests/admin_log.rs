mod common;

use actix_web::{App, http::StatusCode, test, web};
use common::{auth_token, grant_role, insert_channel, insert_role, insert_user, test_context};
use serde_json::{Value, json};
use sqlx::Row;

fn find_log<'a>(logs: &'a [Value], action_type: &str) -> &'a Value {
    logs.iter()
        .find(|entry| entry["action_type"] == action_type)
        .unwrap_or_else(|| panic!("missing log entry for {action_type}"))
}

#[actix_web::test]
async fn migration_creates_admin_log_table() {
    let ctx = test_context().await;
    let row =
        sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'admin_log'")
            .fetch_optional(&ctx.db.0)
            .await
            .expect("query sqlite_master");

    assert_eq!(
        row.expect("admin_log table should exist")
            .get::<String, _>("name"),
        "admin_log"
    );
}

#[actix_web::test]
async fn admin_log_endpoint_requires_admin_role() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "user-1", "user1").await;

    let token = auth_token(&ctx.cfg, "user-1");
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.cfg.clone()))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;

    let request = test::TestRequest::get()
        .uri("/api/admin/logs")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[actix_web::test]
async fn admin_actions_are_logged_without_password_data() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "admin-1", "admin").await;
    insert_user(&ctx.db, "user-2", "target").await;
    insert_role(&ctx.db, "role-admin", "admin").await;
    grant_role(&ctx.db, "admin-1", "role-admin").await;

    let admin_token = auth_token(&ctx.cfg, "admin-1");
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.cfg.clone()))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;

    let update_user_request = test::TestRequest::patch()
        .uri("/api/admin/users/user-2")
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .set_json(json!({
            "username": "target-renamed",
            "email": "renamed@example.com"
        }))
        .to_request();
    let update_user_response = test::call_service(&app, update_user_request).await;
    assert_eq!(update_user_response.status(), StatusCode::OK);

    let create_role_request = test::TestRequest::post()
        .uri("/api/admin/roles")
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .set_json(json!({
            "name": "moderator"
        }))
        .to_request();
    let create_role_response: Value =
        test::call_and_read_body_json(&app, create_role_request).await;
    let created_role_id = create_role_response["id"]
        .as_str()
        .expect("role id")
        .to_string();

    let update_roles_request = test::TestRequest::put()
        .uri("/api/admin/users/user-2/roles")
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .set_json(json!({
            "role_ids": [created_role_id]
        }))
        .to_request();
    let update_roles_response = test::call_service(&app, update_roles_request).await;
    assert_eq!(update_roles_response.status(), StatusCode::OK);

    let password_request = test::TestRequest::put()
        .uri("/api/admin/users/user-2/password")
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .set_json(json!({
            "new_password": "super-secret-password"
        }))
        .to_request();
    let password_response = test::call_service(&app, password_request).await;
    assert_eq!(password_response.status(), StatusCode::OK);

    let logs_request = test::TestRequest::get()
        .uri("/api/admin/logs?limit=20")
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .to_request();
    let logs: Vec<Value> = test::call_and_read_body_json(&app, logs_request).await;

    assert!(logs.len() >= 4);
    assert_eq!(logs[0]["action_type"], "user.password_set");

    let update_entry = find_log(&logs, "user.updated");
    assert_eq!(update_entry["actor"]["username"], "admin");
    assert_eq!(update_entry["action_info"]["target_user_id"], "user-2");
    assert_eq!(
        update_entry["action_info"]["changes"]["username"],
        "target-renamed"
    );

    let role_created_entry = find_log(&logs, "role.created");
    assert_eq!(role_created_entry["action_info"]["role_name"], "moderator");

    let roles_updated_entry = find_log(&logs, "user.roles_updated");
    assert_eq!(
        roles_updated_entry["action_info"]["target_user_id"],
        "user-2"
    );
    assert_eq!(
        roles_updated_entry["action_info"]["new_roles"][0]["name"],
        "moderator"
    );

    let password_entry = find_log(&logs, "user.password_set");
    let password_text = password_entry["action_info"].to_string();
    assert!(!password_text.contains("super-secret-password"));
    assert!(!password_text.contains("argon2"));
}

#[actix_web::test]
async fn channel_owner_actions_are_logged() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "admin-1", "admin").await;
    insert_user(&ctx.db, "owner-1", "owner").await;
    insert_user(&ctx.db, "member-1", "member").await;
    insert_role(&ctx.db, "role-admin", "admin").await;
    grant_role(&ctx.db, "admin-1", "role-admin").await;

    let admin_token = auth_token(&ctx.cfg, "admin-1");
    let owner_token = auth_token(&ctx.cfg, "owner-1");
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.cfg.clone()))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;

    let create_request = test::TestRequest::post()
        .uri("/api/channels")
        .insert_header(("Authorization", format!("Bearer {owner_token}")))
        .set_json(json!({
            "name": "ops-room",
            "is_private": true,
            "is_voice": false,
            "members": ["member-1"]
        }))
        .to_request();
    let create_response: Value = test::call_and_read_body_json(&app, create_request).await;
    let channel_id = create_response["id"]
        .as_str()
        .expect("channel id")
        .to_string();

    let edit_request = test::TestRequest::patch()
        .uri(&format!("/api/channels/{channel_id}"))
        .insert_header(("Authorization", format!("Bearer {owner_token}")))
        .set_json(json!({
            "name": "ops-updated",
            "is_private": false,
            "is_voice": true
        }))
        .to_request();
    let edit_response = test::call_service(&app, edit_request).await;
    assert_eq!(edit_response.status(), StatusCode::OK);

    let members_request = test::TestRequest::post()
        .uri(&format!("/api/channels/{channel_id}/members"))
        .insert_header(("Authorization", format!("Bearer {owner_token}")))
        .set_json(json!({
            "remove": ["member-1"]
        }))
        .to_request();
    let members_response = test::call_service(&app, members_request).await;
    assert_eq!(members_response.status(), StatusCode::OK);

    let delete_request = test::TestRequest::delete()
        .uri(&format!("/api/channels/{channel_id}"))
        .insert_header(("Authorization", format!("Bearer {owner_token}")))
        .to_request();
    let delete_response = test::call_service(&app, delete_request).await;
    assert_eq!(delete_response.status(), StatusCode::OK);

    let logs_request = test::TestRequest::get()
        .uri("/api/admin/logs?limit=20")
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .to_request();
    let logs: Vec<Value> = test::call_and_read_body_json(&app, logs_request).await;

    let created_entry = find_log(&logs, "channel.created");
    assert_eq!(created_entry["actor"]["username"], "owner");
    assert_eq!(created_entry["action_info"]["channel_name"], "ops-room");
    assert_eq!(
        created_entry["action_info"]["initial_member_ids"][0],
        "member-1"
    );

    let updated_entry = find_log(&logs, "channel.updated");
    assert_eq!(updated_entry["action_info"]["channel_id"], channel_id);
    assert_eq!(
        updated_entry["action_info"]["changes"]["name"],
        "ops-updated"
    );
    assert_eq!(updated_entry["action_info"]["changes"]["is_voice"], true);
    assert_eq!(updated_entry["action_info"]["changes"]["is_private"], false);

    let members_entry = find_log(&logs, "channel.members_modified");
    assert_eq!(
        members_entry["action_info"]["removed_user_ids"][0],
        "member-1"
    );

    let deleted_entry = find_log(&logs, "channel.deleted");
    assert_eq!(deleted_entry["action_info"]["channel_id"], channel_id);
    assert_eq!(deleted_entry["action_info"]["channel_name"], "ops-updated");
}

#[actix_web::test]
async fn message_flags_are_logged_for_admin_review() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "admin-1", "admin").await;
    insert_user(&ctx.db, "sender-1", "sender").await;
    insert_user(&ctx.db, "flagger-1", "flagger").await;
    insert_role(&ctx.db, "role-admin", "admin").await;
    grant_role(&ctx.db, "admin-1", "role-admin").await;
    insert_channel(&ctx.db, "channel-1", "general", "sender-1", false).await;

    sqlx::query(
        "INSERT INTO channel_members(channel_id, user_id, can_read, can_write, can_manage) VALUES (?, ?, 1, 1, 0)",
    )
    .bind("channel-1")
    .bind("flagger-1")
    .execute(&ctx.db.0)
    .await
    .expect("insert flagger member");

    sqlx::query(
        "INSERT INTO messages(id, channel_id, user_id, content, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind("message-1")
    .bind("channel-1")
    .bind("sender-1")
    .bind("flag this message")
    .bind(chrono::Utc::now())
    .execute(&ctx.db.0)
    .await
    .expect("insert message");

    let admin_token = auth_token(&ctx.cfg, "admin-1");
    let flagger_token = auth_token(&ctx.cfg, "flagger-1");
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.cfg.clone()))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;

    let flag_request = test::TestRequest::post()
        .uri("/api/messages/message-1/flag")
        .insert_header(("Authorization", format!("Bearer {flagger_token}")))
        .to_request();
    let flag_response = test::call_service(&app, flag_request).await;
    assert_eq!(flag_response.status(), StatusCode::OK);

    let logs_request = test::TestRequest::get()
        .uri("/api/admin/logs?limit=20")
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .to_request();
    let logs: Vec<Value> = test::call_and_read_body_json(&app, logs_request).await;

    let flagged_entry = find_log(&logs, "message.flagged");
    assert_eq!(flagged_entry["actor"]["id"], "flagger-1");
    assert_eq!(flagged_entry["actor"]["username"], "flagger");
    assert_eq!(flagged_entry["action_info"]["message_id"], "message-1");
    assert_eq!(flagged_entry["action_info"]["channel_id"], "channel-1");
    assert_eq!(
        flagged_entry["action_info"]["message_content"],
        "flag this message"
    );
    assert_eq!(
        flagged_entry["action_info"]["message_sender"]["id"],
        "sender-1"
    );
    assert_eq!(
        flagged_entry["action_info"]["message_sender"]["username"],
        "sender"
    );
}

#[actix_web::test]
async fn flag_endpoint_requires_message_read_access() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "sender-1", "sender").await;
    insert_user(&ctx.db, "outsider-1", "outsider").await;
    insert_channel(&ctx.db, "channel-1", "general", "sender-1", false).await;

    sqlx::query(
        "INSERT INTO messages(id, channel_id, user_id, content, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind("message-1")
    .bind("channel-1")
    .bind("sender-1")
    .bind("private note")
    .bind(chrono::Utc::now())
    .execute(&ctx.db.0)
    .await
    .expect("insert message");

    let outsider_token = auth_token(&ctx.cfg, "outsider-1");
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.cfg.clone()))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;

    let flag_request = test::TestRequest::post()
        .uri("/api/messages/message-1/flag")
        .insert_header(("Authorization", format!("Bearer {outsider_token}")))
        .to_request();
    let flag_response = test::call_service(&app, flag_request).await;
    assert_eq!(flag_response.status(), StatusCode::FORBIDDEN);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM admin_log")
        .fetch_one(&ctx.db.0)
        .await
        .expect("count admin logs");
    assert_eq!(count, 0);
}
