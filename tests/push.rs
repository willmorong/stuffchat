mod common;

use actix_web::{App, http::StatusCode, test, web};
use common::{auth_token, insert_user, test_context};
use stuffchat::push::PushRelayRuntime;

#[actix_web::test]
async fn capabilities_reflect_runtime_presence() {
    let ctx = test_context().await;

    let disabled_app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.cfg.clone()))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .app_data(web::Data::new(None::<PushRelayRuntime>))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;
    let disabled_request = test::TestRequest::get()
        .uri("/api/push/capabilities")
        .to_request();
    let disabled_response = test::call_service(&disabled_app, disabled_request).await;
    assert_eq!(disabled_response.status(), StatusCode::OK);
    let disabled_body: serde_json::Value = test::read_body_json(disabled_response).await;
    assert_eq!(disabled_body["enabled"], false);

    let mut enabled_cfg = ctx.cfg.clone();
    enabled_cfg.push_relay_enabled = true;
    enabled_cfg.push_relay_url = Some("http://127.0.0.1:9".to_string());
    enabled_cfg.push_relay_server_id = Some("server-1".to_string());
    enabled_cfg.push_relay_server_secret = Some("secret".to_string());
    let runtime = PushRelayRuntime::new(&enabled_cfg, ctx.db.clone()).expect("push runtime");

    let enabled_app = test::init_service(
        App::new()
            .app_data(web::Data::new(enabled_cfg))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .app_data(web::Data::new(Some(runtime)))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;
    let enabled_request = test::TestRequest::get()
        .uri("/api/push/capabilities")
        .to_request();
    let enabled_response = test::call_service(&enabled_app, enabled_request).await;
    assert_eq!(enabled_response.status(), StatusCode::OK);
    let enabled_body: serde_json::Value = test::read_body_json(enabled_response).await;
    assert_eq!(enabled_body["enabled"], true);
    assert_eq!(enabled_body["ios"]["enabled"], true);
}

#[actix_web::test]
async fn push_device_endpoints_upsert_and_delete_current_installation() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "user-1", "alice").await;
    let token = auth_token(&ctx.cfg, "user-1");

    let mut enabled_cfg = ctx.cfg.clone();
    enabled_cfg.push_relay_enabled = true;
    enabled_cfg.push_relay_url = Some("http://127.0.0.1:9".to_string());
    enabled_cfg.push_relay_server_id = Some("server-1".to_string());
    enabled_cfg.push_relay_server_secret = Some("secret".to_string());
    let runtime = PushRelayRuntime::new(&enabled_cfg, ctx.db.clone()).expect("push runtime");

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(enabled_cfg))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .app_data(web::Data::new(Some(runtime)))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;

    let upsert_request = test::TestRequest::put()
        .uri("/api/push/devices/current")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(serde_json::json!({
            "installation_id": "installation-1",
            "platform": "ios",
            "push_token": "push-token-1",
            "environment": "development",
            "message_notifications": true,
            "call_notifications": true
        }))
        .to_request();
    let upsert_response = test::call_service(&app, upsert_request).await;
    assert_eq!(upsert_response.status(), StatusCode::OK);

    let stored_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM push_devices WHERE installation_id = ?")
            .bind("installation-1")
            .fetch_one(&ctx.db.0)
            .await
            .expect("stored count");
    assert_eq!(stored_count, 1);

    let delete_request = test::TestRequest::delete()
        .uri("/api/push/devices/current")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(serde_json::json!({
            "installation_id": "installation-1"
        }))
        .to_request();
    let delete_response = test::call_service(&app, delete_request).await;
    assert_eq!(delete_response.status(), StatusCode::OK);

    let remaining_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM push_devices WHERE installation_id = ?")
            .bind("installation-1")
            .fetch_one(&ctx.db.0)
            .await
            .expect("remaining count");
    assert_eq!(remaining_count, 0);
}
